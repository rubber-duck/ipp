//! Applied operation effects and their reliable admission.

use super::ErrorReason;

/// One operation's concrete applied effect, issued by World orchestration or by
/// the System that owns it; the batch outcome reports it at its operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OperationEffect {
    /// Exact attachment producer identity and conditional-cleanup outcome.
    WorldAttachment(crate::WorldAttachmentEffect),
    /// An adopting operation found its target already present: an adopting
    /// [`Command::Create`](crate::Command::Create) bound an existing entity, or
    /// an adopting [`Command::InsertComponent`](crate::Command::InsertComponent)
    /// wrote an existing component in place. An adopting operation that created
    /// or inserted reports no effect.
    Adopted,
}

/// Delivery capacity required before one operation can mutate authoring state.
#[derive(Clone, Debug, Default)]
pub struct OperationEffectDemand {
    /// Maximum newly issued attachment tokens, separate from already retained identities.
    pub fresh_attachment_tokens: usize,
    /// Exact existing tokens that can be emitted, for registry-level deduplication.
    pub existing_attachment_tokens: Vec<crate::WorldAttachmentToken>,
    /// Maximum attachment effect records, including repeated observations of an existing token.
    pub max_records: usize,
    /// The operation adopts: it may report one [`OperationEffect::Adopted`] beyond
    /// `max_records`. Adoption reports carry no receipt, so a sink can admit them
    /// with their batch instead of per operation.
    pub adopting: bool,
}

/// Operation-scoped reliable effect admission; native safety cleanup does not use this sink.
pub trait OperationEffectSink {
    /// Reserve registry and delivery capacity before any System mutation callback.
    fn reserve(&mut self, demand: &OperationEffectDemand) -> Result<(), ErrorReason>;

    /// Retain one committed effect infallibly within the successful reservation.
    fn emit(&mut self, effect: OperationEffect);

    /// Release unused reservation on every operation exit, including failed reservation.
    fn settle(&mut self);

    /// Resolve an untrusted receipt in the current submitting session at execution time.
    fn attachment_receipt(
        &self,
        _receipt: u64,
    ) -> Result<crate::WorldAttachmentToken, ErrorReason> {
        Err(ErrorReason::InvalidEntity)
    }
}

/// An effect at its original operation position within the acknowledged command buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedOperationEffect {
    /// Zero-based operation index, including retained effects of a failed operation.
    pub operation: usize,
    /// Owned concrete effect issued by its subsystem.
    pub effect: OperationEffect,
}
