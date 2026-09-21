//! State owned directly by one world system instance.

/// Queued uploads, upload outcomes and the world's retained resource dependencies.
#[derive(Default)]
pub struct AssetDependencySystemState {
    pub(super) evaluation_meshes: std::collections::BTreeMap<
        crate::services::asset_management::service::AssetDemandSelection,
        bool,
    >,
    pub(super) evaluation_meshes_initialized: bool,
    pub(super) animation_demand_revision: Option<u64>,
    pub(super) changed_sources: std::collections::BTreeSet<
        crate::services::asset_management::service::AssetDemandSelection,
    >,
    pub(super) animation_sources: std::collections::BTreeSet<
        crate::services::asset_management::service::AssetDemandSelection,
    >,
    pub(in crate::world) prepared_changes: Vec<crate::AssetResourceSnapshot>,
    pub(in crate::world) component_sources: std::collections::BTreeMap<
        (crate::EntityId, u16),
        std::collections::BTreeSet<
            crate::services::asset_management::service::AssetDemandSelection,
        >,
    >,
    pub(in crate::world) source_users: std::collections::BTreeMap<
        crate::services::asset_management::service::AssetDemandSelection,
        usize,
    >,
    pub(in crate::world) asset_queue:
        std::collections::VecDeque<crate::services::asset_management::AssetUpload>,
    pub(in crate::world) asset_pending: Vec<PendingAssetUpload>,
}

/// Local producer correlation and its shared Host resource identity.
pub(in crate::world) struct PendingAssetUpload {
    pub(in crate::world) request_id: u64,
    pub(in crate::world) producer_key: crate::services::asset_management::AssetUploadIdentity,
    pub(in crate::world) resource_key: Option<crate::services::asset_management::AssetKey>,
    pub(in crate::world) error: Option<String>,
}
