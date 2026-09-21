//! Typed encoding belongs with assets; byte output belongs to generic data sources.

/// Typed CPU representation encoded into its ordinary IPP asset format.
pub trait AssetEncoder {
    /// Encode without allocating more output than the supplied destination budget.
    fn encode_asset(&self, max_bytes: usize) -> Result<Vec<u8>, String>;
}
