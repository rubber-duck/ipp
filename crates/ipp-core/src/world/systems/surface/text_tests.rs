//! Focused tests for shared headless text measurement. Every numeric
//! expectation below is hand-computed from the documented em SCALE (see
//! `test_font`) and the recorded wrap/caret policies, never copied from
//! implementation output.

use super::*;
use crate::services::asset_management::font::FontAsset;

const FONT_KEY: AssetKey = AssetKey {
    slot: 7,
    generation: 3,
};

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_f32(bytes: &mut Vec<u8>, value: f32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

/// Hand-built IPPF font: 1000 units per em, ascender 800, descender -200,
/// line gap 200, so one em line spans 1.2 with baseline at 0.8.
/// Glyph advances in ems: `.notdef` 0.5, `A` 0.6, `V` 0.65, `a` 0.55,
/// space 0.3, `e` 0.55. Single kerning pair `(A, V) = -0.05`.
fn test_font() -> FontAsset {
    let mut bytes = b"IPPF".to_vec();
    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, 1000);
    push_f32(&mut bytes, 800.0);
    push_f32(&mut bytes, -200.0);
    push_f32(&mut bytes, 200.0);

    let advances = [500.0_f32, 600.0, 650.0, 550.0, 300.0, 550.0];
    push_u32(&mut bytes, advances.len() as u32);
    push_u32(&mut bytes, 5);
    push_u32(&mut bytes, 1);

    for advance in advances {
        push_f32(&mut bytes, advance);
        push_f32(&mut bytes, 0.0);
        for bound in [0.0_f32, 0.0, 0.0, 0.0] {
            push_f32(&mut bytes, bound);
        }
        push_u32(&mut bytes, 0);
    }

    for (codepoint, glyph_id) in [
        (0x20_u32, 4_u32),
        (0x41, 1),
        (0x56, 2),
        (0x61, 3),
        (0x65, 5),
    ] {
        push_u32(&mut bytes, codepoint);
        push_u32(&mut bytes, glyph_id);
    }

    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, 2);
    push_f32(&mut bytes, -50.0);

    FontAsset::decode(&bytes).unwrap()
}

fn ready(font: &FontAsset) -> TextFont<'_> {
    TextFont::Ready {
        key: FONT_KEY,
        font,
    }
}

fn request<'a>(
    text: &'a str,
    font: TextFont<'a>,
    line_policy: TextLinePolicy,
    max_width: TextMaxWidth,
) -> TextMeasureRequest<'a> {
    TextMeasureRequest::new(text, font, 0.1, line_policy, max_width).unwrap()
}

fn measured(request: &TextMeasureRequest<'_>) -> TextLayout {
    match measure_text(request) {
        TextOutcome::Measured(layout) => layout,
        TextOutcome::PendingFont {
            key,
        } => panic!("expected measured layout, font {key:?} pending"),
    }
}

fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 1e-5,
        "expected {expected}, got {actual}"
    );
}

fn assert_pair(actual: [f32; 2], expected: [f32; 2]) {
    assert_close(actual[0], expected[0]);
    assert_close(actual[1], expected[1]);
}

#[test]
fn kerning_advances_and_line_metrics_come_from_original_ids() {
    let font = test_font();
    let layout = measured(&request(
        "AV",
        ready(&font),
        TextLinePolicy::Multiline,
        TextMaxWidth::Unbounded,
    ));

    assert_eq!(layout.units(), TextUnits::Ems);
    assert_close(layout.font_size, 0.1);
    assert_eq!(layout.glyphs.len(), 2);

    // Both glyph origins sit on the font's 0.8 em alphabetic baseline; V is
    // shifted left by the (A, V) pair adjustment.
    assert_eq!(layout.glyphs[0].glyph_id, 1);
    assert_pair(layout.glyphs[0].position, [0.0, 0.8]);
    assert_close(layout.glyphs[0].advance, 0.6);
    assert_eq!(layout.glyphs[0].source_range, [0, 1]);
    assert!(!layout.glyphs[0].missing);

    assert_eq!(layout.glyphs[1].glyph_id, 2);
    assert_pair(layout.glyphs[1].position, [0.55, 0.8]);
    assert_close(layout.glyphs[1].advance, 0.65);
    assert_eq!(layout.glyphs[1].source_range, [1, 2]);

    assert_eq!(layout.lines.len(), 1);
    let line = layout.lines[0];
    assert_eq!(line.glyph_range, [0, 2]);
    assert_close(line.top, 0.0);
    assert_close(line.baseline, 0.8);
    assert_close(line.height, 1.2);
    assert_close(line.width, 1.2);
    assert_eq!(line.source_range, [0, 2]);
    assert!(!line.terminated);

    assert_pair(layout.size, [1.2, 1.2]);
    assert_eq!(layout.grapheme_boundaries, vec![0, 1, 2]);
    assert_eq!(layout.missing_glyph_count(), 0);
}

#[test]
fn explicit_newline_splits_lines_down_positive() {
    let font = test_font();
    let layout = measured(&request(
        "A\nV",
        ready(&font),
        TextLinePolicy::Multiline,
        TextMaxWidth::Unbounded,
    ));

    assert_eq!(layout.lines.len(), 2);
    assert!(layout.lines[1].top > layout.lines[0].top);

    assert_pair(layout.glyphs[0].position, [0.0, 0.8]);
    assert_pair(layout.glyphs[1].position, [0.0, 2.0]);

    assert_eq!(layout.lines[0].source_range, [0, 1]);
    assert!(layout.lines[0].terminated);
    assert_close(layout.lines[1].baseline, 2.0);
    assert_eq!(layout.lines[1].source_range, [2, 3]);
    assert!(!layout.lines[1].terminated);

    assert_pair(layout.size, [0.65, 2.4]);
    assert_eq!(layout.grapheme_boundaries, vec![0, 1, 2, 3]);

    let caret = layout.caret_position(1).unwrap();
    assert_eq!(caret.line, 0);
    assert_pair(caret.position, [0.6, 0.0]);
    assert_close(caret.height, 1.2);

    let caret = layout.caret_position(2).unwrap();
    assert_eq!(caret.line, 1);
    assert_pair(caret.position, [0.0, 1.2]);

    let caret = layout.caret_position(3).unwrap();
    assert_pair(caret.position, [0.65, 1.2]);
}

#[test]
fn crlf_is_one_break_and_lone_cr_breaks() {
    let font = test_font();

    let layout = measured(&request(
        "A\r\nV",
        ready(&font),
        TextLinePolicy::Multiline,
        TextMaxWidth::Unbounded,
    ));
    assert_eq!(layout.lines.len(), 2);
    assert_eq!(layout.lines[0].source_range, [0, 1]);
    assert_eq!(layout.lines[1].source_range, [3, 4]);
    assert_pair(layout.glyphs[1].position, [0.0, 2.0]);
    // No caret stop between CR and LF.
    assert_eq!(layout.grapheme_boundaries, vec![0, 1, 3, 4]);
    assert!(!is_grapheme_boundary("A\r\nV", 2));

    let layout = measured(&request(
        "A\rV",
        ready(&font),
        TextLinePolicy::Multiline,
        TextMaxWidth::Unbounded,
    ));
    assert_eq!(layout.lines.len(), 2);
    assert_eq!(layout.lines[1].source_range, [2, 3]);
}

#[test]
fn finite_width_wraps_at_spaces_and_consumes_them() {
    let font = test_font();
    let layout = measured(&request(
        "A V",
        ready(&font),
        TextLinePolicy::Multiline,
        TextMaxWidth::Ems(1.0),
    ));

    // A (0.6) plus space (0.3) fit; V (ending 1.55) does not.
    assert_eq!(layout.glyphs.len(), 2);
    assert_eq!(layout.lines.len(), 2);
    assert_eq!(layout.lines[0].glyph_range, [0, 1]);
    assert_close(layout.lines[0].width, 0.6);
    assert_eq!(layout.lines[0].source_range, [0, 1]);

    assert_eq!(layout.lines[1].glyph_range, [1, 2]);
    assert_pair(layout.glyphs[1].position, [0.0, 2.0]);
    assert_eq!(layout.lines[1].source_range, [2, 3]);

    // The consumed space keeps its grapheme stop and maps to next-line start.
    let caret = layout.caret_position(2).unwrap();
    assert_eq!(caret.line, 1);
    assert_pair(caret.position, [0.0, 1.2]);

    let caret = layout.caret_position(1).unwrap();
    assert_eq!(caret.line, 0);
    assert_pair(caret.position, [0.6, 0.0]);
}

#[test]
fn narrow_words_hard_break_between_scalars() {
    let font = test_font();
    let layout = measured(&request(
        "AAAA",
        ready(&font),
        TextLinePolicy::Multiline,
        TextMaxWidth::Ems(1.0),
    ));

    // Each 0.6 advance exceeds the remaining 0.4, so one glyph per line.
    assert_eq!(layout.lines.len(), 4);

    for (index, line) in layout.lines.iter().enumerate() {
        assert_eq!(line.glyph_range, [index as u32, index as u32 + 1]);
        assert_close(line.top, index as f32 * 1.2);
        assert_close(line.width, 0.6);
        assert_eq!(line.source_range, [index as u32, index as u32 + 1]);
        assert!(!line.terminated);
    }

    assert_pair(layout.size, [0.6, 4.8]);

    // Shared hard-break offsets prefer the next line start.
    let caret = layout.caret_position(1).unwrap();
    assert_eq!(caret.line, 1);
    assert_pair(caret.position, [0.0, 1.2]);

    let rects = layout.selection_rects(0, 4);
    assert_eq!(rects.len(), 4);
    assert_close(rects[2][0], 0.0);
    assert_close(rects[2][1], 2.4);
    assert_close(rects[2][2], 0.6);
    assert_close(rects[2][3], 1.2);
}

#[test]
fn single_line_folds_breaks_to_spaces_without_wrapping() {
    let font = test_font();
    let layout = measured(&request(
        "A\nV",
        ready(&font),
        TextLinePolicy::SingleLine,
        TextMaxWidth::Ems(1.0),
    ));

    assert_eq!(layout.lines.len(), 1);
    assert_eq!(layout.glyphs.len(), 3);
    // Folded break keeps the newline's source byte as a space.
    assert_eq!(layout.glyphs[1].glyph_id, 4);
    assert_eq!(layout.glyphs[1].source_range, [1, 2]);
    assert_pair(layout.glyphs[2].position, [0.9, 0.8]);
    assert_close(layout.lines[0].width, 1.55);
    assert!(!layout.lines[0].terminated);
}

#[test]
fn missing_scalars_keep_ranges_with_notdef_fallback() {
    let font = test_font();
    // U+03A9 is two bytes and absent from the test cmap.
    let layout = measured(&request(
        "A\u{3a9}",
        ready(&font),
        TextLinePolicy::Multiline,
        TextMaxWidth::Unbounded,
    ));

    assert_eq!(layout.glyphs.len(), 2);
    assert_eq!(layout.glyphs[1].glyph_id, 0);
    assert!(layout.glyphs[1].missing);
    assert_eq!(layout.glyphs[1].source_range, [1, 3]);
    assert_close(layout.glyphs[1].position[0], 0.6);
    assert_close(layout.glyphs[1].advance, 0.5);
    assert_eq!(layout.missing_glyph_count(), 1);

    assert_eq!(layout.grapheme_boundaries, vec![0, 1, 3]);
    assert!(!layout.is_caret_position(2));
    assert!(layout.caret_position(2).is_none());
}

#[test]
fn combining_marks_share_a_grapheme_and_survive_wrapping() {
    let font = test_font();
    // e plus U+0301 (two bytes): one grapheme, two scalars, so no caret
    // stop exists between the base and its mark.
    let text = "e\u{301}";

    assert_eq!(grapheme_boundaries(text), vec![0, 3]);
    assert!(!is_grapheme_boundary(text, 1));
    assert!(!is_grapheme_boundary(text, 2));

    let layout = measured(&request(
        text,
        ready(&font),
        TextLinePolicy::Multiline,
        TextMaxWidth::Unbounded,
    ));
    assert_eq!(layout.grapheme_boundaries, vec![0, 3]);
    assert!(layout.caret_position(1).is_none());
    assert!(layout.caret_position(2).is_none());

    let caret = layout.caret_position(3).unwrap();
    assert_pair(caret.position, [1.05, 0.0]);

    // The mark overflows a 0.6 line alone, so it stays with its base.
    let wrapped = measured(&request(
        "e\u{301}e",
        ready(&font),
        TextLinePolicy::Multiline,
        TextMaxWidth::Ems(0.6),
    ));
    assert_eq!(wrapped.lines.len(), 2);
    assert_eq!(wrapped.lines[0].glyph_range, [0, 2]);
    assert_eq!(wrapped.lines[1].glyph_range, [2, 3]);
    assert_eq!(wrapped.lines[1].source_range, [3, 4]);
}

#[cfg(feature = "gui")]
#[test]
fn regional_indicator_pairs_form_flag_graphemes() {
    // U+1F1EB U+1F1F7 (FR): 4 bytes each, one grapheme (GB12/GB13).
    let flag = "\u{1f1eb}\u{1f1f7}";
    assert_eq!(grapheme_boundaries(flag), vec![0, 8]);
    assert!(!is_grapheme_boundary(flag, 4));

    let two = "\u{1f1eb}\u{1f1f7}\u{1f1e9}\u{1f1ea}";
    assert_eq!(grapheme_boundaries(two), vec![0, 8, 16]);

    // An odd trailing indicator stands alone.
    let odd = "\u{1f1eb}\u{1f1f7}\u{1f1eb}";
    assert_eq!(grapheme_boundaries(odd), vec![0, 8, 12]);
    assert!(is_grapheme_boundary(odd, 8));
    assert!(!is_grapheme_boundary(odd, 4));
}

#[cfg(feature = "gui")]
#[test]
fn emoji_modifier_sequences_stay_together() {
    // Thumbs up plus medium skin tone: one grapheme (the modifier is Extend).
    let text = "\u{1f44d}\u{1f3fd}";
    assert_eq!(grapheme_boundaries(text), vec![0, 8]);
    assert!(!is_grapheme_boundary(text, 4));
}

#[cfg(feature = "gui")]
#[test]
fn hangul_jamo_and_syllables_cluster() {
    // Precomposed LV syllable: one scalar, one grapheme.
    assert_eq!(grapheme_boundaries("\u{d55c}"), vec![0, 3]);
    // Leading plus vowel jamo (GB6), an LV syllable plus trailing jamo
    // (GB8), and a full L plus V plus T jamo chain stay together.
    assert_eq!(grapheme_boundaries("\u{1100}\u{1161}"), vec![0, 6]);
    assert_eq!(grapheme_boundaries("\u{ac00}\u{11a8}"), vec![0, 6]);
    let chain = "\u{1100}\u{1161}\u{11a8}";
    assert_eq!(grapheme_boundaries(chain), vec![0, 9]);
    assert!(!is_grapheme_boundary(chain, 3));
    assert!(!is_grapheme_boundary(chain, 6));
}

#[cfg(feature = "gui")]
#[test]
fn zwj_emoji_sequences_form_one_grapheme() {
    // Man plus ZWJ plus woman (GB11): 4 plus 3 plus 4 bytes, one grapheme.
    let text = "\u{1f468}\u{200d}\u{1f469}";
    assert_eq!(grapheme_boundaries(text), vec![0, 11]);
    assert!(!is_grapheme_boundary(text, 4));
    assert!(!is_grapheme_boundary(text, 7));
}

#[cfg(feature = "gui")]
#[test]
fn indic_virama_stays_with_its_consonant() {
    // Ka plus virama plus ssa: the virama (Extend) never splits from its
    // consonant (GB9), so byte 3 is not a boundary. Every reported boundary
    // is an ordered scalar boundary from 0 to the text length.
    let text = "\u{915}\u{94d}\u{937}";
    assert!(!is_grapheme_boundary(text, 3));
    assert!(is_grapheme_boundary(text, 0));
    assert!(is_grapheme_boundary(text, text.len() as u32));
    let boundaries = grapheme_boundaries(text);
    assert_eq!(boundaries.first(), Some(&0));
    assert_eq!(boundaries.last(), Some(&(text.len() as u32)));
    for window in boundaries.windows(2) {
        assert!(window[0] < window[1]);
        assert!(text.is_char_boundary(window[1] as usize));
    }
}

#[cfg(feature = "gui")]
#[test]
fn caret_selection_and_deletion_follow_full_graphemes() {
    let font = test_font();
    // A (1 byte) plus flag (8 bytes): caret stops at 0, 1 and 9 only.
    let text = "A\u{1f1eb}\u{1f1f7}";
    assert_eq!(grapheme_boundaries(text), vec![0, 1, 9]);

    let layout = measured(&request(
        text,
        ready(&font),
        TextLinePolicy::Multiline,
        TextMaxWidth::Unbounded,
    ));
    assert_eq!(layout.grapheme_boundaries, vec![0, 1, 9]);
    assert!(layout.caret_position(4).is_none());
    assert!(layout.caret_position(5).is_none());
    let end = layout.caret_position(9).unwrap();
    // A advances 0.6; each missing flag scalar falls back to .notdef (0.5).
    assert_close(end.position[0], 0.6 + 0.5 + 0.5);

    // Selection over the flag covers both scalars as one rectangle.
    let rects = layout.selection_rects(1, 9);
    assert_eq!(rects.len(), 1);
    assert_close(rects[0][0], 0.6);
    assert_close(rects[0][2], 1.0);

    // Deletion removes whole graphemes.
    let bounds = grapheme_boundaries(text);
    let backspaced = bounds.iter().copied().filter(|b| *b < 9).max().unwrap();
    assert_eq!(backspaced, 1);
    assert_eq!(&text[..backspaced as usize], "A");
    let forward = bounds.iter().copied().find(|b| *b > 1).unwrap();
    assert_eq!(forward, 9);
    assert_eq!(&text[forward as usize..], "");
}

#[cfg(feature = "gui")]
#[test]
fn deletion_removes_modifier_and_hangul_graphemes_whole() {
    let text = "\u{1f44d}\u{1f3fd}\u{d55c}";
    assert_eq!(grapheme_boundaries(text), vec![0, 8, 11]);
    let bounds = grapheme_boundaries(text);
    let prev = bounds.iter().copied().filter(|b| *b < 11).max().unwrap();
    assert_eq!(prev, 8);
    assert_eq!(&text[..prev as usize], "\u{1f44d}\u{1f3fd}");
    let prev = bounds.iter().copied().filter(|b| *b < 8).max().unwrap();
    assert_eq!(prev, 0);
}

#[cfg(feature = "gui")]
#[test]
fn wrapping_keeps_flag_graphemes_on_one_line() {
    let font = test_font();
    // A (0.6) fits a 0.7 line; the flag (two 0.5 .notdef advances) must not
    // split its regional indicators across lines.
    let text = "A\u{1f1eb}\u{1f1f7}";
    let layout = measured(&request(
        text,
        ready(&font),
        TextLinePolicy::Multiline,
        TextMaxWidth::Ems(0.7),
    ));
    assert_eq!(layout.lines.len(), 2);
    assert_eq!(layout.lines[0].source_range, [0, 1]);
    assert_eq!(layout.lines[1].source_range, [1, 9]);
    assert_eq!(layout.lines[1].glyph_range, [1, 3]);
}

#[test]
fn empty_and_trailing_break_text_keep_caret_lines() {
    let font = test_font();

    let layout = measured(&request(
        "",
        ready(&font),
        TextLinePolicy::Multiline,
        TextMaxWidth::Unbounded,
    ));
    assert_eq!(layout.lines.len(), 1);
    assert!(layout.glyphs.is_empty());
    assert_eq!(layout.lines[0].glyph_range, [0, 0]);
    assert_eq!(layout.lines[0].source_range, [0, 0]);
    assert_close(layout.lines[0].baseline, 0.8);
    assert_pair(layout.size, [0.0, 1.2]);
    assert_eq!(layout.grapheme_boundaries, vec![0]);

    let caret = layout.caret_position(0).unwrap();
    assert_eq!(caret.line, 0);
    assert_pair(caret.position, [0.0, 0.0]);

    let layout = measured(&request(
        "A\n",
        ready(&font),
        TextLinePolicy::Multiline,
        TextMaxWidth::Unbounded,
    ));
    assert_eq!(layout.lines.len(), 2);
    assert!(layout.lines[0].terminated);
    assert_eq!(layout.lines[1].source_range, [2, 2]);
    assert_pair(layout.size, [0.6, 2.4]);

    let caret = layout.caret_position(2).unwrap();
    assert_eq!(caret.line, 1);
    assert_pair(caret.position, [0.0, 1.2]);
}

#[test]
fn pending_fonts_report_unavailability_without_metrics() {
    let pending = TextFont::Pending {
        key: FONT_KEY,
    };
    let request = TextMeasureRequest::new(
        "AV",
        pending,
        0.1,
        TextLinePolicy::Multiline,
        TextMaxWidth::Unbounded,
    )
    .unwrap();

    match measure_text(&request) {
        TextOutcome::PendingFont {
            key,
        } => assert_eq!(key, FONT_KEY),
        TextOutcome::Measured(_) => panic!("pending font must not synthesize metrics"),
    }
}

#[test]
fn request_validation_rejects_nonpositive_scales() {
    let font = test_font();

    assert!(matches!(
        TextMeasureRequest::new(
            "A",
            ready(&font),
            0.0,
            TextLinePolicy::Multiline,
            TextMaxWidth::Unbounded,
        ),
        Err(TextRequestError::FontSize(value)) if value == 0.0
    ));
    assert!(matches!(
        TextMeasureRequest::new(
            "A",
            ready(&font),
            f32::NAN,
            TextLinePolicy::Multiline,
            TextMaxWidth::Unbounded,
        ),
        Err(TextRequestError::FontSize(value)) if value.is_nan()
    ));
    assert!(matches!(
        TextMeasureRequest::new(
            "A",
            ready(&font),
            0.1,
            TextLinePolicy::Multiline,
            TextMaxWidth::Ems(-1.0),
        ),
        Err(TextRequestError::MaxWidth(value)) if value == -1.0
    ));
}

#[test]
fn cache_keys_cover_inputs_and_ignore_paint_state() {
    let font = test_font();
    let base = request(
        "AV",
        ready(&font),
        TextLinePolicy::Multiline,
        TextMaxWidth::Unbounded,
    )
    .cache_key();

    // Identical inputs share a key: camera or colour changes reuse layout.
    let same = request(
        "AV",
        ready(&font),
        TextLinePolicy::Multiline,
        TextMaxWidth::Unbounded,
    )
    .cache_key();
    assert_eq!(base, same);

    let changed_text = request(
        "AV ",
        ready(&font),
        TextLinePolicy::Multiline,
        TextMaxWidth::Unbounded,
    )
    .cache_key();
    assert_ne!(base, changed_text);

    let replaced_font = request(
        "AV",
        TextFont::Ready {
            key: AssetKey {
                slot: 7,
                generation: 4,
            },
            font: &font,
        },
        TextLinePolicy::Multiline,
        TextMaxWidth::Unbounded,
    )
    .cache_key();
    assert_ne!(base, replaced_font);

    let changed_policy = request(
        "AV",
        ready(&font),
        TextLinePolicy::SingleLine,
        TextMaxWidth::Unbounded,
    )
    .cache_key();
    assert_ne!(base, changed_policy);

    let changed_width = request(
        "AV",
        ready(&font),
        TextLinePolicy::Multiline,
        TextMaxWidth::Ems(2.0),
    )
    .cache_key();
    assert_ne!(base, changed_width);

    let changed_size = TextMeasureRequest::new(
        "AV",
        ready(&font),
        0.2,
        TextLinePolicy::Multiline,
        TextMaxWidth::Unbounded,
    )
    .unwrap()
    .cache_key();
    assert_ne!(base, changed_size);
}

#[test]
fn selections_map_to_per_line_rects() {
    let font = test_font();
    let layout = measured(&request(
        "A\nV",
        ready(&font),
        TextLinePolicy::Multiline,
        TextMaxWidth::Unbounded,
    ));

    let rects = layout.selection_rects(0, 3);
    assert_eq!(rects.len(), 2);
    assert_close(rects[0][0], 0.0);
    assert_close(rects[0][1], 0.0);
    assert_close(rects[0][2], 0.6);
    assert_close(rects[0][3], 1.2);
    assert_close(rects[1][0], 0.0);
    assert_close(rects[1][1], 1.2);
    assert_close(rects[1][2], 0.65);
    assert_close(rects[1][3], 1.2);

    // Reversed endpoints normalise; empty and illegal ranges are empty.
    assert_eq!(layout.selection_rects(3, 0), rects);
    assert!(layout.selection_rects(1, 1).is_empty());
    assert!(layout.selection_rects(0, 9).is_empty());

    // A range ending at the next line start covers only the first line.
    let head = layout.selection_rects(0, 2);
    assert_eq!(head.len(), 1);
    assert_close(head[0][2], 0.6);
}

#[test]
fn partial_line_selection_clips_to_character_edges() {
    let font = test_font();
    let layout = measured(&request(
        "AV",
        ready(&font),
        TextLinePolicy::Multiline,
        TextMaxWidth::Unbounded,
    ));

    // V starts at pen 0.55; selecting it alone clips to its edges.
    let rects = layout.selection_rects(1, 2);
    assert_eq!(rects.len(), 1);
    assert_close(rects[0][0], 0.55);
    assert_close(rects[0][2], 0.65);
}

#[test]
fn utf16_and_utf8_offsets_convert_per_revision() {
    // A (1 byte, 1 unit), U+00E9 (2 bytes, 1 unit), U+1F4A1 (4 bytes, 2 units).
    let text = "A\u{e9}\u{1f4a1}";

    assert_eq!(utf16_to_utf8_offset(text, 0), Some(0));
    assert_eq!(utf16_to_utf8_offset(text, 1), Some(1));
    assert_eq!(utf16_to_utf8_offset(text, 2), Some(3));
    assert_eq!(utf16_to_utf8_offset(text, 3), None);
    assert_eq!(utf16_to_utf8_offset(text, 4), Some(7));
    assert_eq!(utf16_to_utf8_offset(text, 5), None);

    assert_eq!(utf8_to_utf16_offset(text, 0), Some(0));
    assert_eq!(utf8_to_utf16_offset(text, 1), Some(1));
    assert_eq!(utf8_to_utf16_offset(text, 2), None);
    assert_eq!(utf8_to_utf16_offset(text, 3), Some(2));
    assert_eq!(utf8_to_utf16_offset(text, 4), None);
    assert_eq!(utf8_to_utf16_offset(text, 7), Some(4));
    assert_eq!(utf8_to_utf16_offset(text, 8), None);
}

#[test]
fn utf16_and_utf8_offsets_convert_flag_sequences() {
    // Each regional indicator is 4 bytes and 2 UTF-16 units; a lone unit
    // never converts.
    let text = "\u{1f1eb}\u{1f1f7}";

    assert_eq!(utf16_to_utf8_offset(text, 0), Some(0));
    assert_eq!(utf16_to_utf8_offset(text, 1), None);
    assert_eq!(utf16_to_utf8_offset(text, 2), Some(4));
    assert_eq!(utf16_to_utf8_offset(text, 4), Some(8));
    assert_eq!(utf16_to_utf8_offset(text, 5), None);

    assert_eq!(utf8_to_utf16_offset(text, 0), Some(0));
    assert_eq!(utf8_to_utf16_offset(text, 2), None);
    assert_eq!(utf8_to_utf16_offset(text, 4), Some(2));
    assert_eq!(utf8_to_utf16_offset(text, 8), Some(4));
}

#[test]
fn unicode_version_and_scope_are_recorded() {
    #[cfg(feature = "gui")]
    {
        assert_eq!(UNICODE_VERSION, unicode_segmentation::UNICODE_VERSION);
        assert_eq!(SEGMENTATION_SCOPE, "uax29-extended");
    }
    #[cfg(not(feature = "gui"))]
    {
        assert_eq!(UNICODE_VERSION, (16, 0, 0));
        assert_eq!(SEGMENTATION_SCOPE, "basic-ltr-subset");
    }
}
