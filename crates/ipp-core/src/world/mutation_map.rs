//! A commit usually touches one component. Keep that entry inline; larger commits
//! retain the existing ordered B-tree behavior and deterministic callback order.

use std::{collections::BTreeMap, ops::Index};

pub(super) struct MutationMap<K, V> {
    single: Option<(K, V)>,
    multiple: BTreeMap<K, V>,
}

impl<K, V> Default for MutationMap<K, V> {
    fn default() -> Self {
        Self {
            single: None,
            multiple: BTreeMap::new(),
        }
    }
}

impl<K: Ord, V> MutationMap<K, V> {
    pub fn get(&self, key: &K) -> Option<&V> {
        if let Some((k, v)) = &self.single {
            (k == key).then_some(v)
        } else {
            self.multiple.get(key)
        }
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.get(key).is_some()
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        if let Some((k, v)) = &mut self.single {
            if *k == key {
                return Some(std::mem::replace(v, value));
            }
            let (k, v) = self.single.take().unwrap();
            self.multiple.insert(k, v);
        }
        if self.multiple.is_empty() && crate::allocation_optimizations_enabled() {
            self.single = Some((key, value));
            None
        } else {
            self.multiple.insert(key, value)
        }
    }

    pub fn insert_if_absent(&mut self, key: K, value: V) {
        if !self.contains_key(&key) {
            self.insert(key, value);
        }
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        if self.single.as_ref().is_some_and(|(k, _)| k == key) {
            self.single.take().map(|(_, v)| v)
        } else {
            self.multiple.remove(key)
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.single
            .iter()
            .map(|(k, v)| (k, v))
            .chain(self.multiple.iter())
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.iter().map(|(k, _)| k)
    }

    pub fn clear(&mut self) {
        self.single = None;
        self.multiple.clear();
    }
}

impl<K: Ord, V> Index<&K> for MutationMap<K, V> {
    type Output = V;

    fn index(&self, key: &K) -> &V {
        self.get(key).expect("present mutation key")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_spill_removal_and_reuse_match_ordered_map() {
        let mut map = MutationMap::default();
        let mut expected = BTreeMap::new();
        for (key, value) in [(8, 1), (8, 2), (3, 3), (12, 4), (3, 5)] {
            assert_eq!(map.insert(key, value), expected.insert(key, value));
            assert_eq!(
                map.iter().collect::<Vec<_>>(),
                expected.iter().collect::<Vec<_>>()
            );
        }
        map.insert_if_absent(8, 90);
        assert_eq!(map[&8], 2);
        for key in [8, 99, 3, 12] {
            assert_eq!(map.remove(&key), expected.remove(&key));
        }
        map.insert(2, 20);
        assert_eq!(map.keys().copied().collect::<Vec<_>>(), [2]);
        map.clear();
        assert!(map.iter().next().is_none());
        map.insert_if_absent(1, 7);
        assert_eq!(map[&1], 7);
    }
}
