//! Native single-line editing routed through the composed router into the
//! local mutation boundary, observed through native state, the `text` field
//! and completed Canvas paint.

use crate::services::gui_input::router::{GuiPhysicalButton, GuiPhysicalInput, GuiPhysicalKey};
use crate::services::gui_input::{GuiDeliveryTerminal, GuiInputError};
use crate::systems::canvas::{CanvasPaintEntry, CanvasPart, CanvasPrimitive};
use crate::systems::gui::layout::GuiLayout;
use crate::systems::gui::local::test_support::{
    GuiRoutedHost, GuiTestValue, ROUTED_EXTENT, ROUTED_FONT_SIZE, action, set_field,
};
use crate::systems::gui::local::{
    GuiLocalAction, GuiLocalActionError, GuiLocalEffect, GuiLocalEffectKind, GuiNativeTextState,
    GuiTextComposition, GuiTextEdit, GuiTextFence, GuiTextInput, MAX_GUI_TEXT_BYTES,
};
use crate::{Command, ComponentValue, EntityId, EntityRef};
use std::sync::Arc;

/// Fixture font advances in ems: "a" and "e" 0.55, "V" 0.65, unmapped 0.5.
const A: f32 = 0.55 * ROUTED_FONT_SIZE;
const V: f32 = 0.65 * ROUTED_FONT_SIZE;
const UNMAPPED: f32 = 0.5 * ROUTED_FONT_SIZE;

/// Ascent 0.8, descent 0.2 and line gap 0.2 em.
const LINE: f32 = 1.2 * ROUTED_FONT_SIZE;

/// The marks on a text line, a sixteenth of an em thick: the caret's width
/// and the composition underline along the line's bottom.
const TEXT_MARK: f32 = ROUTED_FONT_SIZE / 16.0;

/// Height of the fixture's text inputs.
const INPUT_HEIGHT: f32 = 20.0;

/// The label line's origin in an input: one em in from the left, centred
/// vertically. The caret, selection and pointer text hits share it.
const ORIGIN: [f32; 2] = [ROUTED_FONT_SIZE, (INPUT_HEIGHT - LINE) * 0.5];

/// A rectangle given relative to the label line, in input coordinates.
fn placed([x, y, width, height]: [f32; 4]) -> [f32; 4] {
    [ORIGIN[0] + x, ORIGIN[1] + y, width, height]
}

/// Fixture glyph identities.
const GLYPH_A: u32 = 3;
const GLYPH_E: u32 = 5;
const GLYPH_V: u32 = 2;

fn text_input(host: &mut GuiRoutedHost, text: &str) -> EntityId {
    host.create(vec![
        ComponentValue::GuiTextInput(GuiTextInput {
            text: text.into(),
            placeholder: Arc::default(),
            ..Default::default()
        }),
        ComponentValue::GuiLayout(GuiLayout {
            width: ROUTED_EXTENT,
            height: INPUT_HEIGHT,
            ..Default::default()
        }),
    ])
}

/// Focus the first control by keyboard, giving it native text at its end.
fn focused(text: &str) -> (GuiRoutedHost, EntityId) {
    let mut host = GuiRoutedHost::new();
    let entity = text_input(&mut host, text);
    host.route(GuiPhysicalInput::Key {
        key: GuiPhysicalKey::Tab,
        shift: false,
    })
    .unwrap();
    let native = host
        .native()
        .expect("Tab gives the text input native focus");
    assert_eq!(native.selection, [text.len() as u32; 2]);
    host.terminals();
    (host, entity)
}

/// Apply one edit stamped with the current fence and return the resulting
/// native state.
fn edit(host: &mut GuiRoutedHost, edit: GuiTextEdit) -> GuiNativeTextState {
    let fence = host.native().expect("native text focus").fence;
    host.edit(fence, edit.clone()).unwrap();
    match host.terminals().as_slice() {
        [GuiDeliveryTerminal::NativeApplied(state)] => {
            assert_eq!(host.native().as_ref(), Some(state), "{edit:?}");
            state.clone()
        }
        other => panic!("{edit:?} was not applied: {other:?}"),
    }
}

/// Route an edit stamped with `fence` that the local boundary must refuse.
fn refused(host: &mut GuiRoutedHost, fence: GuiTextFence, edit: GuiTextEdit) -> GuiInputError {
    host.edit(fence, edit.clone()).unwrap();
    match host.terminals().as_slice() {
        [GuiDeliveryTerminal::Rejected(error)] => *error,
        other => panic!("{edit:?} was not refused: {other:?}"),
    }
}

fn text(state: &GuiNativeTextState) -> &str {
    &state.text
}

/// The input's stored `text` field.
fn stored(host: &mut GuiRoutedHost, entity: EntityId) -> Arc<str> {
    match host.snapshot(entity).value {
        GuiTestValue::Text(text) => text,
        other => panic!("text input value: {other:?}"),
    }
}

/// The text input's completed paint, keyed by part.
fn parts(host: &GuiRoutedHost, entity: EntityId) -> Vec<(CanvasPart, CanvasPrimitive)> {
    host.canvas()
        .entries
        .iter()
        .filter_map(|entry| match entry.as_ref() {
            CanvasPaintEntry::Primitive {
                primitive,
                ..
            } if primitive.style().identity.target.entity == entity => {
                Some((primitive.style().identity.part, primitive.clone()))
            }
            _ => None,
        })
        .collect()
}

fn part(host: &GuiRoutedHost, entity: EntityId, wanted: CanvasPart) -> Option<CanvasPrimitive> {
    let mut found = parts(host, entity)
        .into_iter()
        .filter(|(part, _)| *part == wanted)
        .map(|(_, primitive)| primitive);
    let first = found.next();
    assert!(found.next().is_none(), "{wanted:?} painted twice");
    first
}

/// Canvas-local `[x, y, width, height]` of a painted box part.
fn rect(host: &GuiRoutedHost, entity: EntityId, wanted: CanvasPart) -> Option<[f32; 4]> {
    part(host, entity, wanted).map(|primitive| {
        let CanvasPrimitive::Box {
            style,
            size,
            ..
        } = primitive
        else {
            panic!("{wanted:?} is not a box")
        };
        [style.position[0], style.position[1], size[0], size[1]]
    })
}

/// Label glyph identities and their shared storage.
fn label(
    host: &GuiRoutedHost,
    entity: EntityId,
) -> (Vec<u32>, Arc<[crate::systems::canvas::CanvasGlyph]>) {
    let Some(CanvasPrimitive::Glyphs {
        glyphs,
        ..
    }) = part(host, entity, CanvasPart::Label)
    else {
        panic!("the label paints glyphs")
    };
    (glyphs.iter().map(|glyph| glyph.glyph_id).collect(), glyphs)
}

fn assert_close(actual: [f32; 4], expected: [f32; 4]) {
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| (actual - expected).abs() < 1e-4),
        "{actual:?} != {expected:?}"
    );
}

#[test]
fn typed_edits_write_the_text_field_in_order_and_remeasure_the_label() {
    let mut host = GuiRoutedHost::new();
    let entity = text_input(&mut host, "a");

    // A tap past the text focuses the input with its caret at the end.
    host.tap([50.0, 10.0]);
    let native = host
        .native()
        .expect("a tap gives the text input native focus");
    assert_eq!(native.selection, [1, 1]);
    assert!(host.snapshot(entity).focused);
    host.terminals();
    let layout = host.canvas().layout_revision;

    // Each edit applies to the text its predecessor wrote.
    let typed = edit(&mut host, GuiTextEdit::Insert("e".into()));
    assert_eq!(text(&typed), "ae");
    assert_eq!(typed.selection, [2, 2]);
    assert_ne!(typed.fence.generation, native.fence.generation);
    assert!(Arc::ptr_eq(&typed.text, &stored(&mut host, entity)));
    assert!(host.canvas().layout_revision > layout);
    assert_eq!(label(&host, entity).0, [GLYPH_A, GLYPH_E]);
    assert_close(
        rect(&host, entity, CanvasPart::Caret).unwrap(),
        placed([2.0 * A, 0.0, TEXT_MARK, LINE]),
    );

    let deleted = edit(&mut host, GuiTextEdit::Backspace);
    assert_eq!(text(&deleted), "a");
    assert_eq!(deleted.selection, [1, 1]);
    assert_eq!(label(&host, entity).0, [GLYPH_A]);
    assert_close(
        rect(&host, entity, CanvasPart::Caret).unwrap(),
        placed([A, 0.0, TEXT_MARK, LINE]),
    );
    assert_eq!(&*stored(&mut host, entity), "a");
}

#[test]
fn caret_keys_and_selection_edit_at_the_native_caret() {
    let (mut host, entity) = focused("ae");

    // A collapsed caret inserts between graphemes; a selection is replaced.
    edit(&mut host, GuiTextEdit::Selection([1, 1]));
    let inserted = edit(&mut host, GuiTextEdit::Insert("V".into()));
    assert_eq!((text(&inserted), inserted.selection), ("aVe", [2, 2]));
    edit(&mut host, GuiTextEdit::Selection([0, 1]));
    let replaced = edit(&mut host, GuiTextEdit::Insert("e".into()));
    assert_eq!((text(&replaced), replaced.selection), ("eVe", [1, 1]));

    // Home and Delete remove the first grapheme; arrows collapse a range to
    // its edge before moving; End and Backspace remove the last one.
    assert_eq!(edit(&mut host, GuiTextEdit::Home).selection, [0, 0]);
    let deleted = edit(&mut host, GuiTextEdit::Delete);
    assert_eq!((text(&deleted), deleted.selection), ("Ve", [0, 0]));
    edit(&mut host, GuiTextEdit::SelectAll);
    assert_eq!(edit(&mut host, GuiTextEdit::Right).selection, [2, 2]);
    assert_eq!(edit(&mut host, GuiTextEdit::Left).selection, [1, 1]);
    edit(&mut host, GuiTextEdit::Selection([0, 2]));
    assert_eq!(edit(&mut host, GuiTextEdit::Left).selection, [0, 0]);
    assert_eq!(edit(&mut host, GuiTextEdit::End).selection, [2, 2]);
    let removed = edit(&mut host, GuiTextEdit::Backspace);
    assert_eq!((text(&removed), removed.selection), ("V", [1, 1]));

    // Offsets inside a grapheme are refused without moving the caret.
    edit(&mut host, GuiTextEdit::SelectAll);
    let composed = edit(&mut host, GuiTextEdit::Insert("a\u{301}".into()));
    assert_eq!(text(&composed), "a\u{301}");
    let fence = composed.fence;
    assert_eq!(
        refused(&mut host, fence, GuiTextEdit::Selection([1, 1])),
        GuiInputError::Local(GuiLocalActionError::InvalidValue)
    );
    assert_eq!(host.native().unwrap().selection, [3, 3]);
    assert_eq!(&*stored(&mut host, entity), "a\u{301}");
}

#[test]
fn caret_and_selection_moves_repaint_without_reflow_or_commit() {
    let (mut host, entity) = focused("ae");
    let before = host.canvas();
    let (glyphs, storage) = label(&host, entity);
    assert_eq!(glyphs, [GLYPH_A, GLYPH_E]);
    let committed = stored(&mut host, entity);

    let mut paint = before.paint_revision;
    for (edit_kind, selection) in [
        (GuiTextEdit::Selection([0, 1]), [0, 1]),
        (GuiTextEdit::Right, [1, 1]),
        (GuiTextEdit::Left, [0, 0]),
        (GuiTextEdit::End, [2, 2]),
        (GuiTextEdit::Home, [0, 0]),
        (GuiTextEdit::SelectAll, [0, 2]),
    ] {
        let state = edit(&mut host, edit_kind.clone());
        assert_eq!(state.selection, selection, "{edit_kind:?}");
        assert!(Arc::ptr_eq(&state.text, &committed));
        let after = host.canvas();
        assert_eq!(
            after.layout_revision, before.layout_revision,
            "{edit_kind:?} reflowed the Canvas"
        );
        assert!(
            after.paint_revision > paint,
            "{edit_kind:?} did not repaint"
        );
        paint = after.paint_revision;
        assert!(Arc::ptr_eq(&label(&host, entity).1, &storage));
    }
    assert!(Arc::ptr_eq(&stored(&mut host, entity), &committed));
}

#[test]
fn caret_and_selection_paint_follow_the_measured_label() {
    let (mut host, entity) = focused("ae");

    // A collapsed caret paints a bar at the measured advance and no highlight.
    edit(&mut host, GuiTextEdit::Selection([1, 1]));
    assert_close(
        rect(&host, entity, CanvasPart::Caret).unwrap(),
        placed([A, 0.0, TEXT_MARK, LINE]),
    );
    assert!(rect(&host, entity, CanvasPart::Selection).is_none());

    // A forward range highlights both glyphs with the caret at its end.
    edit(&mut host, GuiTextEdit::Selection([0, 2]));
    assert_close(
        rect(&host, entity, CanvasPart::Selection).unwrap(),
        placed([0.0, 0.0, 2.0 * A, LINE]),
    );
    assert_close(
        rect(&host, entity, CanvasPart::Caret).unwrap(),
        placed([2.0 * A, 0.0, TEXT_MARK, LINE]),
    );

    // A backward range keeps its anchor and caret: the same highlight, with
    // the caret at the leading edge.
    let backward = edit(&mut host, GuiTextEdit::Selection([2, 0]));
    assert_eq!(backward.selection, [2, 0]);
    assert_close(
        rect(&host, entity, CanvasPart::Selection).unwrap(),
        placed([0.0, 0.0, 2.0 * A, LINE]),
    );
    assert_close(
        rect(&host, entity, CanvasPart::Caret).unwrap(),
        placed([0.0, 0.0, TEXT_MARK, LINE]),
    );
    assert_eq!(&*stored(&mut host, entity), "ae");
}

#[test]
fn caret_and_selection_resolve_their_own_skin_rows() {
    use crate::components::rows::Rows;
    use crate::systems::canvas::CanvasShapeFill;
    use crate::systems::gui::presentation::{GuiPaintPart, GuiSkin, GuiTheme};
    use crate::systems::gui::{GuiPartId, GuiPrimitivePart};

    const LABEL: [f32; 4] = [0.9, 0.8, 0.7, 1.0];
    const CARET: [f32; 4] = [0.1, 0.9, 0.9, 1.0];
    const SELECTION: [f32; 4] = [0.1, 0.4, 0.8, 1.0];
    let (mut host, entity) = focused("ae");
    let mut parts = Rows::new();
    for (part, color) in [
        (GuiPrimitivePart::Label, LABEL),
        (GuiPrimitivePart::Caret, CARET),
        (GuiPrimitivePart::Selection, SELECTION),
    ] {
        parts
            .push(GuiPaintPart {
                color: Some(color),
                ..GuiPaintPart::keyed(GuiPartId::base(part)).unwrap()
            })
            .unwrap();
    }
    let theme = host.create(vec![ComponentValue::GuiTheme(GuiTheme {
        parts,
        ..Default::default()
    })]);
    host.apply(vec![Command::insert_value(
        EntityRef::Handle(entity),
        ComponentValue::GuiSkin(GuiSkin {
            theme,
            ..Default::default()
        }),
    )]);
    edit(&mut host, GuiTextEdit::Selection([0, 1]));

    // Each part paints its own colour: the highlight no longer borrows the
    // label's, and the glyphs keep theirs.
    let fill = |part| match part_of_host(&host, entity, part) {
        CanvasPrimitive::Box {
            fill: CanvasShapeFill::Solid(color),
            ..
        } => color,
        other => panic!("{other:?}"),
    };
    assert_eq!(fill(CanvasPart::Selection), SELECTION);
    assert_eq!(fill(CanvasPart::Caret), CARET);
    assert_eq!(
        part_of_host(&host, entity, CanvasPart::Label).style().color,
        LABEL
    );
}

#[test]
fn an_unthemed_text_input_paints_the_reference_text_caret_and_selection() {
    use crate::systems::canvas::CanvasShapeFill;

    /// Linear RGBA of an sRGB `0xrrggbb` sample.
    fn srgb(hex: u32) -> [f32; 4] {
        let linear = |byte: u32| {
            let value = f64::from(byte & 0xff) / 255.0;
            (if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }) as f32
        };
        [linear(hex >> 16), linear(hex >> 8), linear(hex), 1.0]
    }

    let (mut host, entity) = focused("ae");
    edit(&mut host, GuiTextEdit::Selection([0, 1]));
    let fill = |part| match part_of_host(&host, entity, part) {
        CanvasPrimitive::Box {
            fill: CanvasShapeFill::Solid(color),
            ..
        } => color,
        other => panic!("{other:?}"),
    };
    assert_eq!(fill(CanvasPart::Selection), srgb(0x0b6fc0));
    assert_eq!(fill(CanvasPart::Caret), srgb(0x00f4fb));
    assert_eq!(
        part_of_host(&host, entity, CanvasPart::Label).style().color,
        srgb(0xe5f5f7)
    );
}

fn part_of_host(host: &GuiRoutedHost, entity: EntityId, wanted: CanvasPart) -> CanvasPrimitive {
    part(host, entity, wanted).unwrap_or_else(|| panic!("{wanted:?} must paint"))
}

#[test]
fn backward_multibyte_selection_keeps_its_utf8_anchor_and_caret() {
    let (mut host, entity) = focused("a😀b");

    // The emoji spans UTF-8 bytes 1..5 and paints the unmapped advance.
    let backward = edit(&mut host, GuiTextEdit::Selection([5, 1]));
    assert_eq!(backward.selection, [5, 1]);
    assert_close(
        rect(&host, entity, CanvasPart::Selection).unwrap(),
        placed([A, 0.0, UNMAPPED, LINE]),
    );
    assert_close(
        rect(&host, entity, CanvasPart::Caret).unwrap(),
        placed([A, 0.0, TEXT_MARK, LINE]),
    );

    // Extending from that anchor keeps it while the caret moves.
    let left = edit(&mut host, GuiTextEdit::Left);
    assert_eq!(left.selection, [1, 1]);
    assert_eq!(&*stored(&mut host, entity), "a😀b");
}

#[test]
fn pointer_drag_selects_through_each_routed_caret_change() {
    let mut host = GuiRoutedHost::new();
    let entity = text_input(&mut host, "ae");
    // Pointer positions along the label line.
    let point = |host: &GuiRoutedHost, x: f32| host.point([ORIGIN[0] + x, 10.0]);

    let down = point(&host, 0.5);
    host.route(GuiPhysicalInput::PointerDown {
        pointer: 1,
        point: down,
        button: GuiPhysicalButton::Primary,
    })
    .unwrap();
    assert_eq!(host.native().unwrap().selection, [0, 0]);
    host.terminals();

    // Every move past a grapheme boundary publishes its extended selection.
    for (x, selection) in [(2.0 * A, [0, 2]), (A + 0.5, [0, 1]), (0.2, [0, 0])] {
        let move_to = point(&host, x);
        host.route(GuiPhysicalInput::PointerMove {
            pointer: 1,
            point: move_to,
        })
        .unwrap();
        assert_eq!(host.native().unwrap().selection, selection, "at {x}");
        assert!(matches!(
            host.terminals().as_slice(),
            [GuiDeliveryTerminal::NativeApplied(state)] if state.selection == selection
        ));
    }
    let end = point(&host, 2.0 * A);
    host.route(GuiPhysicalInput::PointerMove {
        pointer: 1,
        point: end,
    })
    .unwrap();
    host.route(GuiPhysicalInput::PointerUp {
        pointer: 1,
        point: end,
        button: GuiPhysicalButton::Primary,
    })
    .unwrap();
    let released = host.native().unwrap();
    assert_eq!(released.selection, [0, 2]);
    assert_close(
        rect(&host, entity, CanvasPart::Selection).unwrap(),
        placed([0.0, 0.0, 2.0 * A, LINE]),
    );
    assert_eq!(&*stored(&mut host, entity), "ae");
}

#[test]
fn composition_paints_its_own_part_and_leaves_with_cancel_commit_or_focus_loss() {
    let (mut host, entity) = focused("ae");
    let committed = stored(&mut host, entity);

    // A provisional run displays at the caret with its own underline, clause
    // highlight and caret, without touching the committed value.
    let composing = edit(
        &mut host,
        GuiTextEdit::Compose(GuiTextComposition {
            text: "ee".into(),
            selection: [0, 1],
        }),
    );
    assert!(Arc::ptr_eq(&composing.text, &committed));
    assert_eq!(label(&host, entity).0, [GLYPH_A, GLYPH_E, GLYPH_E, GLYPH_E]);
    assert_close(
        rect(&host, entity, CanvasPart::Composition).unwrap(),
        placed([2.0 * A, LINE - TEXT_MARK, 2.0 * A, TEXT_MARK]),
    );
    assert_close(
        rect(&host, entity, CanvasPart::Selection).unwrap(),
        placed([2.0 * A, 0.0, A, LINE]),
    );
    assert_close(
        rect(&host, entity, CanvasPart::Caret).unwrap(),
        placed([3.0 * A, 0.0, TEXT_MARK, LINE]),
    );
    let identities: std::collections::BTreeSet<_> = parts(&host, entity)
        .iter()
        .map(|(_, primitive)| primitive.style().identity)
        .collect();
    assert_eq!(identities.len(), parts(&host, entity).len());
    assert!(Arc::ptr_eq(&stored(&mut host, entity), &committed));

    // Cancelling drops the provisional paint and restores the committed label.
    edit(&mut host, GuiTextEdit::CancelComposition);
    assert!(part(&host, entity, CanvasPart::Composition).is_none());
    assert_eq!(label(&host, entity).0, [GLYPH_A, GLYPH_E]);
    assert!(Arc::ptr_eq(&stored(&mut host, entity), &committed));

    // Committing inserts the provisional text once.
    edit(
        &mut host,
        GuiTextEdit::Compose(GuiTextComposition {
            text: "V".into(),
            selection: [1, 1],
        }),
    );
    let accepted = edit(&mut host, GuiTextEdit::CommitComposition);
    assert_eq!(text(&accepted), "aeV");
    assert_eq!(accepted.composition, None);
    assert!(part(&host, entity, CanvasPart::Composition).is_none());
    assert_eq!(label(&host, entity).0, [GLYPH_A, GLYPH_E, GLYPH_V]);
    assert_close(
        rect(&host, entity, CanvasPart::Caret).unwrap(),
        placed([2.0 * A + V, 0.0, TEXT_MARK, LINE]),
    );

    // Losing focus drops an open provisional run from paint, and its later
    // commit writes nothing.
    let stale = edit(
        &mut host,
        GuiTextEdit::Compose(GuiTextComposition {
            text: "ee".into(),
            selection: [2, 2],
        }),
    )
    .fence;
    host.route(GuiPhysicalInput::Key {
        key: GuiPhysicalKey::Escape,
        shift: false,
    })
    .unwrap();
    assert!(host.native().is_none());
    assert!(!host.snapshot(entity).focused);
    assert!(part(&host, entity, CanvasPart::Composition).is_none());
    assert!(part(&host, entity, CanvasPart::Caret).is_none());
    assert_eq!(label(&host, entity).0, [GLYPH_A, GLYPH_E, GLYPH_V]);
    host.terminals();
    assert_eq!(
        host.edit(stale, GuiTextEdit::CommitComposition),
        Err(GuiInputError::Unavailable)
    );
    assert_eq!(&*stored(&mut host, entity), "aeV");
}

#[test]
fn composition_selection_must_fall_on_grapheme_boundaries() {
    let (mut host, entity) = focused("");
    let fence = host.native().unwrap().fence;
    assert_eq!(
        refused(
            &mut host,
            fence,
            GuiTextEdit::Compose(GuiTextComposition {
                text: "a\u{301}".into(),
                selection: [1, 3],
            }),
        ),
        GuiInputError::Local(GuiLocalActionError::InvalidValue)
    );
    assert_eq!(host.native().unwrap().composition, None);
    let composing = edit(
        &mut host,
        GuiTextEdit::Compose(GuiTextComposition {
            text: "a\u{301}".into(),
            selection: [0, 3],
        }),
    );
    assert_eq!(
        composing.composition.as_deref(),
        Some(&GuiTextComposition {
            text: "a\u{301}".into(),
            selection: [0, 3],
        })
    );
    assert_eq!(&*stored(&mut host, entity), "");
}

#[test]
fn stale_fences_conflict_after_equal_length_client_write_and_native_text_refreshes() {
    let (mut host, entity) = focused("ae");
    let world = host.world.id();
    let observed = edit(&mut host, GuiTextEdit::Selection([0, 1]));
    let write = |host: &mut GuiRoutedHost, value: &str| {
        let written = set_field(
            &mut host.gui,
            world,
            entity,
            ComponentValue::GUI_TEXT_INPUT,
            std::mem::offset_of!(GuiTextInput, text),
            crate::FieldValue::String(value.into()),
        );
        assert!(written.result.is_ok(), "{written:?}");
    };

    // An equal-length client write refreshes the native state without new
    // input: same target, a new generation, caret at the end.
    write(&mut host, "Ve");
    host.frame();
    host.terminals();
    let refreshed = host.native().unwrap();
    assert_eq!(refreshed.fence.target, observed.fence.target);
    assert_ne!(refreshed.fence.generation, observed.fence.generation);
    assert_eq!(text(&refreshed), "Ve");
    assert!(Arc::ptr_eq(&refreshed.text, &stored(&mut host, entity)));
    assert_eq!(refreshed.selection, [2, 2]);

    // Delayed edits stamped before the write keep in-bounds offsets but
    // conflict instead of rebasing, as does a stamp naming a generation the
    // text never reached.
    for stale in [
        (observed.fence, GuiTextEdit::Selection([0, 1])),
        (observed.fence, GuiTextEdit::Insert("late".into())),
        (
            GuiTextFence {
                generation: refreshed.fence.generation + 9,
                ..refreshed.fence
            },
            GuiTextEdit::Insert("future".into()),
        ),
    ] {
        assert_eq!(
            refused(&mut host, stale.0, stale.1),
            GuiInputError::Unavailable
        );
        assert_eq!(host.native().unwrap(), refreshed);
    }

    // The next edit stamped against the refreshed text lands at its end.
    let appended = edit(&mut host, GuiTextEdit::Insert("A".into()));
    assert_eq!(text(&appended), "VeA");

    // A provisional run fenced to its generation never commits over an
    // equal-length write; the write also clears it.
    let composing = edit(
        &mut host,
        GuiTextEdit::Compose(GuiTextComposition {
            text: "V".into(),
            selection: [1, 1],
        }),
    );
    write(&mut host, "123");
    host.frame();
    host.terminals();
    assert_eq!(host.native().unwrap().composition, None);
    assert_eq!(
        refused(&mut host, composing.fence, GuiTextEdit::CommitComposition),
        GuiInputError::Unavailable
    );
    assert_eq!(&*stored(&mut host, entity), "123");
}

#[test]
fn programmatic_focus_keeps_native_ownership_and_blur_fences_old_edits() {
    let (mut host, entity) = focused("ae");
    let world = host.world.id();
    let composing = edit(
        &mut host,
        GuiTextEdit::Compose(GuiTextComposition {
            text: "V".into(),
            selection: [1, 1],
        }),
    );

    // A command focusing the focused control changes nothing: the physical
    // session keeps its native record and provisional run.
    let target = host.snapshot(entity).target;
    action(&mut host.gui, world, target, GuiLocalAction::Focus(0));
    host.frame();
    assert_eq!(host.native().unwrap().composition, composing.composition);

    // A command blurring it ends focus and the native record with its run.
    action(&mut host.gui, world, target, GuiLocalAction::Blur);
    host.frame();
    assert!(!host.snapshot(entity).focused);
    assert!(host.native().is_none());
    assert!(part(&host, entity, CanvasPart::Composition).is_none());
    assert_eq!(label(&host, entity).0, [GLYPH_A, GLYPH_E]);
    host.terminals();

    // The session's provisional commit names the ended record.
    assert_eq!(
        refused(&mut host, composing.fence, GuiTextEdit::CommitComposition),
        GuiInputError::Unavailable
    );
    assert_eq!(&*stored(&mut host, entity), "ae");
}

#[test]
fn client_focus_on_a_text_input_gives_the_presenting_context_its_native_record() {
    let mut host = GuiRoutedHost::new();
    let entity = text_input(&mut host, "ab");
    let target = host.snapshot(entity).target;
    let world = host.world.id();

    // A command focuses the input without an owning session or native record.
    action(&mut host.gui, world, target, GuiLocalAction::Focus(0));
    host.frame();
    assert!(host.snapshot(entity).focused);
    assert!(host.native().is_none());

    // The presenting context adopts it at its routing boundary, and typing
    // reaches it.
    host.synchronize();
    host.frame();
    assert_eq!(host.native().unwrap().fence.target, target);
    assert_eq!(
        &*edit(&mut host, GuiTextEdit::Insert("c".into())).text,
        "abc"
    );
    assert_eq!(&*stored(&mut host, entity), "abc");

    // The focus stays the command's: replacing the input context keeps it and
    // ends the old native record, and the next context installs its own.
    let old = host.native().unwrap().fence;
    host.rebind();
    host.frame();
    assert!(host.snapshot(entity).focused);
    assert!(host.native().is_none());
    host.synchronize();
    host.frame();
    assert_eq!(
        refused(&mut host, old, GuiTextEdit::Insert("x".into())),
        GuiInputError::Unavailable
    );
    assert_eq!(
        &*edit(&mut host, GuiTextEdit::Insert("d".into())).text,
        "abcd"
    );
}

/// Submitted text among routed terminals.
fn submissions(terminals: &[GuiDeliveryTerminal]) -> Vec<Arc<str>> {
    terminals
        .iter()
        .filter_map(|terminal| match terminal {
            GuiDeliveryTerminal::Applied(GuiLocalEffect {
                kind: GuiLocalEffectKind::Submitted(text),
                ..
            }) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn pipelined_enter_submits_the_typed_text_once() {
    let (mut host, entity) = focused("a");
    let fence = host.native().unwrap().fence;

    // Typing and Enter reach the same local boundary before any frame.
    host.send(GuiPhysicalInput::Text {
        fence,
        edit: GuiTextEdit::Insert("b".into()),
    })
    .unwrap();
    host.send(GuiPhysicalInput::Key {
        key: GuiPhysicalKey::Enter,
        shift: false,
    })
    .unwrap();
    host.frame();
    let terminals = host.terminals();
    assert_eq!(
        submissions(&terminals),
        [Arc::<str>::from("ab")],
        "{terminals:?}"
    );

    // Submission writes nothing and publishes nothing further.
    host.frame();
    assert!(host.terminals().is_empty());
    assert_eq!(&*stored(&mut host, entity), "ab");
    assert!(host.snapshot(entity).focused);
}

#[test]
fn enter_during_composition_submits_nothing_and_keeps_the_provisional_run() {
    let (mut host, entity) = focused("a");
    edit(
        &mut host,
        GuiTextEdit::Compose(GuiTextComposition {
            text: "V".into(),
            selection: [1, 1],
        }),
    );

    // Enter belongs to the IME while it composes, by native edit or key.
    let fence = host.native().unwrap().fence;
    assert_eq!(
        refused(&mut host, fence, GuiTextEdit::Submit),
        GuiInputError::Local(GuiLocalActionError::InvalidValue)
    );
    host.route(GuiPhysicalInput::Key {
        key: GuiPhysicalKey::Enter,
        shift: false,
    })
    .unwrap();
    let terminals = host.terminals();
    assert!(submissions(&terminals).is_empty(), "{terminals:?}");
    assert!(host.native().unwrap().composition.is_some());
    assert_eq!(&*stored(&mut host, entity), "a");

    // Once the IME commits, Enter submits the composed text.
    edit(&mut host, GuiTextEdit::CommitComposition);
    host.route(GuiPhysicalInput::Key {
        key: GuiPhysicalKey::Enter,
        shift: false,
    })
    .unwrap();
    assert_eq!(submissions(&host.terminals()), [Arc::<str>::from("aV")]);
}

#[test]
fn native_edits_share_the_committed_text_limit() {
    let (mut host, entity) = focused("");
    let fence = host.native().unwrap().fence;
    let oversized: Arc<str> = "a".repeat(MAX_GUI_TEXT_BYTES + 1).into();
    for edit in [
        GuiTextEdit::Insert(oversized.clone()),
        GuiTextEdit::Compose(GuiTextComposition {
            text: oversized.clone(),
            selection: [0, 0],
        }),
        GuiTextEdit::Insert("line\nbreak".into()),
    ] {
        assert!(matches!(
            refused(&mut host, fence, edit),
            GuiInputError::Local(GuiLocalActionError::InvalidValue) | GuiInputError::Capacity
        ));
    }
    assert_eq!(host.native().unwrap().fence, fence);

    // The limit is inclusive; one byte more past a full value is refused.
    let full = edit(
        &mut host,
        GuiTextEdit::Insert("a".repeat(MAX_GUI_TEXT_BYTES).into()),
    );
    assert_eq!(full.text.len(), MAX_GUI_TEXT_BYTES);
    assert_eq!(
        refused(&mut host, full.fence, GuiTextEdit::Insert("a".into())),
        GuiInputError::Local(GuiLocalActionError::InvalidValue)
    );
    assert!(Arc::ptr_eq(&stored(&mut host, entity), &full.text));
}

#[test]
fn focus_and_native_text_survive_their_font_unloading() {
    let (mut host, entity) = focused("ae");
    edit(&mut host, GuiTextEdit::Selection([0, 1]));
    let root = host.root_entity;
    host.apply(vec![Command::RemoveComponent {
        entity: EntityRef::Handle(root),
        component: ComponentValue::GUI_FONT,
    }]);
    host.frame();
    host.frame();

    // Focus does not depend on the font: the input stays focused and
    // editable while only its measured glyphs are withdrawn.
    let snapshot = host.snapshot(entity);
    assert!(snapshot.focused && snapshot.available);
    assert!(part(&host, entity, CanvasPart::Label).is_none());
    let native = host.native().expect("native focus survives the font");
    assert_eq!(native.selection, [0, 1]);
    let typed = edit(&mut host, GuiTextEdit::Insert("V".into()));
    assert_eq!(text(&typed), "Ve");
}

fn mask(host: &mut GuiRoutedHost, entity: EntityId, masked: bool) {
    let world = host.world.id();
    let written = set_field(
        &mut host.gui,
        world,
        entity,
        ComponentValue::GUI_TEXT_INPUT,
        std::mem::offset_of!(GuiTextInput, masked),
        crate::FieldValue::Bool(masked),
    );
    assert!(written.result.is_ok(), "{written:?}");
    host.frame();
    host.terminals();
}

#[test]
fn masking_measures_one_glyph_per_grapheme_with_original_caret_offsets() {
    let (mut host, entity) = focused("ae\u{301}V");
    mask(&mut host, entity, true);
    assert_eq!(label(&host, entity).0, [0, 0, 0]);
    assert_eq!(&*stored(&mut host, entity), "ae\u{301}V");
    assert!(host.native().unwrap().masked);

    edit(&mut host, GuiTextEdit::Selection([1, 4]));
    assert_close(
        rect(&host, entity, CanvasPart::Selection).unwrap(),
        placed([UNMAPPED, 0.0, UNMAPPED, LINE]),
    );
    assert_close(
        rect(&host, entity, CanvasPart::Caret).unwrap(),
        placed([2.0 * UNMAPPED, 0.0, TEXT_MARK, LINE]),
    );

    // Pointer selection also maps the painted mask back to source bytes.
    host.tap([ORIGIN[0] + 2.0 * UNMAPPED, 10.0]);
    assert_eq!(host.native().unwrap().selection, [4, 4]);
    host.terminals();
    edit(&mut host, GuiTextEdit::Backspace);
    assert_eq!(&*stored(&mut host, entity), "aV");
    assert_eq!(label(&host, entity).0, [0, 0]);
}

#[test]
fn reveal_preserves_native_fence_selection_and_provisional_composition() {
    let (mut host, entity) = focused("aV");
    mask(&mut host, entity, true);
    edit(&mut host, GuiTextEdit::Selection([1, 2]));
    let before = edit(
        &mut host,
        GuiTextEdit::Compose(GuiTextComposition {
            text: "e\u{301}".into(),
            selection: [0, 3],
        }),
    );
    assert_eq!(label(&host, entity).0, [0, 0]);
    assert_close(
        rect(&host, entity, CanvasPart::Composition).unwrap(),
        placed([UNMAPPED, LINE - TEXT_MARK, UNMAPPED, TEXT_MARK]),
    );
    mask(&mut host, entity, false);
    let after = host.native().unwrap();
    assert_eq!(
        after,
        GuiNativeTextState {
            masked: false,
            ..before.clone()
        }
    );
    assert!(host.snapshot(entity).focused);
    assert_eq!(&*stored(&mut host, entity), "aV");
    edit(&mut host, GuiTextEdit::CommitComposition);
    assert_eq!(&*stored(&mut host, entity), "ae\u{301}");
}

#[test]
fn masked_placeholder_is_readable_and_empty_composition_is_masked() {
    let mut host = GuiRoutedHost::new();
    let entity = host.create(vec![
        ComponentValue::GuiTextInput(GuiTextInput {
            masked: true,
            placeholder: "aV".into(),
            ..Default::default()
        }),
        ComponentValue::GuiLayout(GuiLayout {
            width: ROUTED_EXTENT,
            height: INPUT_HEIGHT,
            ..Default::default()
        }),
    ]);
    assert_eq!(label(&host, entity).0, [GLYPH_A, GLYPH_V]);
    host.tap([50.0, 10.0]);
    host.terminals();
    edit(
        &mut host,
        GuiTextEdit::Compose(GuiTextComposition {
            text: "e\u{301}".into(),
            selection: [3, 3],
        }),
    );
    assert_eq!(label(&host, entity).0, [0]);
    assert_eq!(&*stored(&mut host, entity), "");
}

#[test]
fn masked_numeric_commits_and_steps_keep_their_number_semantics() {
    let mut host = GuiRoutedHost::new();
    let entity = host.create(vec![
        ComponentValue::GuiTextInput(GuiTextInput {
            masked: true,
            numeric: true,
            value: 10.0,
            ..Default::default()
        }),
        ComponentValue::GuiLayout(GuiLayout {
            width: ROUTED_EXTENT,
            height: INPUT_HEIGHT,
            ..Default::default()
        }),
    ]);
    host.tap([50.0, 10.0]);
    host.terminals();
    assert_eq!(&*host.native().unwrap().text, "10");
    assert_eq!(label(&host, entity).0, [0, 0]);
    edit(&mut host, GuiTextEdit::SelectAll);
    edit(&mut host, GuiTextEdit::Insert("12".into()));
    let fence = host.native().unwrap().fence;
    host.edit(fence, GuiTextEdit::Submit).unwrap();
    host.terminals();
    assert_eq!(&*host.native().unwrap().text, "12");
    assert!(host.native().unwrap().masked);
    host.route(GuiPhysicalInput::Key {
        key: GuiPhysicalKey::Up,
        shift: false,
    })
    .unwrap();
    assert_eq!(&*host.native().unwrap().text, "13");
    assert!(host.native().unwrap().masked);
}
