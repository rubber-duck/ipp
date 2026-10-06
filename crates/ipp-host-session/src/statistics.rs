//! Ordinary layout work at the whole-Host evaluation boundary, independent of presentation.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use ipp_core::systems::gui::layout::{GuiEntityLayoutStatistics, GuiEntityLayoutWork};
use ipp_core::{HostFrameReport, HostRuntime, WorldId, WorldRef};

struct WorldLayoutSample {
    world: WorldRef,
    tick: u64,
    status: &'static str,
    statistics: Option<GuiEntityLayoutStatistics>,
}

/// Every live World is sampled once. Retired lifetime totals remain in Host totals.
#[derive(Default)]
pub struct HostGuiLayoutStatistics {
    frame: Option<u64>,
    worlds: Vec<WorldLayoutSample>,
    identities: Vec<WorldId>,
    previous: BTreeMap<WorldRef, GuiEntityLayoutWork>,
    retired: GuiEntityLayoutWork,
    retired_worlds: u64,
    latest: GuiEntityLayoutWork,
    total: Option<GuiEntityLayoutWork>,
    discontinuous: bool,
    unavailable: bool,
}

fn add(total: &mut GuiEntityLayoutWork, work: GuiEntityLayoutWork) {
    total.reflows = total.reflows.saturating_add(work.reflows);
    total.visited_entities = total.visited_entities.saturating_add(work.visited_entities);
    total.text_measurements = total
        .text_measurements
        .saturating_add(work.text_measurements);
    total.reused_texts = total.reused_texts.saturating_add(work.reused_texts);
}

fn work_json(output: &mut String, work: GuiEntityLayoutWork) {
    write!(
        output,
        "{{\"reflows\":{},\"visitedEntities\":{},\"textMeasurements\":{},\"reusedTexts\":{}}}",
        word(work.reflows),
        word(work.visited_entities),
        word(work.text_measurements),
        word(work.reused_texts)
    )
    .expect("string write");
}

fn word(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

impl HostGuiLayoutStatistics {
    /// Observe actual evaluations, including non-presented Worlds; unevaluated Worlds never repeat old latest work.
    pub fn record(&mut self, host: &mut HostRuntime, frame: &HostFrameReport) {
        if self.frame == Some(frame.frame) {
            return;
        }

        self.frame = Some(frame.frame);
        self.worlds.clear();
        self.identities.clear();
        self.identities.extend(host.world_ids());
        self.latest = GuiEntityLayoutWork::default();
        self.unavailable = false;
        let mut current = BTreeSet::new();

        for &identity in &self.identities {
            let context = host
                .world_mut(identity)
                .expect("live Host World under exclusive access");
            let world = context.world_ref();
            let evaluated = frame.evaluation_order.contains(&identity);
            let status = if evaluated {
                "evaluated"
            } else {
                "unavailable"
            };
            let statistics = context.gui_entity_layout_statistics();

            if let Some(statistics) = statistics {
                self.unavailable |= status == "unavailable";
                current.insert(world);
                if let Some(previous) = self.previous.get(&world) {
                    self.discontinuous |= statistics.total.reflows < previous.reflows
                        || statistics.total.visited_entities < previous.visited_entities
                        || statistics.total.text_measurements < previous.text_measurements
                        || statistics.total.reused_texts < previous.reused_texts;
                }
                self.previous.insert(world, statistics.total);
                if evaluated {
                    add(&mut self.latest, statistics.latest);
                }
            }

            self.worlds.push(WorldLayoutSample {
                world,
                tick: context.tick(),
                status,
                statistics,
            });
        }

        self.previous.retain(|world, total| {
            if current.contains(world) {
                true
            } else {
                add(&mut self.retired, *total);
                self.retired_worlds = self.retired_worlds.saturating_add(1);
                false
            }
        });

        self.total = (!self.previous.is_empty() || self.retired_worlds > 0).then(|| {
            let mut total = self.retired;
            for &work in self.previous.values() {
                add(&mut total, work);
            }
            total
        });
    }

    /// Saturating legacy report fields, now explicitly whole-Host ordinary-layout work.
    pub fn words(&self) -> Option<[u32; 4]> {
        if self.discontinuous || self.unavailable {
            return None;
        }

        self.total.map(|total| {
            [
                word(self.latest.reflows),
                word(self.latest.text_measurements),
                word(total.reflows),
                word(total.text_measurements),
            ]
        })
    }

    /// Diagnostic membership and evaluation evidence; counters saturate at u32 like render counters.
    pub fn write_json(&self, output: &mut String) {
        let Some(frame) = self.frame else {
            output.push_str("null");
            return;
        };

        write!(output, "{{\"scope\":\"host\",\"frame\":\"{frame}\",\"complete\":{},\"retiredWorlds\":{},\"retired\":", !self.discontinuous && !self.unavailable, self.retired_worlds).expect("string write");
        work_json(output, self.retired);
        output.push_str(",\"latest\":");
        if self.words().is_some() {
            work_json(output, self.latest);
        } else {
            output.push_str("null");
        }
        output.push_str(",\"total\":");
        if let Some(total) = self.total.filter(|_| self.words().is_some()) {
            work_json(output, total);
        } else {
            output.push_str("null");
        }
        output.push_str(",\"worlds\":[");
        for (index, sample) in self.worlds.iter().enumerate() {
            write!(output, "{}{{\"world\":{{\"id\":\"{}\",\"incarnation\":\"{}\"}},\"tick\":\"{}\",\"status\":\"{}\",\"layout\":", if index == 0 { "" } else { "," }, sample.world.id().0, sample.world.incarnation(), sample.tick, sample.status).expect("string write");
            if let Some(statistics) = sample.statistics {
                output.push_str("{\"latest\":");
                if sample.status == "evaluated" {
                    work_json(output, statistics.latest);
                } else {
                    output.push_str("null");
                }
                output.push_str(",\"total\":");
                work_json(output, statistics.total);
                output.push('}');
            } else {
                output.push_str("null");
            }
            output.push('}');
        }
        output.push_str("]}");
    }
}

#[cfg(test)]
#[path = "statistics_tests.rs"]
mod tests;
