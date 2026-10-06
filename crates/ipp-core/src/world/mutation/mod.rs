//! Ordered mutation: single operations, whole batches, the commit boundary,
//! reliable effect admission and the batch-local bookkeeping they share.

mod batch;
mod commit;
mod effect_admission;
mod entity_aliases;
mod mutation_map;
mod operation;

pub(in crate::world) use commit::commit_components;
pub(in crate::world) use entity_aliases::EntityAliases;
pub(in crate::world) use mutation_map::MutationMap;
