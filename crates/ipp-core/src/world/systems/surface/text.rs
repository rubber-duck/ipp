//! Shared headless text measurement with grapheme-aware caret metrics.
//!
//! One implementation serves Surface label preparation, GUI layout (ipp-9nx.5)
//! and future text editing caret/selection work (ipp-9nx.8). It is deliberately
//! headless: it borrows an immutable [`FontAsset`](crate::services::asset_management::font::FontAsset),
//! never touches cameras, colours, Worlds or sessions, and never synthesizes
//! metrics for fonts that are not ready.
//!
//! ## Units and coordinates
//!
//! All geometry is in **ems**. The caller scales to Surface metres by
//! multiplying with the validated `font_size` (metres per em) echoed on
//! [`TextLayout::font_size`]; GUI consumers additionally apply their
//! `unitsPerMetre` factor. Keeping measurement scale-free means wrapping,
//! kerning and advance decisions are identical at every size.
//!
//! Positions consume the shared top-left/Y-down Surface content convention
//! (ipp-9nx.16): the layout origin is the top-left of the first line, +X runs
//! right and +Y runs down. Each glyph origin sits on its line's alphabetic
//! baseline, which [`TextLine::baseline`] measures down from the layout origin.
//! Line tops, carets and selection rectangles remain line-box geometry. This
//! module leaves the layout origin at the first line's top-left and performs no
//! Y flip.
//!
//! ## Request inputs
//!
//! [`TextMeasureRequest`] carries the exact identified text revision (runtime
//! source ranges are half-open UTF-8 byte offsets into that revision), the
//! immutable font identity with its readiness ([`TextFont`]), the validated
//! font size, the explicit [`TextLinePolicy`] and the finite or unbounded
//! [`TextMaxWidth`] constraint in ems.
//!
//! ## Results
//!
//! [`TextLayout`] reports original IPPF glyph IDs, em placements and advances,
//! per-line extents and baselines, per-glyph source ranges, the legal caret
//! offsets ([`TextLayout::grapheme_boundaries`]) and caret/selection mappings
//! ([`TextLayout::caret_position`], [`TextLayout::selection_rects`]). Missing
//! scalars keep their source range and resolve to glyph `0` (`.notdef`) with
//! [`TextGlyph::missing`] set; see the missing-glyph policy below.
//!
//! ## Readiness
//!
//! [`TextFont::Pending`] measures to [`TextOutcome::PendingFont`]. Dependent
//! layout/caret output must stay marked unavailable until the font is ready
//! and the text is rebuilt; metrics are never guessed. Missing glyphs in a
//! *ready* font are different: they are observable fallback inside a measured
//! layout, not unavailability.
//!
//! ## Cache keys
//!
//! [`TextCacheKey`] covers the text bytes, the font [`AssetKey`]
//! (slot plus generation, so font replacement invalidates), the line policy,
//! the width constraint and the font size. Camera state, colours, opacity and
//! other paint-only inputs are never key members: they must not trigger
//! remeasurement. Content, resource or constraint edits change the key and
//! rebuild; pending fonts rebuild on readiness.
//!
//! ## Recorded policies
//!
//! * Segmentation: complete UAX #29 extended grapheme clusters for the Unicode
//!   version in [`UNICODE_VERSION`] (see [`SEGMENTATION_SCOPE`] and
//!   [`is_grapheme_boundary`]) whenever the `gui` feature is enabled, via the
//!   `unicode-segmentation` dependency justified below. Without `gui`,
//!   measurement keeps the documented basic-LTR subset so simple Surface
//!   labels work with no Unicode dependency. Either way, complex shaping,
//!   bidi and emoji-presentation rendering stay out of scope: every scalar
//!   maps to at most one glyph, and missing glyphs never alter edit
//!   boundaries.
//! * Missing glyphs: substitute `.notdef` (original ID `0`), retain the source
//!   range, set the missing flag. Never skip the scalar or invent an advance.
//! * Wrapping: greedy basic-LTR wrap at `U+0020` opportunities under a finite
//!   width; the opportunity space is consumed (no glyph emitted) and leading
//!   spaces after a soft wrap are skipped. Words wider than the constraint
//!   hard-break between scalars, keeping a combining mark with its base when
//!   the base already fits. Explicit breaks come from `LF`, `CRLF` (one break)
//!   and lone `CR`. `U+0009` and other controls have no special handling and
//!   flow through cmap/missing reporting. [`TextLinePolicy::SingleLine`]
//!   folds every line break to `U+0020` and never wraps.
//! * Kerning applies between adjacent emitted glyphs on one line by original
//!   glyph ID, including `.notdef` pairs (usually absent, contributing zero).
//!   Kerning never crosses a line boundary.
//! * Dependency cost: `unicode-segmentation` (established Unicode tables with
//!   no transitive dependencies, used for segmentation only) is optional and
//!   gated on the `gui` capability, the only configuration that edits text.
//!   Surface labels without `gui` keep the dependency-free inline subset
//!   tables, so lean builds omit the cost entirely.
//!
//! ## Browser offset conversion
//!
//! Runtime offsets are UTF-8 byte offsets. Browser adapters hold UTF-16
//! code-unit offsets into the *same text revision*. Convert with
//! [`utf16_to_utf8_offset`] and [`utf8_to_utf16_offset`]; both return `None`
//! when the input splits a scalar instead of silently rounding. Bytes, scalar
//! counts and glyph indices are never interchangeable.

use crate::services::asset_management::{AssetKey, font::FontAsset};

#[cfg(test)]
#[path = "text_tests.rs"]
mod tests;

/// Unicode version whose `GraphemeBreakProperty` semantics the `gui`
/// segmentation follows. This is the table version exported by the selected
/// `unicode-segmentation` dependency, so dependency updates cannot leave the
/// runtime metadata stale.
#[cfg(feature = "gui")]
pub const UNICODE_VERSION: (u64, u64, u64) = unicode_segmentation::UNICODE_VERSION;

/// Unicode version whose `GraphemeBreakProperty` semantics the dependency-free
/// inline [`is_grapheme_boundary`] subset follows.
#[cfg(not(feature = "gui"))]
pub const UNICODE_VERSION: (u64, u64, u64) = (16, 0, 0);

/// Segmentation scope owned by this module with the `gui` feature: complete
/// UAX #29 extended grapheme clusters. Rendering guarantees stay limited to
/// basic LTR Latin; only edit boundaries are complete.
#[cfg(feature = "gui")]
pub const SEGMENTATION_SCOPE: &str = "uax29-extended";

/// Segmentation scope owned by this module without the `gui` feature:
/// basic-LTR subset, no shaping stack. Covers `CR`/`LF` control handling plus
/// combining-mark blocks used with Latin, Greek, Cyrillic, Hebrew, Arabic and
/// Syriac diacritics, `ZWJ` carry-through and variation selectors. Hangul
/// jamo, Indic scripts outside the listed ranges, emoji `ZWJ`/
/// regional-indicator sequences and `Prepend` characters fall back to
/// per-scalar boundaries; full UAX #29 arrives with the `gui` feature above.
#[cfg(not(feature = "gui"))]
pub const SEGMENTATION_SCOPE: &str = "basic-ltr-subset";

/// Extended grapheme segmentation for preserved source text.
#[cfg(feature = "gui")]
use unicode_segmentation::UnicodeSegmentation;

/// Geometry unit used by every measurement result in this module.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextUnits {
    /// Font-relative units; multiply by `font_size` (metres per em) for
    /// Surface metres.
    Ems,
}

/// Immutable font identity with explicit readiness for measurement.
#[derive(Clone, Copy, Debug)]
pub enum TextFont<'a> {
    /// Ready immutable font; `key` (slot plus generation) identifies the
    /// exact asset incarnation for cache keys.
    Ready {
        /// Runtime slot handle of the ready font asset.
        key: AssetKey,
        /// Borrowed immutable decoded font metrics.
        font: &'a FontAsset,
    },
    /// Font not yet available; measurement stays unavailable until rebuilt.
    Pending {
        /// Runtime slot handle the caller waits on.
        key: AssetKey,
    },
}

/// Explicit line-break policy for measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextLinePolicy {
    /// Fold every line break to `U+0020` and never wrap, whatever the width
    /// constraint. Single-line inputs measure (and edit) on one line.
    SingleLine,
    /// Honour explicit breaks and wrap only under a finite width constraint.
    Multiline,
}

/// Width constraint in ems for measurement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextMaxWidth {
    /// No wrapping; lines grow without bound.
    Unbounded,
    /// Greedy wrap so finished lines end at or under this em width.
    /// Must be finite and positive; validated by [`TextMeasureRequest::new`].
    Ems(f32),
}

impl TextMaxWidth {
    fn bits(self) -> u32 {
        match self {
            Self::Unbounded => u32::MAX,
            Self::Ems(width) => width.to_bits(),
        }
    }
}

/// Rejected measurement request parameter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextRequestError {
    /// `font_size` (metres per em) was not finite and positive.
    FontSize(f32),
    /// Finite width constraint was not finite and positive.
    MaxWidth(f32),
}

impl std::fmt::Display for TextRequestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FontSize(value) => {
                write!(
                    formatter,
                    "text font size must be finite and positive, got {value}"
                )
            }
            Self::MaxWidth(value) => {
                write!(
                    formatter,
                    "text max width must be finite and positive, got {value}"
                )
            }
        }
    }
}

/// Headless measurement request. See the module docs for units and policies.
#[derive(Clone, Copy, Debug)]
pub struct TextMeasureRequest<'a> {
    /// Exact text revision; all source ranges index these bytes.
    pub text: &'a str,
    /// Immutable font identity and readiness.
    pub font: TextFont<'a>,
    /// Metres per em for scaling em results; validated finite and positive.
    pub font_size: f32,
    /// Explicit line-break policy.
    pub line_policy: TextLinePolicy,
    /// Finite or unbounded width constraint in ems.
    pub max_width: TextMaxWidth,
}

impl<'a> TextMeasureRequest<'a> {
    /// Validate scalar parameters without touching font readiness.
    pub fn new(
        text: &'a str,
        font: TextFont<'a>,
        font_size: f32,
        line_policy: TextLinePolicy,
        max_width: TextMaxWidth,
    ) -> Result<Self, TextRequestError> {
        if !font_size.is_finite() || font_size <= 0.0 {
            return Err(TextRequestError::FontSize(font_size));
        }

        if let TextMaxWidth::Ems(width) = max_width
            && (!width.is_finite() || width <= 0.0)
        {
            return Err(TextRequestError::MaxWidth(width));
        }

        Ok(Self {
            text,
            font,
            font_size,
            line_policy,
            max_width,
        })
    }

    /// Cache identity for this request. Covers text, font incarnation, line
    /// policy, width constraint and font size; never camera or paint state.
    pub fn cache_key(&self) -> TextCacheKey<'a> {
        TextCacheKey {
            text: self.text,
            font: match self.font {
                TextFont::Ready {
                    key,
                    ..
                } => key,
                TextFont::Pending {
                    key,
                } => key,
            },
            line_policy: self.line_policy,
            max_width_bits: self.max_width.bits(),
            font_size_bits: self.font_size.to_bits(),
        }
    }
}

/// Cache identity for measured text. Compare (or hash) per retained item and
/// rebuild the layout whenever it differs; reuse it otherwise, including
/// across camera-only or paint-only changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextCacheKey<'a> {
    /// Measured text bytes.
    pub text: &'a str,
    /// Font slot plus generation; replacement invalidates.
    pub font: AssetKey,
    /// Line-break policy.
    pub line_policy: TextLinePolicy,
    /// Width constraint bits (`u32::MAX` for unbounded).
    pub max_width_bits: u32,
    /// Font size bits (output scale member of the key).
    pub font_size_bits: u32,
}

/// Measured outcome. Pending fonts never produce guessed metrics.
#[derive(Clone, Debug)]
pub enum TextOutcome {
    /// Completed headless measurement in ems.
    Measured(TextLayout),
    /// Font not ready; keep dependent output unavailable and rebuild later.
    PendingFont {
        /// Runtime slot handle the caller waits on.
        key: AssetKey,
    },
}

/// One placed glyph with its source range and fallback status.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextGlyph {
    /// Original IPPF glyph ID; `0` (`.notdef`) when [`TextGlyph::missing`].
    pub glyph_id: u32,
    /// Em offset of the glyph origin from the layout origin (top-left,
    /// Y-down). The origin sits on the glyph's alphabetic line baseline.
    pub position: [f32; 2],
    /// Em advance of this glyph, excluding neighbouring kerning.
    pub advance: f32,
    /// Half-open UTF-8 byte offsets of the source scalar.
    pub source_range: [u32; 2],
    /// True when the font cmap lacked the scalar and `.notdef` was used.
    pub missing: bool,
}

/// One finished line: extents, baseline and source coverage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextLine {
    /// Half-open glyph indices into [`TextLayout::glyphs`].
    pub glyph_range: [u32; 2],
    /// Em offset of the line top from the layout origin.
    pub top: f32,
    /// Em offset of the alphabetic baseline from the layout origin.
    pub baseline: f32,
    /// Em line height (`ascender - descender + line_gap`).
    pub height: f32,
    /// Em advance width of emitted glyphs, excluding consumed wrap spaces.
    pub width: f32,
    /// Half-open UTF-8 byte offsets of emitted line content, excluding the
    /// terminating break and any consumed wrap space. Empty lines report
    /// `[offset, offset]` at their starting offset.
    pub source_range: [u32; 2],
    /// True when an explicit break (`LF`/`CRLF`/`CR`) ended this line.
    /// Soft-wrapped and final lines report false.
    pub terminated: bool,
}

/// Caret geometry for one legal boundary offset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextCaret {
    /// Index into [`TextLayout::lines`].
    pub line: u32,
    /// Em caret position (pen location, top-left/Y-down space).
    pub position: [f32; 2],
    /// Em caret height (the line height).
    pub height: f32,
}

/// Measured headless text in ems. Multiply positions, advances, extents and
/// baselines by [`TextLayout::font_size`] for Surface metres.
#[derive(Clone, Debug, PartialEq)]
pub struct TextLayout {
    /// Metres-per-em scale echoed from the request; the fixed output scale.
    pub font_size: f32,
    /// Placed glyphs in reading order, original IDs.
    pub glyphs: Vec<TextGlyph>,
    /// Finished lines; always at least one, even for empty text.
    pub lines: Vec<TextLine>,
    /// Sorted legal caret offsets: half-open UTF-8 byte boundaries at
    /// grapheme edges, always including `0` and the text length.
    pub grapheme_boundaries: Vec<u32>,
    /// Total `[width, height]` extents in ems.
    pub size: [f32; 2],
}

impl TextLayout {
    /// Geometry unit of every field; always ems.
    pub const fn units(&self) -> TextUnits {
        TextUnits::Ems
    }

    /// True when `offset` is a legal caret position (a grapheme edge,
    /// including `0` and the text length).
    pub fn is_caret_position(&self, offset: u32) -> bool {
        self.grapheme_boundaries.binary_search(&offset).is_ok()
    }

    /// Number of placed glyphs that fell back to `.notdef`.
    pub fn missing_glyph_count(&self) -> usize {
        self.glyphs.iter().filter(|glyph| glyph.missing).count()
    }

    /// Caret geometry for a legal boundary offset, or `None` when `offset`
    /// splits a scalar or grapheme, or lies past the text. An offset shared
    /// by a line end and the next line start prefers the next line start.
    pub fn caret_position(&self, offset: u32) -> Option<TextCaret> {
        if !self.is_caret_position(offset) {
            return None;
        }

        for (index, line) in self.lines.iter().enumerate() {
            if line.source_range[0] == offset {
                return Some(TextCaret {
                    line: index as u32,
                    position: [0.0, line.top],
                    height: line.height,
                });
            }
        }

        for (index, line) in self.lines.iter().enumerate() {
            let [start, end] = line.source_range;

            if offset == end && offset >= start {
                return Some(TextCaret {
                    line: index as u32,
                    position: [line.width, line.top],
                    height: line.height,
                });
            }

            if offset > start && offset < end {
                for glyph_index in line.glyph_range[0]..line.glyph_range[1] {
                    let glyph = self.glyphs[glyph_index as usize];

                    if glyph.source_range[0] >= offset {
                        return Some(TextCaret {
                            line: index as u32,
                            position: [glyph.position[0], line.top],
                            height: line.height,
                        });
                    }
                }

                return Some(TextCaret {
                    line: index as u32,
                    position: [line.width, line.top],
                    height: line.height,
                });
            }
        }

        None
    }

    /// Per-line `[x, y, width, height]` selection rectangles in ems for the
    /// half-open caret range `[start, end)`. The endpoints swap to normalise
    /// order; both must be legal caret positions or the result is empty.
    /// Zero-width lines contribute no rectangle.
    pub fn selection_rects(&self, start: u32, end: u32) -> Vec<[f32; 4]> {
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };

        if start == end || !self.is_caret_position(start) || !self.is_caret_position(end) {
            return Vec::new();
        }

        let mut rects = Vec::new();

        for line in &self.lines {
            let [line_start, line_end] = line.source_range;

            if end <= line_start || start >= line_end {
                continue;
            }

            let from = start.max(line_start);
            let to = end.min(line_end);
            let x0 = line_pen_at(line, &self.glyphs, from);
            let x1 = line_pen_at(line, &self.glyphs, to);

            if x1 > x0 {
                rects.push([x0, line.top, x1 - x0, line.height]);
            }
        }

        rects
    }
}

/// Pen x for an offset clamped inside one line's content range.
fn line_pen_at(line: &TextLine, glyphs: &[TextGlyph], offset: u32) -> f32 {
    let [start, end] = line.source_range;

    if offset <= start {
        return 0.0;
    }

    if offset >= end {
        return line.width;
    }

    for glyph_index in line.glyph_range[0]..line.glyph_range[1] {
        let glyph = glyphs[glyph_index as usize];

        if glyph.source_range[0] >= offset {
            return glyph.position[0];
        }
    }

    line.width
}

/// Resolve one scalar to its original glyph ID, falling back to `.notdef`.
fn resolve_glyph(font: &FontAsset, character: char) -> (u32, bool) {
    match font.glyph_id(character) {
        Some(glyph_id) => (glyph_id, false),
        None => (0, true),
    }
}

/// Em advance of one original glyph ID (`.notdef` always exists).
fn glyph_advance_ems(font: &FontAsset, glyph_id: u32, unit: f32) -> f32 {
    font.glyph(glyph_id)
        .map_or(0.0, |glyph| glyph.advance * unit)
}

/// One paragraph: scalar content with byte offsets, explicit-break flag and
/// the byte offset where the paragraph starts.
struct Paragraph {
    chars: Vec<(char, u32, u32)>,
    terminated: bool,
    start_byte: u32,
}

/// Split text into paragraphs on `LF`, `CRLF` (one break) and lone `CR`.
/// [`TextLinePolicy::SingleLine`] folds every break to `U+0020` instead.
fn split_paragraphs(text: &str, single_line: bool) -> Vec<Paragraph> {
    let mut paragraphs = Vec::new();
    let mut current: Vec<(char, u32, u32)> = Vec::new();
    let mut start_byte = 0_u32;
    let mut cursor = text.char_indices().peekable();

    while let Some((byte, character)) = cursor.next() {
        let byte = byte as u32;
        let length = character.len_utf8() as u32;

        if character == '\n' || character == '\r' {
            let break_end =
                if character == '\r' && cursor.peek().is_some_and(|(_, next)| *next == '\n') {
                    let (lf_byte, _) = cursor.next().unwrap();
                    lf_byte as u32 + 1
                } else {
                    byte + length
                };

            if single_line {
                current.push((' ', byte, break_end - byte));
                continue;
            }

            paragraphs.push(Paragraph {
                chars: std::mem::take(&mut current),
                terminated: true,
                start_byte,
            });
            start_byte = break_end;
            continue;
        }

        current.push((character, byte, length));
    }

    paragraphs.push(Paragraph {
        chars: current,
        terminated: false,
        start_byte,
    });

    paragraphs
}

/// Measure text headlessly against an immutable ready font, or report the
/// pending font without synthesizing metrics.
pub fn measure_text(request: &TextMeasureRequest<'_>) -> TextOutcome {
    let font = match request.font {
        TextFont::Ready {
            font,
            ..
        } => font,
        TextFont::Pending {
            key,
        } => {
            return TextOutcome::PendingFont {
                key,
            };
        }
    };

    let unit = 1.0 / font.units_per_em() as f32;
    let ascender = font.ascender() * unit;
    let line_height = (font.ascender() - font.descender() + font.line_gap()) * unit;
    let limit = match request.max_width {
        TextMaxWidth::Unbounded => None,
        TextMaxWidth::Ems(width) => Some(width),
    };
    let wrap = request.line_policy == TextLinePolicy::Multiline && limit.is_some();

    let mut glyphs: Vec<TextGlyph> = Vec::with_capacity(request.text.len());
    let mut lines: Vec<TextLine> = Vec::new();
    let mut line_top = 0.0;

    for paragraph in split_paragraphs(
        request.text,
        request.line_policy == TextLinePolicy::SingleLine,
    ) {
        if !wrap {
            line_top = push_plain_line(
                font,
                unit,
                ascender,
                line_height,
                &paragraph,
                line_top,
                &mut glyphs,
                &mut lines,
            );
            continue;
        }

        line_top = push_wrapped_lines(
            font,
            request.text,
            unit,
            ascender,
            line_height,
            limit.unwrap_or(f32::INFINITY),
            &paragraph,
            line_top,
            &mut glyphs,
            &mut lines,
        );
    }

    debug_assert!(!lines.is_empty());

    let width = lines.iter().map(|line| line.width).fold(0.0_f32, f32::max);

    TextOutcome::Measured(TextLayout {
        font_size: request.font_size,
        glyphs,
        lines,
        grapheme_boundaries: grapheme_boundaries(request.text),
        size: [width, line_top],
    })
}

/// Emit one unwrapped line: every scalar in order with pairwise kerning.
#[allow(clippy::too_many_arguments)]
fn push_plain_line(
    font: &FontAsset,
    unit: f32,
    ascender: f32,
    line_height: f32,
    paragraph: &Paragraph,
    line_top: f32,
    glyphs: &mut Vec<TextGlyph>,
    lines: &mut Vec<TextLine>,
) -> f32 {
    let glyph_start = glyphs.len() as u32;
    let baseline = line_top + ascender;
    let mut pen = 0.0;
    let mut previous: Option<u32> = None;

    for (character, byte, length) in &paragraph.chars {
        let (glyph_id, missing) = resolve_glyph(font, *character);
        let kern = previous.map_or(0.0, |left| font.kerning(left, glyph_id) * unit);
        let advance = glyph_advance_ems(font, glyph_id, unit);

        glyphs.push(TextGlyph {
            glyph_id,
            position: [pen + kern, baseline],
            advance,
            source_range: [*byte, *byte + *length],
            missing,
        });
        pen += kern + advance;
        previous = Some(glyph_id);
    }

    let content_end = paragraph
        .chars
        .last()
        .map_or(paragraph.start_byte, |(_, byte, length)| *byte + *length);

    lines.push(TextLine {
        glyph_range: [glyph_start, glyphs.len() as u32],
        top: line_top,
        baseline,
        height: line_height,
        width: pen,
        source_range: [paragraph.start_byte, content_end],
        terminated: paragraph.terminated,
    });

    line_top + line_height
}

/// Emit one paragraph as greedy wrapped lines. Returns the next line top.
///
/// `text` is the full measured revision: hard wrap breaks consult its
/// grapheme boundaries so multi-scalar clusters never split across lines.
#[allow(clippy::too_many_arguments)]
fn push_wrapped_lines(
    font: &FontAsset,
    text: &str,
    unit: f32,
    ascender: f32,
    line_height: f32,
    limit: f32,
    paragraph: &Paragraph,
    mut line_top: f32,
    glyphs: &mut Vec<TextGlyph>,
    lines: &mut Vec<TextLine>,
) -> f32 {
    let chars = &paragraph.chars;
    let mut index = 0_usize;
    let mut soft_wrap = false;

    loop {
        if soft_wrap {
            while chars
                .get(index)
                .is_some_and(|(character, _, _)| *character == ' ')
            {
                index += 1;
            }

            if index >= chars.len() {
                break;
            }
        }

        // Empty paragraphs still emit their one caret line here; exhausted
        // soft wraps break above without emitting trailing-space lines.
        let line_start_byte = chars
            .get(index)
            .map_or(paragraph.start_byte, |(_, byte, _)| *byte);

        let glyph_start = glyphs.len() as u32;
        let baseline = line_top + ascender;
        let mut pen = 0.0;
        let mut previous: Option<u32> = None;
        let mut emitted = 0_u32;
        let mut last_space_out: Option<u32> = None;
        let mut last_space_index = 0_usize;
        let mut next_index: Option<usize> = None;

        while index < chars.len() {
            let (character, byte, length) = chars[index];
            let (glyph_id, missing) = resolve_glyph(font, character);
            let kern = previous.map_or(0.0, |left| font.kerning(left, glyph_id) * unit);
            let advance = glyph_advance_ems(font, glyph_id, unit);

            if pen + kern + advance > limit && emitted > 0 {
                if character == ' ' {
                    next_index = Some(index + 1);
                    break;
                }

                if let Some(out) = last_space_out {
                    glyphs.truncate(out as usize);
                    next_index = Some(last_space_index + 1);
                    break;
                }

                if may_hard_break_before(text, byte, character) {
                    next_index = Some(index);
                    break;
                }
                // Otherwise keep the grapheme with its base past the limit
                // rather than splitting it across lines.
            }

            glyphs.push(TextGlyph {
                glyph_id,
                position: [pen + kern, baseline],
                advance,
                source_range: [byte, byte + length],
                missing,
            });
            pen += kern + advance;
            previous = Some(glyph_id);
            emitted += 1;

            if character == ' ' {
                last_space_out = Some(glyphs.len() as u32 - 1);
                last_space_index = index;
            }

            index += 1;
        }

        let surviving = &glyphs[glyph_start as usize..];
        let (width, content_end) = surviving.last().map_or((0.0, line_start_byte), |glyph| {
            (glyph.position[0] + glyph.advance, glyph.source_range[1])
        });

        lines.push(TextLine {
            glyph_range: [glyph_start, glyphs.len() as u32],
            top: line_top,
            baseline,
            height: line_height,
            width,
            source_range: [line_start_byte, content_end],
            terminated: false,
        });
        line_top += line_height;

        match next_index {
            Some(next) => {
                index = next;
                soft_wrap = true;
            }
            None => break,
        }
    }

    if let Some(line) = lines.last_mut()
        && paragraph.terminated
    {
        // Only the paragraph-final line carries the explicit break.
        line.terminated = true;
    }

    line_top
}

/// True for the basic-LTR grapheme subset's no-break-before characters:
/// combining marks in the covered blocks, `ZWJ` and variation selectors.
/// Anything else (including deferred scripts and emoji) breaks per scalar.
///
/// Dependency-free subset only; the `gui` build answers the same question
/// through complete UAX #29 boundaries instead.
#[cfg(not(feature = "gui"))]
fn is_extend(character: char) -> bool {
    matches!(
        character,
        '\u{300}'..='\u{36f}'
            | '\u{483}'..='\u{489}'
            | '\u{591}'..='\u{5bd}'
            | '\u{5bf}'
            | '\u{5c1}'..='\u{5c2}'
            | '\u{5c4}'..='\u{5c5}'
            | '\u{5c7}'
            | '\u{610}'..='\u{61a}'
            | '\u{64b}'..='\u{65f}'
            | '\u{670}'
            | '\u{711}'
            | '\u{730}'..='\u{74a}'
            | '\u{1ab0}'..='\u{1aff}'
            | '\u{1dc0}'..='\u{1dff}'
            | '\u{200d}'
            | '\u{20d0}'..='\u{20ff}'
            | '\u{fe00}'..='\u{fe0f}'
            | '\u{fe20}'..='\u{fe2f}'
            | '\u{e0100}'..='\u{e01ef}'
    )
}

/// True for line/paragraph separators that force a grapheme break after them
/// even before an extend character.
///
/// Dependency-free subset only; unused when `gui` enables complete UAX #29
/// boundaries.
#[cfg(not(feature = "gui"))]
fn is_break_mandatory(character: char) -> bool {
    character.is_control() || character == '\u{2028}' || character == '\u{2029}'
}

/// Legal caret offsets for `text`: sorted UTF-8 byte offsets at complete UAX
/// #29 extended grapheme edges ([`SEGMENTATION_SCOPE`]), always including `0`
/// and the text length.
#[cfg(feature = "gui")]
pub fn grapheme_boundaries(text: &str) -> Vec<u32> {
    if text.is_empty() {
        return vec![0];
    }

    let mut boundaries = Vec::with_capacity(text.len() + 1);
    for (byte, _) in text.grapheme_indices(true) {
        boundaries.push(byte as u32);
    }
    boundaries.push(text.len() as u32);

    boundaries
}

/// Legal caret offsets for `text`: sorted UTF-8 byte offsets at extended
/// grapheme edges under the basic-LTR subset ([`SEGMENTATION_SCOPE`]),
/// always including `0` and the text length.
#[cfg(not(feature = "gui"))]
pub fn grapheme_boundaries(text: &str) -> Vec<u32> {
    if text.is_empty() {
        return vec![0];
    }

    let mut boundaries = Vec::with_capacity(text.len() + 1);
    boundaries.push(0);

    let mut previous: Option<char> = None;

    for (byte, character) in text.char_indices() {
        let byte = byte as u32;

        if byte != 0 {
            let breaks = match previous {
                // `CRLF` is one grapheme (GB3).
                Some('\r') if character == '\n' => false,
                // Break after controls (GB4/GB5) wins over extension.
                Some(before) if is_break_mandatory(before) => true,
                // Otherwise no break before extends/`ZWJ` (GB9/GB11 subset).
                _ if is_extend(character) => false,
                _ => true,
            };

            if breaks {
                boundaries.push(byte);
            }
        }

        previous = Some(character);
    }

    boundaries.push(text.len() as u32);

    boundaries
}

/// True when `offset` is a legal caret position in `text`: a UTF-8 scalar
/// boundary that does not split a complete UAX #29 extended grapheme.
#[cfg(feature = "gui")]
pub fn is_grapheme_boundary(text: &str, offset: u32) -> bool {
    let offset = offset as usize;

    if offset > text.len() || !text.is_char_boundary(offset) {
        return false;
    }

    if offset == 0 || offset == text.len() {
        return true;
    }

    text.grapheme_indices(true).any(|(byte, _)| byte == offset)
}

/// Whether a wrapping hard break may split before the scalar starting at
/// `byte`. This is the full UAX #29 boundary in the same text revision, so
/// flags, modifier sequences, Hangul syllables, `ZWJ` sequences and conjuncts
/// never split across lines.
#[cfg(feature = "gui")]
fn may_hard_break_before(text: &str, byte: u32, _character: char) -> bool {
    is_grapheme_boundary(text, byte)
}

/// Whether a wrapping hard break may split before `character` under the
/// basic-LTR subset: never before a covered combining mark.
#[cfg(not(feature = "gui"))]
fn may_hard_break_before(_text: &str, _byte: u32, character: char) -> bool {
    !is_extend(character)
}

/// True when `offset` is a legal caret position in `text`: a UTF-8 scalar
/// boundary that does not split a grapheme under the basic-LTR subset.
#[cfg(not(feature = "gui"))]
pub fn is_grapheme_boundary(text: &str, offset: u32) -> bool {
    let offset = offset as usize;

    if offset > text.len() || !text.is_char_boundary(offset) {
        return false;
    }

    let (before, after) = text.split_at(offset);
    let previous = before.chars().next_back();
    let next = after.chars().next();

    match (previous, next) {
        (None, _) | (_, None) => true,
        (Some('\r'), Some('\n')) => false,
        (Some(before), _) if is_break_mandatory(before) => true,
        (_, Some(next)) if is_extend(next) => false,
        _ => true,
    }
}

/// Convert a browser UTF-16 code-unit offset into a runtime UTF-8 byte offset
/// for the same text revision. Returns `None` when the offset splits a
/// surrogate pair or lies past the text.
pub fn utf16_to_utf8_offset(text: &str, utf16_offset: usize) -> Option<u32> {
    let mut utf16 = 0_usize;

    for (byte, character) in text.char_indices() {
        if utf16 == utf16_offset {
            return Some(byte as u32);
        }

        utf16 += character.len_utf16();
    }

    if utf16 == utf16_offset {
        return Some(text.len() as u32);
    }

    None
}

/// Convert a runtime UTF-8 byte offset into a browser UTF-16 code-unit offset
/// for the same text revision. Returns `None` when the offset splits a
/// scalar.
pub fn utf8_to_utf16_offset(text: &str, utf8_offset: u32) -> Option<usize> {
    let utf8_offset = utf8_offset as usize;

    if utf8_offset > text.len() || !text.is_char_boundary(utf8_offset) {
        return None;
    }

    let mut utf16 = 0_usize;

    for (byte, character) in text.char_indices() {
        if byte == utf8_offset {
            return Some(utf16);
        }

        utf16 += character.len_utf16();
    }

    Some(utf16)
}
