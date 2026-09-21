//! Component-state behavior for [`WorldEntityState`](super::WorldEntityState)
//! and [`WorldMutationState`](super::WorldMutationState), split by phase.
//!
//! Ownership and orchestration stay in [`super`]. Entity identity, metadata
//! and index operations live in [`super::entity_state`]. The private
//! [`super::component_binding`] and [`super::component_query`] helpers keep
//! their phase and invalidation contracts unchanged.
//!
//! - [`access`]: read access to staged and retained values.
//! - [`staging`]: input hydration, dirty tracking and preparation.
//! - [`mutation`]: per-command component application.
//! - [`observations`]: lifecycle effect and observation recording.

pub(super) mod access;
pub(super) mod mutation;
pub(super) mod observations;
pub(super) mod staging;
