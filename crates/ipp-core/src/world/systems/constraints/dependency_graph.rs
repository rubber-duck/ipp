//! Deterministic mixed-driver dependencies by exact resolved property overlap.

use super::DriverProperty;
use crate::EntityId;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct DriverKey(pub EntityId, pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct PropertyIdentity {
    pub(super) entity: EntityId,
    pub(super) incarnation: u64,
    pub(super) kind: crate::DynamicPropertyKind,
    pub(super) property: DriverProperty,
}

pub(super) struct DriverDependencies {
    pub(super) key: DriverKey,
    pub(super) target: PropertyIdentity,
    pub(super) sources: Vec<PropertyIdentity>,
}

/// Only strongly connected cycle members are suppressed. Downstream drivers
/// read their retained values. BTree order breaks writer and traversal ties.
pub(super) fn order(drivers: &[DriverDependencies]) -> (Vec<DriverKey>, BTreeSet<DriverKey>) {
    let mut writers: BTreeMap<PropertyIdentity, Vec<DriverKey>> = BTreeMap::new();
    for driver in drivers {
        writers.entry(driver.target).or_default().push(driver.key);
    }
    let mut dependencies: BTreeMap<DriverKey, BTreeSet<DriverKey>> = BTreeMap::new();
    for driver in drivers {
        let edges = dependencies.entry(driver.key).or_default();
        for property in &driver.sources {
            if let Some(sources) = writers.get(property) {
                edges.extend(sources);
            }
        }
    }
    // Iterative DFS finishing order, followed by reversed-graph SCC traversal.
    // Bounded by drivers/edges, without recursive stack growth on long chains.
    let mut visited = BTreeSet::new();
    let mut finish = Vec::new();
    for &start in dependencies.keys() {
        let mut stack = vec![(start, false)];
        while let Some((node, exiting)) = stack.pop() {
            if exiting {
                finish.push(node);
                continue;
            }
            if !visited.insert(node) {
                continue;
            }
            stack.push((node, true));
            stack.extend(dependencies[&node].iter().rev().map(|&next| (next, false)));
        }
    }
    let mut reverse: BTreeMap<DriverKey, Vec<DriverKey>> = BTreeMap::new();
    for (&node, edges) in &dependencies {
        for &next in edges {
            reverse.entry(next).or_default().push(node);
        }
    }
    visited.clear();
    let mut cyclic = BTreeSet::new();
    for &start in finish.iter().rev() {
        if !visited.insert(start) {
            continue;
        }
        let mut members = vec![start];
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            for &next in reverse.get(&node).into_iter().flatten() {
                if visited.insert(next) {
                    members.push(next);
                    stack.push(next);
                }
            }
        }
        if members.len() > 1 || dependencies[&start].contains(&start) {
            cyclic.extend(members);
        }
    }
    visited.clear();
    let mut order = Vec::new();
    for &start in dependencies.keys() {
        let mut stack = vec![(start, false)];
        while let Some((node, exiting)) = stack.pop() {
            if exiting {
                order.push(node);
                continue;
            }
            if cyclic.contains(&node) || !visited.insert(node) {
                continue;
            }
            stack.push((node, true));
            stack.extend(dependencies[&node].iter().rev().map(|&next| (next, false)));
        }
    }
    (order, cyclic)
}

#[cfg(test)]
#[path = "dependency_graph_tests.rs"]
mod tests;
