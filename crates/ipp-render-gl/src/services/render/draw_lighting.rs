//! Retained per-entity lighting blocks indexed directly by generational entity slots.

use super::light_selection::PreparedDrawLighting;
use super::scene::RenderEntity as EntityId;
use std::collections::BTreeMap;

struct DrawLightingRow {
    entity: EntityId,
    epoch: u64,
    draw: PreparedDrawLighting,
}

#[derive(Default)]
pub(super) struct DrawLightingTable {
    rows: BTreeMap<EntityId, DrawLightingRow>,
    epoch: u64,
}

impl DrawLightingTable {
    pub fn begin(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.rows
            .retain(|_, row| row.epoch.wrapping_add(1) == self.epoch);
    }

    pub fn get(&self, entity: &EntityId) -> Option<&PreparedDrawLighting> {
        let row = self.rows.get(entity)?;
        (row.entity == *entity && row.epoch == self.epoch).then_some(&row.draw)
    }

    pub fn get_or_insert(
        &mut self,
        entity: EntityId,
        create: impl FnOnce() -> PreparedDrawLighting,
    ) -> &mut PreparedDrawLighting {
        let row = self.rows.entry(entity).or_insert_with(|| DrawLightingRow {
            entity,
            epoch: self.epoch,
            draw: create(),
        });
        if row.epoch != self.epoch && row.epoch.wrapping_add(1) != self.epoch {
            row.draw.selected_count = 0;
        }
        row.epoch = self.epoch;
        &mut row.draw
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut PreparedDrawLighting> {
        let epoch = self.epoch;
        self.rows
            .values_mut()
            .filter(move |row| row.epoch == epoch)
            .map(|row| &mut row.draw)
    }
}
