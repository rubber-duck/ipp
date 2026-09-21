//! Applied lifecycle observations, session subscriptions and bounded owned publication.

use std::{
    any::Any,
    collections::{BTreeMap, VecDeque},
};

use super::{
    System, SystemCommandContext, SystemFactory, SystemId, SystemInitContext, SystemInitError,
    SystemLifecycleContext, SystemUpdateContext,
};
use crate::{EntityId, ErrorReason};

/// Committed entity transitions; evaluation never produces these observations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityLifecycleKind {
    /// A fresh generational identity became live.
    Created,
    /// Authored entity metadata changed.
    MetadataChanged,
    /// The identity became dead after component cleanup.
    Deleted,
}

/// Committed effective component transitions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComponentLifecycleKind {
    /// Previously absent effective storage became occupied.
    Inserted,
    /// A value changed within its existing incarnation.
    Updated,
    /// A different incarnation replaced occupied storage.
    Replaced,
    /// Occupied storage was removed.
    Removed,
}

/// Owned identity data describing an applied effect, independent of wire delivery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LifecycleObservation {
    /// An entity identity or its authored metadata changed.
    Entity {
        /// World-local generational identity.
        entity: EntityId,
        /// Applied transition.
        kind: EntityLifecycleKind,
    },
    /// An effective component changed after synchronous invalidation completed.
    Component {
        /// Owning World-local identity.
        entity: EntityId,
        /// Compiled component type.
        component: u16,
        /// Applied transition.
        kind: ComponentLifecycleKind,
        /// Previous effective storage identity, absent for insertion.
        previous_incarnation: Option<u64>,
        /// New effective storage identity, absent for removal.
        incarnation: Option<u64>,
    },
    /// A relevant Host resource changed availability; its identity survives unloading.
    Asset {
        /// Stable identity and availability at this applied boundary.
        resource: crate::AssetResourceSnapshot,
        /// Identity removal is distinct from residency changes.
        kind: crate::services::asset_management::AssetLifecycleKind,
    },
}

/// Domain selection with optional exact identity filters; absent identities match all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleFilter {
    /// Include entity transitions.
    pub entities: bool,
    /// Include component transitions.
    pub components: bool,
    /// Include resource transitions relevant to this World.
    pub assets: bool,
    /// Restrict entity/component observations to one generational identity.
    pub entity: Option<EntityId>,
    /// Restrict component observations to one compiled component type.
    pub component: Option<u16>,
    /// Restrict resource observations to one stable Host resource identity.
    pub asset: Option<u64>,
}

impl Default for LifecycleFilter {
    fn default() -> Self {
        Self {
            entities: true,
            components: true,
            assets: true,
            entity: None,
            component: None,
            asset: None,
        }
    }
}

impl LifecycleFilter {
    fn validate(&self) -> Result<(), ErrorReason> {
        let selected = self.entities || self.components;
        let selected = selected || self.assets;
        if !selected || self.component == Some(0) || self.entity.is_some_and(|id| id.to_bits() == 0)
        {
            return Err(ErrorReason::InvalidValue);
        }
        if self.asset == Some(0) {
            return Err(ErrorReason::InvalidValue);
        }
        Ok(())
    }

    fn matches(&self, observation: &LifecycleObservation) -> bool {
        match observation {
            LifecycleObservation::Entity {
                entity,
                ..
            } => self.entities && self.entity.is_none_or(|id| id == *entity),
            LifecycleObservation::Component {
                entity,
                component,
                ..
            } => {
                self.components
                    && self.entity.is_none_or(|id| id == *entity)
                    && self.component.is_none_or(|id| id == *component)
            }
            LifecycleObservation::Asset {
                resource,
                ..
            } => self.assets && self.asset.is_none_or(|id| id == resource.id),
        }
    }
}

/// Correlated subscription control interpreted exclusively by this system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LifecyclePublisherCommand {
    /// Start observing subsequent effects at this ordered boundary.
    Subscribe {
        /// Nonzero client-chosen identity within this session.
        subscription: u64,
        /// Bounded matching criteria.
        filter: LifecycleFilter,
    },
    /// Release matching queued observations and future publication.
    Unsubscribe {
        /// Previously selected identity within this session.
        subscription: u64,
    },
}

/// Owned publication selected for one subscription.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecyclePublication {
    /// Client-chosen session-local identity.
    pub subscription: u64,
    /// Monotonic World observation sequence; filtering may create gaps.
    pub sequence: u64,
    /// Core mutation/evaluation boundary where the effect was observed.
    pub tick: u64,
    /// Applied identity transition.
    pub observation: LifecycleObservation,
}

/// Transport drains these owned values without retaining runtime storage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LifecyclePublisherOutput {
    /// Ordered applied effects for this session.
    Events(Vec<LifecyclePublication>),
    /// All subscriptions in this session ended because its queue filled.
    Overflow {
        /// Number of observations discarded, including the triggering observation.
        dropped: u64,
    },
}

mod system;
pub use system::{LifecyclePublisherSystem, LifecyclePublisherSystemFactory};
