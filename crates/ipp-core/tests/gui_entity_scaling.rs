//! Release timings and ordinary-layout work counts for flat and deep GUI entity trees
//! of 1k, 4k and 16k entities: the first layout, a paint-only refresh, a committed
//! control value, and removing half the tree or the whole scope through core
//! lifecycle. Work counts are asserted exactly, so the steps stay linear in the
//! entities they must visit; timings and their 1k-to-16k growth are printed for a
//! quiet machine (`cargo test -p ipp-core --release --test gui_entity_scaling --
//! --nocapture`).

mod support;

use ipp_core::components::{GuiCheckbox, GuiLayout};
use ipp_core::systems::canvas::{CanvasBox, CanvasStyle};
use ipp_core::systems::gui::layout::{GuiEntityLayoutWork, MAX_LAYOUT_DEPTH};
use ipp_core::systems::gui::local::GuiLocalAction;
use ipp_core::*;
use std::collections::BTreeMap;
use std::mem::offset_of;
use std::time::{Duration, Instant};
use support::gui_panel::*;

const SIZES: [usize; 3] = [1_000, 4_000, 16_000];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Shape {
    /// Checkbox leaves in one column, half of them under a nested column.
    Flat,
    /// One chain of columns, which layout follows to `MAX_LAYOUT_DEPTH`.
    Deep,
}

fn column() -> GuiLayout {
    GuiLayout {
        kind: 2,
        ..Default::default()
    }
}

/// Elapsed time per shape and operation, in `SIZES` order.
type Timings = BTreeMap<(String, &'static str), Vec<Duration>>;

fn measure(
    timings: &mut Timings,
    size: usize,
    shape: Shape,
    operation: &'static str,
    run: impl FnOnce(),
) {
    let start = Instant::now();
    run();
    let elapsed = start.elapsed();
    println!(
        "gui-scaling entities={size} shape={shape:?} operation={operation} elapsed_us={}",
        elapsed.as_micros()
    );
    timings
        .entry((format!("{shape:?}"), operation))
        .or_default()
        .push(elapsed);
}

/// Canvas root, the nested column that holds about half of the entities, and
/// the queued batch that creates every entity. The first measured frame
/// applies that batch whole and lays the tree out.
fn build(shape: Shape, size: usize) -> (GuiPanel, EntityId) {
    let mut panel = GuiPanel::new(column());
    let root = panel.root_entity;
    let half = panel.node(root, column());
    let mut operations = Vec::with_capacity(size * 5);
    for index in 0..size {
        let alias = index as u32 + 1;
        let entity = EntityRef::Alias(alias);
        operations.push(Command::Create {
            alias,
            metadata: Default::default(),
            adopt: false,
        });
        let parent = match shape {
            Shape::Flat if index >= size / 2 => EntityRef::Handle(half),
            Shape::Flat => EntityRef::Handle(root),
            Shape::Deep if index == 0 => EntityRef::Handle(root),
            Shape::Deep => EntityRef::Alias(alias - 1),
        };
        let values = match shape {
            Shape::Flat => vec![
                ComponentValue::GuiCheckbox(GuiCheckbox::default()),
                ComponentValue::GuiLayout(GuiLayout {
                    width: 10.0,
                    height: 1.0,
                    ..Default::default()
                }),
            ],
            Shape::Deep => vec![
                ComponentValue::GuiLayout(column()),
                ComponentValue::CanvasBox(CanvasBox::default()),
                ComponentValue::CanvasStyle(CanvasStyle::default()),
            ],
        };
        operations.extend(
            values
                .into_iter()
                .map(|value| Command::insert_value(entity.clone(), value)),
        );
        operations.push(Command::PlaceEntity {
            entity,
            placement: EntityPlacementRef {
                parent: Some(parent),
                before: None,
            },
        });
    }
    panel.queue(operations);
    (panel, half)
}

fn work(panel: &mut GuiPanel) -> GuiEntityLayoutWork {
    panel.work()
}

#[test]
fn maintained_scaling_ordinary_gui_layout_paint_commit_and_removal() {
    let mut timings = Timings::new();
    for shape in [Shape::Flat, Shape::Deep] {
        for size in SIZES {
            let (mut panel, half) = build(shape, size);

            measure(&mut timings, size, shape, "first-layout", || panel.frame());
            let entities: Vec<EntityId> = panel
                .take_outcome()
                .result
                .unwrap()
                .into_iter()
                .map(|(_, id)| id)
                .collect();
            let first = work(&mut panel);
            assert_eq!(first.reflows, 1);
            match shape {
                // Root, nested column and every leaf, each once.
                Shape::Flat => assert_eq!(first.visited_entities, size as u64 + 2),
                // Deeper entities are cut once at the supported depth.
                Shape::Deep => assert!(
                    first.visited_entities <= MAX_LAYOUT_DEPTH as u64 + 3,
                    "{first:?}"
                ),
            }
            let published = panel.output();
            if shape == Shape::Flat {
                assert_eq!(published.hits.len(), size);
            }

            match shape {
                // A committed control value repaints without layout work,
                // whatever the number of controls.
                Shape::Flat => {
                    let target = *entities.last().unwrap();
                    panel.act(target, GuiLocalAction::Toggle);
                    measure(&mut timings, size, shape, "value-commit", || panel.frame());
                    assert_eq!(panel.value(target), ControlValue::Bool(true));
                }
                // A paint-only edit repaints without layout work.
                Shape::Deep => {
                    panel.queue_set(
                        entities[0],
                        ComponentValue::CANVAS_STYLE,
                        offset_of!(CanvasStyle, red),
                        FieldValue::F32(0.5),
                    );
                    measure(&mut timings, size, shape, "paint-refresh", || panel.frame());
                    panel.take_outcome().result.unwrap();
                }
            }
            assert_eq!(work(&mut panel), GuiEntityLayoutWork::default());
            assert!(panel.output().paint_revision > published.paint_revision);

            // Removing about half the tree through core lifecycle re-evaluates
            // only what remains.
            let removed = match shape {
                Shape::Flat => half,
                Shape::Deep => entities[size / 2],
            };
            panel.queue(vec![Command::DeleteSubtree {
                root: EntityRef::Handle(removed),
            }]);
            measure(&mut timings, size, shape, "half-removal", || panel.frame());
            panel.take_outcome().result.unwrap();
            let after = work(&mut panel);
            assert_eq!(after.reflows, 1);
            match shape {
                Shape::Flat => {
                    assert_eq!(after.visited_entities, (size - size / 2) as u64 + 1);
                    assert_eq!(panel.output().hits.len(), size / 2);
                }
                Shape::Deep => assert!(
                    after.visited_entities <= MAX_LAYOUT_DEPTH as u64 + 3,
                    "{after:?}"
                ),
            }

            // Removing the whole tree leaves the World's canvas empty.
            let root = panel.root_entity;
            panel.queue(vec![Command::DeleteSubtree {
                root: EntityRef::Handle(root),
            }]);
            measure(&mut timings, size, shape, "scope-removal", || panel.frame());
            panel.take_outcome().result.unwrap();
            assert!(panel.output().entries.is_empty());
            assert!(
                panel
                    .host
                    .world_mut(panel.world)
                    .unwrap()
                    .entities()
                    .is_empty()
            );
        }
    }

    // Timings are evidence for a quiet machine; regression runs share the host,
    // so only the work counts above are asserted.
    for ((shape, operation), times) in &timings {
        println!(
            "gui-scaling shape={shape} operation={operation} growth_1k_to_16k={:.1}x",
            times[2].as_secs_f64() / times[0].as_secs_f64().max(f64::MIN_POSITIVE)
        );
    }
}
