//! Upper bound on the symbol reports of one batch outcome, counted before the
//! batch applies.
//!
//! [`BatchOutcome::symbols`](crate::BatchOutcome::symbols) carries one entry per
//! distinct symbol and handle that the batch's symbolic references resolved to.
//! A symbol names another handle only after a command of the same batch gives it
//! to an entity, through the metadata of a [`Command::Create`] or
//! [`Command::SetMetadata`]. A symbol therefore reports at most once per
//! reference and at most once more than the commands that assign it; in an
//! ordinary batch that is one report per distinct symbol. A Host whose outcome
//! encoding bounds the reports or their size counts the batch before it applies
//! and refuses it whole, so no batch applies whose outcome it could not report.

use super::{Command, EntityRef};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Distinct symbols of one logical batch and the most outcome entries they can
/// produce, accumulated over its commands in any order.
#[derive(Debug, Default)]
pub struct BatchSymbolReports {
    symbols: BTreeMap<Arc<str>, SymbolUses>,
    reports: usize,
    text_bytes: usize,
}

#[derive(Debug, Default)]
struct SymbolUses {
    references: usize,
    assignments: usize,
}

impl SymbolUses {
    fn reports(&self) -> usize {
        self.references.min(self.assignments.saturating_add(1))
    }
}

impl BatchSymbolReports {
    /// Count one command's symbolic references and symbol assignment. The
    /// command is not changed; the mutable borrow shares the one exhaustive
    /// reference visitor of [`Command`].
    pub fn add(&mut self, command: &mut Command) {
        if let Command::Create {
            metadata,
            ..
        }
        | Command::SetMetadata {
            metadata,
            ..
        } = command
            && let Some(symbol) = &metadata.symbolic_id
        {
            let symbol = match self.symbols.get_key_value(symbol.as_str()) {
                Some((known, _)) => known.clone(),
                None => Arc::from(symbol.as_str()),
            };
            self.update(&symbol, |uses| uses.assignments += 1);
        }

        let _ = command.visit_entity_refs_mut(&mut |reference| {
            if let EntityRef::Symbol(symbol) = reference {
                self.update(symbol, |uses| uses.references += 1);
            }
            Ok::<(), std::convert::Infallible>(())
        });
    }

    fn update(&mut self, symbol: &Arc<str>, change: impl FnOnce(&mut SymbolUses)) {
        let uses = self.symbols.entry(symbol.clone()).or_default();
        let before = uses.reports();
        change(uses);
        // Counts only grow, so neither does a symbol's bound.
        let added = uses.reports() - before;
        self.reports = self.reports.saturating_add(added);
        self.text_bytes = self
            .text_bytes
            .saturating_add(added.saturating_mul(symbol.len()));
    }

    /// Most entries the batch outcome can report for the commands counted so far.
    pub fn reports(&self) -> usize {
        self.reports
    }

    /// Most symbol text those entries can carry, in UTF-8 bytes.
    pub fn text_bytes(&self) -> usize {
        self.text_bytes
    }
}

#[cfg(test)]
#[path = "batch_symbol_reports_tests.rs"]
mod tests;
