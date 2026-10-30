//! Ordered completed envelopes with reusable capacity. IDs increase monotonically.

use std::collections::VecDeque;

use super::publication::{RetainedPublication, WorldPublicationId};

#[derive(Default)]
pub(crate) struct PublicationEntries {
    entries: VecDeque<(WorldPublicationId, RetainedPublication)>,
}

impl PublicationEntries {
    pub fn get(&self, id: &WorldPublicationId) -> Option<&RetainedPublication> {
        self.entries
            .binary_search_by_key(id, |(id, _)| *id)
            .ok()
            .map(|index| &self.entries[index].1)
    }

    pub fn get_mut(&mut self, id: &WorldPublicationId) -> Option<&mut RetainedPublication> {
        self.entries
            .binary_search_by_key(id, |(id, _)| *id)
            .ok()
            .map(|index| &mut self.entries[index].1)
    }

    pub fn insert(&mut self, id: WorldPublicationId, entry: RetainedPublication) {
        debug_assert!(
            self.entries
                .back()
                .is_none_or(|(previous, _)| *previous < id)
        );
        self.entries.push_back((id, entry));
    }

    pub fn remove(&mut self, id: &WorldPublicationId) -> Option<RetainedPublication> {
        self.entries
            .binary_search_by_key(id, |(id, _)| *id)
            .ok()
            .map(|index| self.entries.remove(index).unwrap().1)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&WorldPublicationId, &RetainedPublication)> {
        self.entries.iter().map(|(id, entry)| (id, entry))
    }

    pub fn values(&self) -> impl Iterator<Item = &RetainedPublication> {
        self.entries.iter().map(|(_, entry)| entry)
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut RetainedPublication> {
        self.entries.iter_mut().map(|(_, entry)| entry)
    }
}
