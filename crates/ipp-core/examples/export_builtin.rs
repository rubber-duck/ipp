//! Export authoritative built-in encoded fixtures without a second generator.

use std::io::{self, Write};

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let kind = args
        .next()
        .ok_or("usage: export_builtin mesh|texture|skeleton|pose|skin URI")?;
    let uri = args
        .next()
        .ok_or("usage: export_builtin mesh|texture|skeleton|pose|skin URI")?;
    if args.next().is_some() {
        return Err("usage: export_builtin mesh|texture|skeleton|pose|skin URI".into());
    }

    let bytes = match kind.as_str() {
        "mesh" => ipp_core::services::asset_management::builtin::mesh(&uri),
        #[cfg(feature = "skeletal-animation")]
        "skeleton" => {
            ipp_core::services::asset_management::builtin::rig(ipp_core::SKELETON_TYPE, &uri)
        }
        #[cfg(feature = "skeletal-animation")]
        "pose" => ipp_core::services::asset_management::builtin::rig(ipp_core::POSE_TYPE, &uri),
        #[cfg(feature = "skeletal-animation")]
        "skin" => ipp_core::services::asset_management::builtin::rig(ipp_core::SKIN_TYPE, &uri),
        "texture" => ipp_core::services::asset_management::builtin::texture(&uri),
        _ => return Err("unsupported built-in asset kind".into()),
    }
    .map_err(|error| format!("invalid {kind} recipe: {error:?}"))?;
    io::stdout()
        .lock()
        .write_all(&bytes)
        .map_err(|error| error.to_string())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
