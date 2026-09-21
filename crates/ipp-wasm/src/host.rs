//! Host composition for this platform.

use crate::services::WasmHostServices;

pub(crate) type WasmHost = ipp_host_session::Host<WasmHostServices>;
