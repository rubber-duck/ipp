//! Provisional IME composition, distinct from committed text.
//!
//! One active composition per world at most. It is revision- and focus-fenced:
//! `revision` is the committed/predicted revision the provisional was started
//! against, `target` is the focused text input it belongs to, and `session`
//! is the input owner. Commits insert the provisional through the ordinary
//! revision-gated envelope path; cancels and focus/visibility/session loss
//! drop it without a commit.

use super::system::GuiInputTarget;
use crate::systems::surface::is_grapheme_boundary;

/// Active provisional composition for one text input.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ActiveComposition {
    /// Focused text input owning the provisional.
    pub target: GuiInputTarget,
    /// Session that started it.
    pub session: u64,
    /// Base revision it was started against.
    pub revision: u32,
    /// Provisional IME text, never committed until explicit commit.
    pub provisional: String,
    /// Caret/selection within `provisional` (byte offsets, boundaries).
    pub caret_start: u32,
    /// Caret/selection within `provisional` (byte offsets, boundaries).
    pub caret_end: u32,
}

impl ActiveComposition {
    /// Start or replace the provisional, validating its internal carets.
    /// Returns `None` when the provisional carets split the provisional.
    pub(crate) fn new(
        target: GuiInputTarget,
        session: u64,
        revision: u32,
        provisional: String,
        caret_start: u32,
        caret_end: u32,
    ) -> Option<Self> {
        if provisional.len() > super::super::tree::nodes::MAX_TEXT_BYTES {
            return None;
        }
        if !is_grapheme_boundary(&provisional, caret_start.min(provisional.len() as u32)) {
            return None;
        }
        if !is_grapheme_boundary(&provisional, caret_end.min(provisional.len() as u32)) {
            return None;
        }
        if caret_start > provisional.len() as u32 || caret_end > provisional.len() as u32 {
            return None;
        }
        Some(Self {
            target,
            session,
            revision,
            provisional,
            caret_start,
            caret_end,
        })
    }

    /// Whether this composition belongs to `target` under `session`.
    pub(crate) fn is_for(&self, target: &GuiInputTarget, session: u64) -> bool {
        self.target == *target && self.session == session
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EntityId;
    use crate::GuiNodeId;

    fn target() -> GuiInputTarget {
        GuiInputTarget {
            entity: EntityId::from_bits(1),
            node: GuiNodeId(2),
            lifetime: 1,
            root_incarnation: 9,
        }
    }

    #[test]
    fn rejects_split_provisional_caret() {
        let provisional = "a\u{301}";
        assert!(ActiveComposition::new(target(), 7, 1, provisional.into(), 0, 3).is_some());
        assert!(ActiveComposition::new(target(), 7, 1, provisional.into(), 1, 3).is_none());
    }
}
