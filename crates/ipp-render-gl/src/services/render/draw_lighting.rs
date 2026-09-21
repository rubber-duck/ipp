//! Retained per-entity lighting blocks indexed directly by generational entity slots.

use super::light_selection::PreparedDrawLighting;
use ipp_core::EntityId;

struct DrawLightingRow {
    entity: EntityId,
    epoch: u64,
    draw: PreparedDrawLighting,
}

#[derive(Default)]
pub(super) struct DrawLightingTable {
    rows: Vec<Option<DrawLightingRow>>,
    epoch: u64,
}

impl DrawLightingTable {
    pub fn begin(&mut self, slots: usize) {
        self.epoch = self.epoch.wrapping_add(1);
        if self.rows.len() < slots {
            self.rows.resize_with(slots, || None);
        }
    }

    pub fn get(&self, entity: &EntityId) -> Option<&PreparedDrawLighting> {
        let row = self.rows.get(entity.index() as usize)?.as_ref()?;
        (row.entity == *entity && row.epoch == self.epoch).then_some(&row.draw)
    }

    pub fn get_or_insert(
        &mut self,
        entity: EntityId,
        create: impl FnOnce() -> PreparedDrawLighting,
    ) -> &mut PreparedDrawLighting {
        let slot = entity.index() as usize;
        if self.rows.len() <= slot {
            self.rows.resize_with(slot + 1, || None);
        }
        let row = &mut self.rows[slot];
        if row.as_ref().is_none_or(|row| row.entity != entity) {
            *row = Some(DrawLightingRow {
                entity,
                epoch: self.epoch,
                draw: create(),
            });
        }
        let row = row.as_mut().expect("initialized row");
        if row.epoch != self.epoch && row.epoch.wrapping_add(1) != self.epoch {
            row.draw.selected_count = 0;
        }
        row.epoch = self.epoch;
        &mut row.draw
    }

    pub fn insert(&mut self, entity: EntityId, draw: PreparedDrawLighting) {
        let slot = entity.index() as usize;
        if self.rows.len() <= slot {
            self.rows.resize_with(slot + 1, || None);
        }
        self.rows[slot] = Some(DrawLightingRow {
            entity,
            epoch: self.epoch,
            draw,
        });
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut PreparedDrawLighting> {
        let epoch = self.epoch;
        self.rows.iter_mut().filter_map(move |row| {
            row.as_mut()
                .filter(|row| row.epoch == epoch)
                .map(|row| &mut row.draw)
        })
    }
}
