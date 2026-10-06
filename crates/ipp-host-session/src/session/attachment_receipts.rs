//! Bounded session identities and pre-mutation reliable reply admission.

use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use crate::reliable_output::SharedReplyReservation;
use ipp_core::{
    ErrorReason, OperationEffect, OperationEffectDemand, OperationEffectSink,
    WorldAttachmentEffect, WorldAttachmentToken,
};
use ipp_protocol::world::attachment_receipts::{
    ADOPTED_EFFECT_BYTES, AttachmentEffect, AttachmentEffectKind, AttachmentReceipt,
    BatchOperationEffect, MAX_ATTACHMENT_EFFECT_BYTES, ReceiptBatchOutcome,
};

const RECEIPT_LIMIT: usize = 4096;

/// Encoded alias report: the batch alias and the handle it named.
pub(crate) const ALIAS_WIRE_BYTES: usize = 12;

const ALIAS_RETAINED_BYTES: usize = 4 * std::mem::size_of::<(u32, ipp_core::EntityId)>();

/// Encoded symbol report beside its text: a length prefix and the handle.
const SYMBOL_WIRE_BYTES: usize = 12;

const SYMBOL_RETAINED_BYTES: usize =
    std::mem::size_of::<(std::sync::Arc<str>, ipp_core::EntityId)>();

// Adoption reports are admitted with their batch at their encoded
// `ADOPTED_EFFECT_BYTES` (5) and no retained charge: each is held only from its
// operation until the reply is encoded in the same frame, in no more memory than
// the decoded command it reports on, whose count the connection's buffered-batch
// budget bounds. So one message bounds an adopting batch beside its aliases and
// symbol reports: a reconnect adopting 32,768 entities (the alias limit) with two
// components each reports 884,736 bytes, leaving room for 32,716 more adoptions.
const _: () = assert!(
    std::mem::size_of::<ipp_core::AppliedOperationEffect>()
        + std::mem::size_of::<BatchOperationEffect>()
        <= std::mem::size_of::<ipp_core::Command>()
);

#[cfg(test)]
#[path = "attachment_receipts_tests.rs"]
mod tests;

pub(crate) type SharedReceipts = Rc<RefCell<ReceiptRegistry>>;

/// Whether a command defines an alias its batch outcome reports.
pub(crate) fn defines_alias(command: &ipp_core::Command) -> bool {
    matches!(command, ipp_core::Command::Create { .. })
}

/// Whether a command can report an adoption in its batch outcome.
pub(crate) fn may_adopt(command: &ipp_core::Command) -> bool {
    matches!(
        command,
        ipp_core::Command::Create {
            adopt: true,
            ..
        } | ipp_core::Command::InsertComponent {
            adopt: true,
            ..
        }
    )
}

pub(crate) struct ReceiptRegistry {
    live: bool,
    limit: usize,
    tokens: BTreeMap<u64, WorldAttachmentToken>,
    identities: BTreeMap<(u64, u64), u64>,
}

impl Default for ReceiptRegistry {
    fn default() -> Self {
        Self {
            live: true,
            limit: RECEIPT_LIMIT,
            tokens: BTreeMap::new(),
            identities: BTreeMap::new(),
        }
    }
}

impl ReceiptRegistry {
    pub(crate) fn resolve(&self, receipt: u64) -> Result<WorldAttachmentToken, ErrorReason> {
        if !self.live {
            return Err(ErrorReason::InvalidEntity);
        }
        self.tokens
            .get(&receipt)
            .cloned()
            .ok_or(ErrorReason::InvalidEntity)
    }

    pub(crate) fn release(&mut self, receipt: u64) -> Result<(), ErrorReason> {
        let token = self.resolve(receipt)?;
        self.tokens.remove(&receipt);
        self.identities.remove(&token.identity());
        Ok(())
    }

    pub(crate) fn close(&mut self) {
        self.live = false;
        self.tokens.clear();
        self.identities.clear();
    }

    pub(crate) fn outcome(
        &self,
        outcome: ipp_core::BatchOutcome,
    ) -> Result<ReceiptBatchOutcome, String> {
        let effects = outcome
            .effects
            .iter()
            .map(|effect| {
                let operation = u32::try_from(effect.operation)
                    .map_err(|_| "Operation index exceeds wire framing")?;
                let (kind, token) = match &effect.effect {
                    OperationEffect::WorldAttachment(WorldAttachmentEffect::Written(token)) => {
                        (AttachmentEffectKind::Written, token)
                    }
                    OperationEffect::WorldAttachment(WorldAttachmentEffect::Detached(token)) => {
                        (AttachmentEffectKind::Detached, token)
                    }
                    OperationEffect::WorldAttachment(WorldAttachmentEffect::Superseded(token)) => {
                        (AttachmentEffectKind::Superseded, token)
                    }
                    OperationEffect::Adopted => {
                        return Ok(BatchOperationEffect::Adopted {
                            operation,
                        });
                    }
                };
                let id = *self
                    .identities
                    .get(&token.identity())
                    .ok_or("Committed receipt has no reserved identity")?;
                Ok(BatchOperationEffect::Attachment(AttachmentEffect {
                    operation,
                    kind,
                    receipt: AttachmentReceipt {
                        id,
                        parent: token.parent().into(),
                        anchor: token.anchor().to_bits(),
                        incarnation: token.incarnation(),
                        revision: token.identity().1,
                        child: token.child().map(Into::into),
                    },
                }))
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(ReceiptBatchOutcome {
            outcome,
            effects,
        })
    }
}

pub(crate) struct ReceiptSink {
    registry: SharedReceipts,
    reply: SharedReplyReservation,
    report_bytes: usize,
    report_retained: usize,
    slots: Vec<u64>,
    remaining_records: usize,
}

impl ReceiptSink {
    pub(crate) fn new(
        registry: SharedReceipts,
        reply: SharedReplyReservation,
        aliases: usize,
        adoptions: usize,
        symbols: &ipp_core::BatchSymbolReports,
    ) -> Result<(Self, SharedReplyReservation), ErrorReason> {
        reply.borrow_mut().reserve_bytes(128)?;
        // Every alias, adoption and symbol report the outcome can carry is
        // admitted with the batch at its encoded size, so an outcome that cannot
        // be answered refuses the batch before its first operation instead of
        // failing after it applied. Attachment effects, bounded by the receipt
        // registry, are still reserved per operation.
        let reports = |per_alias: usize, per_symbol: usize| {
            aliases
                .checked_mul(per_alias)?
                .checked_add(symbols.reports().checked_mul(per_symbol)?)?
                .checked_add(symbols.text_bytes())
        };
        Ok((
            Self {
                registry,
                reply: reply.clone(),
                report_bytes: reports(ALIAS_WIRE_BYTES, SYMBOL_WIRE_BYTES)
                    .and_then(|bytes| {
                        bytes.checked_add(adoptions.checked_mul(ADOPTED_EFFECT_BYTES)?)
                    })
                    .ok_or(ErrorReason::Capacity)?,
                report_retained: reports(ALIAS_RETAINED_BYTES, SYMBOL_RETAINED_BYTES)
                    .ok_or(ErrorReason::Capacity)?,
                slots: Vec::new(),
                remaining_records: 0,
            },
            reply,
        ))
    }
}

impl OperationEffectSink for ReceiptSink {
    fn reserve(&mut self, demand: &OperationEffectDemand) -> Result<(), ErrorReason> {
        let registry = self.registry.borrow();
        if !registry.live {
            return Err(ErrorReason::InvalidEntity);
        }
        if self.report_retained != 0 {
            self.reply
                .borrow_mut()
                .reserve_retained(self.report_retained)?;
            self.report_retained = 0;
        }
        let missing = demand
            .existing_attachment_tokens
            .iter()
            .filter(|token| !registry.identities.contains_key(&token.identity()))
            .map(WorldAttachmentToken::identity)
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        let slots = missing
            .checked_add(demand.fresh_attachment_tokens)
            .ok_or(ErrorReason::Capacity)?;
        if slots > registry.limit.saturating_sub(registry.tokens.len()) {
            return Err(ErrorReason::Capacity);
        }
        self.reply.borrow_mut().grow(
            demand
                .max_records
                .checked_mul(MAX_ATTACHMENT_EFFECT_BYTES)
                .and_then(|bytes| bytes.checked_add(self.report_bytes))
                .ok_or(ErrorReason::Capacity)?,
        )?;
        self.report_bytes = 0;
        self.remaining_records = demand.max_records;
        for _ in 0..slots {
            self.slots
                .push(super::ingress::next_ingress_id().map_err(|_| ErrorReason::Capacity)?);
        }
        Ok(())
    }

    fn emit(&mut self, effect: OperationEffect) {
        let OperationEffect::WorldAttachment(effect) = effect else {
            // Adoption reports need no receipt; the batch admitted their bytes.
            return;
        };
        assert!(self.remaining_records > 0);
        self.remaining_records -= 1;
        let token = match effect {
            WorldAttachmentEffect::Written(token)
            | WorldAttachmentEffect::Detached(token)
            | WorldAttachmentEffect::Superseded(token) => token,
        };
        let mut registry = self.registry.borrow_mut();
        if let std::collections::btree_map::Entry::Vacant(entry) =
            registry.identities.entry(token.identity())
        {
            let id = self.slots.pop().expect("reserved receipt slot");
            entry.insert(id);
            registry.tokens.insert(id, token);
        }
    }

    fn settle(&mut self) {
        self.slots.clear();
        let mut reply = self.reply.borrow_mut();
        let retained = reply.bytes - self.remaining_records * MAX_ATTACHMENT_EFFECT_BYTES;
        reply.shrink(retained);
        self.remaining_records = 0;
    }

    fn attachment_receipt(&self, receipt: u64) -> Result<WorldAttachmentToken, ErrorReason> {
        self.registry.borrow().resolve(receipt)
    }
}
