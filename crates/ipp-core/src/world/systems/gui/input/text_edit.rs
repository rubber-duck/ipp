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
///
/// Produces the pen positions of [`TextLayout::caret_position`] for every
/// boundary in one merged pass. Measured lines and their glyphs follow
/// source order (basic-LTR subset), so the first line starting at, and the
/// first line ending at or after, each ascending boundary only move forward,
/// as does the first glyph starting at or after it.
pub(crate) fn caret_offset_at_x(layout: &TextLayout, x_ems: f32) -> u32 {
    let lines = &layout.lines;
    let glyphs = &layout.glyphs;
    let mut starting_line = 0;
    let mut ending_line = 0;
    let mut glyph = 0;
    let mut best = 0_u32;
    let mut best_dist = f32::INFINITY;

    for boundary in layout.grapheme_boundaries.iter().copied() {
        while starting_line < lines.len() && lines[starting_line].source_range[0] < boundary {
            record_step();
            starting_line += 1;
        }

        let caret_x = if lines
            .get(starting_line)
            .is_some_and(|line| line.source_range[0] == boundary)
        {
            0.0
        } else {
            while ending_line < lines.len() && lines[ending_line].source_range[1] < boundary {
                record_step();
                ending_line += 1;
            }

            // A boundary between lines (a consumed break or wrap space) has
            // no caret geometry.
            let Some(line) = lines.get(ending_line) else {
                continue;
            };
            let [start, end] = line.source_range;
            if start >= boundary {
                continue;
            }

            if boundary == end {
                line.width
            } else {
                let [first, last] = line.glyph_range.map(|index| index as usize);
                glyph = glyph.max(first);
                while glyph < last && glyphs[glyph].source_range[0] < boundary {
                    record_step();
                    glyph += 1;
                }

                if glyph < last {
                    glyphs[glyph].position[0]
                } else {
                    line.width
                }
            }
        };

        record_step();
        let dist = (caret_x - x_ems).abs();
        if dist < best_dist || (dist == best_dist && boundary > best) {
            best_dist = dist;
            best = boundary;
        }
    }

    best
}

#[cfg(test)]
thread_local! {
    /// Line, glyph and boundary steps taken by [`caret_offset_at_x`].
    static CARET_STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[inline]
fn record_step() {
    #[cfg(test)]
    CARET_STEPS.set(CARET_STEPS.get() + 1);
}

#[cfg(test)]
#[path = "text_edit_tests.rs"]
mod tests;
