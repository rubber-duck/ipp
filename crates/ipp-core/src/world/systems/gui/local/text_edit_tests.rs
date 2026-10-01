use super::*;
use crate::services::asset_management::AssetKey;
use crate::services::asset_management::font::FontAsset;
use crate::systems::surface::{
    TextFont, TextLinePolicy, TextMaxWidth, TextMeasureRequest, TextOutcome, measure_text,
};
use crate::world::systems::gui::test_support::test_font;
use std::time::Instant;

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
    assert_eq!(&*outcome.text, "");
    assert_eq!(outcome.caret, 0);
    assert!(backspace("", 0, None).is_none());
}

#[test]
fn insert_snaps_caret_in_new_text() {
    let outcome = insert_at("ae", 1, 1, "V");
    assert_eq!(&*outcome.text, "aVe");
    assert_eq!(outcome.caret, 2);
    let replaced = insert_at("aVe", 0, 1, "e");
    assert_eq!(&*replaced.text, "eVe");
    assert_eq!(replaced.caret, 1);
}

fn layout(
    font: &FontAsset,
    text: &str,
    line_policy: TextLinePolicy,
    max_width: TextMaxWidth,
) -> TextLayout {
    let font = TextFont::Ready {
        key: AssetKey {
            slot: 1,
            generation: 1,
        },
        font,
    };
    let request = TextMeasureRequest::new(text, font, 0.1, line_policy, max_width).unwrap();
    match measure_text(&request) {
        TextOutcome::Measured(layout) => layout,
        TextOutcome::PendingFont {
            ..
        } => panic!("the fixture font is ready"),
    }
}

/// Independent mapping through per-boundary caret geometry: the nearest
/// caret pen position with ties to the later offset.
fn caret_offset_through_caret_positions(layout: &TextLayout, x_ems: f32) -> u32 {
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

#[test]
fn caret_mapping_matches_per_boundary_caret_geometry() {
    let font = test_font();
    let texts = [
        "",
        "a",
        "aVe",
        "AVAV aa ee",
        "a\u{301}e\u{301}V",
        "a\nV\r\ne\rA",
        "aa\n\n\nee",
        "e\u{1F600}aA \u{1F1FA}\u{1F1F8}V",
        "aa ee AV aVe AAAA VVVV eeee aaaa",
        "  leading and trailing  ",
    ];
    let shapes = [
        (TextLinePolicy::SingleLine, TextMaxWidth::Unbounded),
        (TextLinePolicy::Multiline, TextMaxWidth::Unbounded),
        (TextLinePolicy::Multiline, TextMaxWidth::Ems(1.0)),
        (TextLinePolicy::Multiline, TextMaxWidth::Ems(2.5)),
    ];

    for text in texts {
        for (policy, width) in shapes {
            let layout = layout(&font, text, policy, width);

            // Probe every caret pen, the midpoints between them, and points
            // outside the text, so ties and gaps take both paths.
            let mut probes = vec![-10.0, -0.0, 0.0, f32::NAN, 1.0e9, layout.size[0] + 1.0];
            for boundary in layout.grapheme_boundaries.iter().copied() {
                if let Some(caret) = layout.caret_position(boundary) {
                    probes.push(caret.position[0]);
                    probes.push(caret.position[0] + 0.137);
                    probes.push(caret.position[0] - 0.275);
                }
            }
            let mut step = -1.0;
            while step < layout.size[0] + 1.0 {
                probes.push(step);
                step += 0.05;
            }

            for x in probes {
                assert_eq!(
                    caret_offset_at_x(&layout, x),
                    caret_offset_through_caret_positions(&layout, x),
                    "text {text:?} policy {policy:?} width {width:?} x {x}"
                );
            }
        }
    }
}

#[test]
fn maintained_scaling_caret_mapping_is_linear_in_text_length() {
    let font = test_font();
    for bytes in [4_096_usize, 16_384, crate::MAX_GUI_TEXT_BYTES] {
        let text: String = "aAVe ".chars().cycle().take(bytes).collect();
        let layout = layout(
            &font,
            &text,
            TextLinePolicy::SingleLine,
            TextMaxWidth::Unbounded,
        );
        let middle = layout.size[0] * 0.5;

        CARET_STEPS.set(0);
        let start = Instant::now();
        let offset = caret_offset_at_x(&layout, middle);
        let elapsed = start.elapsed();
        let steps = CARET_STEPS.get();
        println!(
            "scaling caret_bytes={bytes} elapsed_us={} caret_steps={steps}",
            elapsed.as_micros()
        );

        let work = layout.grapheme_boundaries.len() + layout.lines.len() * 2 + layout.glyphs.len();
        assert!(
            steps <= work,
            "caret mapping took {steps} steps over {work} boundaries, lines and glyphs"
        );
        assert!(is_boundary(&text, offset));
        if bytes == 4_096 {
            assert_eq!(
                offset,
                caret_offset_through_caret_positions(&layout, middle)
            );
        }
    }
}
