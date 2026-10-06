//! Schema-independent connection opening and contract retrieval, and bounded
//! owned codecs for the headless host contract.
//!
//! Each independently routed lane is a module: [`contract`] (hello, announcement
//! and contract export), [`world`] (World session envelopes), [`host`] (Host
//! connection control), [`dataset`], [`bulk_read`] and [`asset_source`].

mod codec;
mod limits;

pub mod asset_source;
pub mod bulk_read;
pub mod contract;
pub mod dataset;
pub mod host;
/// Owned, untrusted transport reference tokens and exact Host resolution.
pub mod references;
pub mod world;

pub use codec::ProtocolError;
pub use limits::{
    BATCH_OUTCOME_ALIASES, BATCH_OUTCOME_EFFECTS, COMMAND_PAGE_BYTES, COMMAND_PAGE_COMMANDS,
    INSPECTION_PAGE_RECORDS, MAX_ANIMATION_TARGET_INDICES, MAX_ENTITY_TREE_DEPTH,
    MAX_FAILURE_MESSAGE_BYTES, MAX_FIELD_BYTES, MAX_INSERT_FIELDS, MAX_INSPECTED_COMPONENTS,
    MAX_INSPECTED_FIELDS, MAX_LIFECYCLE_PUBLICATIONS, MAX_MESSAGE_BYTES, MAX_METADATA_CLASSES,
    MAX_PLAYBACK_EVENTS, MAX_RESOURCE_EVENT_RECORDS,
};

/// Runtime trait path used by target fixture derives.
pub use ipp_core::components::schema;
