//! Pure single-line text-edit helpers over UTF-8 byte offsets.
//!
//! All caret offsets are byte offsets that must sit on grapheme boundaries
//! (basic-LTR subset in [`crate::systems::surface`]). Insertion and deletion
//! snap the resulting caret forward to the next boundary in the new text.

use crate::systems::surface::{TextLayout, grapheme_boundaries, is_grapheme_boundary};

/// Outcome of one committed-text edit with its snapped caret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EditOutcome {
    /// New committed text bytes.
    pub text: String,
    /// Snapped caret byte offset in `text`.
    pub caret: u32,
}

/// Whether `offset` is a legal caret position in `text`.
pub(crate) fn is_boundary(text: &str, offset: u32) -> bool {
    is_grapheme_boundary(text, offset)
}

/// Largest grapheme boundary strictly below `offset`, if any.
pub(crate) fn prev_boundary(text: &str, offset: u32) -> Option<u32> {
    let mut best: Option<u32> = None;
    for boundary in grapheme_boundaries(text) {
        if boundary < offset {
            best = Some(boundary);
        } else {
            break;
        }
    }
    best
}

/// Smallest grapheme boundary strictly above `offset`, if any.
pub(crate) fn next_boundary(text: &str, offset: u32) -> Option<u32> {
    grapheme_boundaries(text)
        .into_iter()
        .find(|boundary| *boundary > offset)
}

/// Snap `offset` forward to the next grapheme boundary in `text`.
/// Offsets past the text clamp to its length (always a boundary).
pub(crate) fn snap_to_boundary(text: &str, offset: u32) -> u32 {
    let len = text.len() as u32;
    let clamped = offset.min(len);
    if is_grapheme_boundary(text, clamped) {
        return clamped;
    }
    next_boundary(text, clamped).unwrap_or(len)
}

/// Normalize a selection range into `(start, end)` with `start <= end`.
pub(crate) fn normalize_range(start: u32, end: u32) -> (u32, u32) {
    if start <= end {
        (start, end)
    } else {
        (end, start)
    }
}

/// Insert `payload` over `[start, end)` (selection or collapsed caret).
/// Callers validate `start`/`end` against the old text; the caret snaps in
/// the new text.
pub(crate) fn insert_at(text: &str, start: u32, end: u32, payload: &str) -> EditOutcome {
    let (start, end) = normalize_range(start, end);
    let start = start.min(text.len() as u32);
    let end = end.min(text.len() as u32);
    let (start, end) = (start as usize, end as usize);
    let mut out = String::with_capacity(text.len() + payload.len());
    out.push_str(&text[..start]);
    out.push_str(payload);
    out.push_str(&text[end..]);
    let caret = snap_to_boundary(&out, (start + payload.len()) as u32);
    EditOutcome {
        text: out,
        caret,
    }
}

/// Delete `[start, end)` with the caret snapped to `start` in the new text.
pub(crate) fn delete_range(text: &str, start: u32, end: u32) -> EditOutcome {
    let (start, end) = normalize_range(start, end);
    let start = start.min(text.len() as u32);
    let end = end.min(text.len() as u32);
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..start as usize]);
    out.push_str(&text[end as usize..]);
    let caret = snap_to_boundary(&out, start);
    EditOutcome {
        text: out,
        caret,
    }
}

/// Backspace over a selection or the single grapheme before `caret`.
/// Returns `None` when collapsed at zero (no-op).
pub(crate) fn backspace(text: &str, caret: u32, anchor: Option<u32>) -> Option<EditOutcome> {
    if let Some(anchor) = anchor
        && anchor != caret
    {
        return Some(delete_range(text, anchor, caret));
    }
    let prev = prev_boundary(text, caret)?;
    Some(delete_range(text, prev, caret))
}

/// Forward delete over a selection or the single grapheme after `caret`.
/// Returns `None` when collapsed at the end (no-op).
pub(crate) fn delete_forward(text: &str, caret: u32, anchor: Option<u32>) -> Option<EditOutcome> {
    if let Some(anchor) = anchor
        && anchor != caret
    {
        return Some(delete_range(text, anchor, caret));
    }
    let next = next_boundary(text, caret)?;
    Some(delete_range(text, caret, next))
}

/// Nearest legal caret offset to an em pen `x` on a measured layout.
/// Ties prefer the later offset so a centred single glyph keeps an
/// append-style caret at the end.
pub(crate) fn caret_offset_at_x(layout: &TextLayout, x_ems: f32) -> u32 {
    let mut best = 0_u32;
    let mut best_dist = f32::INFINITY;
    for boundary in layout.grapheme_boundaries.iter().copied() {
        let Some(caret) = layout.caret_position(boundary) else {
            continue;
        };
        let dist = (caret.position[0] - x_ems).abs();
        if dist < best_dist || (dist == best_dist && boundary > best) {
            best_dist = dist;
            best = boundary;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combining_mark_is_one_grapheme() {
        let text = "a\u{301}";
        assert_eq!(grapheme_boundaries(text), vec![0, 3]);
        assert!(is_boundary(text, 0));
        assert!(!is_boundary(text, 1));
        assert!(is_boundary(text, 3));
        assert_eq!(prev_boundary(text, 3), Some(0));
        assert_eq!(prev_boundary(text, 0), None);
        assert_eq!(next_boundary(text, 0), Some(3));
    }

    #[test]
    fn backspace_deletes_whole_grapheme() {
        let text = "a\u{301}";
        let outcome = backspace(text, 3, None).unwrap();
        assert_eq!(outcome.text, "");
        assert_eq!(outcome.caret, 0);
        assert!(backspace("", 0, None).is_none());
    }

    #[test]
    fn insert_snaps_caret_in_new_text() {
        let outcome = insert_at("ae", 1, 1, "V");
        assert_eq!(outcome.text, "aVe");
        assert_eq!(outcome.caret, 2);
        let replaced = insert_at("aVe", 0, 1, "e");
        assert_eq!(replaced.text, "eVe");
        assert_eq!(replaced.caret, 1);
    }
}
