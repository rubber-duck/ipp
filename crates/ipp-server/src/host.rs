//! Host composition for this platform.

use crate::services::NativeHostServices;

pub(crate) type NativeHost = ipp_host_session::Host<NativeHostServices>;
