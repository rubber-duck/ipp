//! Provider completion policy shared by platform adapters.

/// Deliver owned provider input without turning resource pressure into session failure.
///
/// The core can reject a complete buffer before accepting ownership into its input
/// queue. This adapter has already consumed the provider result, so a rejection
/// ends that resource's reader with an ordinary failure. Its declaration and any
/// committed batch outcomes remain intact; the next resource poll reports Failed.
pub fn deliver_resource(
    world: &mut ipp_core::HostRuntime,
    id: u64,
    result: Result<Vec<u8>, String>,
) {
    if let Err(error) = world.complete_resource(id, result) {
        world.asset_input_end(id, Err(format!("Asset provider input rejected: {error}")));
    }
}

/// Generate a compiled built-in source through the ordinary provider contract.
#[cfg(feature = "builtin-assets")]
pub fn builtin_resource(request: &ipp_core::AssetAcquisitionRequest) -> Result<Vec<u8>, String> {
    let result = match request.kind {
        ipp_core::AssetResourceKind::Mesh => {
            ipp_core::services::asset_management::builtin::mesh(&request.source)
        }
        ipp_core::AssetResourceKind::Texture => {
            ipp_core::services::asset_management::builtin::texture(&request.source)
        }
        #[cfg(feature = "skeletal-animation")]
        kind if kind == ipp_core::SKELETON_TYPE || kind == ipp_core::POSE_TYPE => {
            ipp_core::services::asset_management::builtin::rig(kind, &request.source)
        }
        #[cfg(feature = "skeletal-animation")]
        ipp_core::SKIN_TYPE => {
            ipp_core::services::asset_management::builtin::rig(request.kind, &request.source)
        }
        _ => return Err("Built-in resource type unavailable".into()),
    };
    result.map_err(|error| error.to_string())
}
