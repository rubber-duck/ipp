//! Recoverable presentation failures are separate from World and Host lifetime.

/// A presentation adapter reports the scope without faulting the Host.
#[derive(Clone, Debug)]
pub struct HostPresentationFailure {
    /// Draw, resource or context boundary that failed.
    pub scope: ipp_protocol::RuntimeFailureScope,
    /// Diagnostic retained separately from semantic mutation outcomes.
    pub message: String,
}
