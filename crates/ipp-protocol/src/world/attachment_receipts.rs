//! Session-fenced attachment receipts, separate from persistent authored values.

use crate::codec::{ProtocolError, Writer};
use crate::contract::wire_manifest::{
    ATTACHMENT_DETACHED, ATTACHMENT_SUPERSEDED, ATTACHMENT_WRITTEN, OPERATION_ADOPTED,
};
use crate::references::WorldReference;

/// Maximum encoded size of one applied receipt record.
pub const MAX_ATTACHMENT_EFFECT_BYTES: usize = 70;

/// Encoded size of one adoption report: its operation index and effect tag.
pub const ADOPTED_EFFECT_BYTES: usize = 5;

/// Exact immutable identity of an applied attachment producer write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentReceipt {
    /// Opaque registry handle valid only in the issuing session.
    pub id: u64,
    /// Exact producer World lifetime.
    pub parent: WorldReference,
    /// Generational attachment entity.
    pub anchor: u64,
    /// Exact WorldAttachment component lifetime.
    pub incarnation: u64,
    /// Non-reused write revision, including same-value writes.
    pub revision: u64,
    /// Exact authored child, if present.
    pub child: Option<WorldReference>,
}

/// The applied producer or conditional cleanup result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachmentEffectKind {
    /// A new producer revision was written.
    Written,
    /// Conditional cleanup removed the exact producer revision.
    Detached,
    /// Conditional cleanup preserved a replacement producer.
    Superseded,
}

/// An applied effect retains its original operation position after partial failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentEffect {
    /// Original zero-based index in this command buffer.
    pub operation: u32,
    /// Applied action, independent of later batch failure.
    pub kind: AttachmentEffectKind,
    /// Session-owned retirement observation identity.
    pub receipt: AttachmentReceipt,
}

/// One applied per-operation effect of a batch reply, in core order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BatchOperationEffect {
    /// An attachment producer write or conditional cleanup, with its receipt.
    Attachment(AttachmentEffect),
    /// An adopting create or insert found its entity or component already present.
    Adopted {
        /// Original zero-based index in this command buffer.
        operation: u32,
    },
}

/// Core outcomes with every applied effect mapped to its wire form, attachment
/// effects to a retained receipt.
#[derive(Clone, Debug, PartialEq)]
pub struct ReceiptBatchOutcome {
    /// Original aliases, error and operation positions.
    pub outcome: ipp_core::BatchOutcome,
    /// Complete mapped effects, in core order.
    pub effects: Vec<BatchOperationEffect>,
}

impl Writer {
    /// Encode every applied effect of a batch reply in core order, attachment effects
    /// with their receipts.
    pub(super) fn batch_operation_effects(
        &mut self,
        outcome: &ReceiptBatchOutcome,
    ) -> Result<(), ProtocolError> {
        if outcome.effects.len() != outcome.outcome.effects.len() {
            return Err(ProtocolError::Malformed("unmapped attachment effects"));
        }
        self.count(outcome.effects.len(), crate::BATCH_OUTCOME_EFFECTS)?;
        for effect in &outcome.effects {
            let effect = match effect {
                BatchOperationEffect::Attachment(effect) => effect,
                BatchOperationEffect::Adopted {
                    operation,
                } => {
                    self.u32(*operation)?;
                    self.u8(OPERATION_ADOPTED)?;
                    continue;
                }
            };
            self.u32(effect.operation)?;
            self.u8(match effect.kind {
                AttachmentEffectKind::Written => ATTACHMENT_WRITTEN,
                AttachmentEffectKind::Detached => ATTACHMENT_DETACHED,
                AttachmentEffectKind::Superseded => ATTACHMENT_SUPERSEDED,
            })?;
            let receipt = &effect.receipt;
            self.u64(receipt.id)?;
            self.world_reference(receipt.parent)?;
            self.u64(receipt.anchor)?;
            self.u64(receipt.incarnation)?;
            self.u64(receipt.revision)?;
            self.u8(u8::from(receipt.child.is_some()))?;
            if let Some(child) = receipt.child {
                self.world_reference(child)?;
            }
        }
        Ok(())
    }
}
