use super::*;
use crate::components::rows::Rows;
use crate::components::schema::ComponentLifecycle;
use crate::systems::gui::presentation::GuiTheme;
use std::collections::BTreeSet;

fn look(name: &str) -> &'static GuiSkinLook {
    gui_skin_looks()
        .iter()
        .find(|look| look.name == name)
        .unwrap()
}

#[test]
fn every_look_is_a_valid_asset_free_theme_at_the_body_type_size() {
    let mut names = BTreeSet::new();
    for look in gui_skin_looks() {
        assert!(names.insert(look.name), "{} repeats", look.name);
        assert_eq!(look.em, 16.0, "{}", look.name);
        let mut parts = Rows::new();
        for row in &look.parts {
            assert!(row.asset.is_none(), "{} paints an asset", look.name);
            parts.push(row.clone()).unwrap();
        }
        assert_eq!(
            GuiTheme {
                parts,
                ..Default::default()
            }
            .validate(),
            Ok(()),
            "{}",
            look.name
        );
    }
    assert_eq!(
        names,
        BTreeSet::from([
            "amber",
            "button",
            "checkbox",
            "color",
            "dial",
            "docked",
            "scroll",
            "secondary",
            "secondaryAmber",
            "slider",
            "switch",
            "textInput"
        ])
    );
}

#[test]
fn every_control_kind_paints_its_own_default_look() {
    for (kind, name) in [
        (GuiControlKind::Button, "button"),
        (GuiControlKind::Checkbox, "checkbox"),
        (GuiControlKind::Slider, "slider"),
        (GuiControlKind::TextInput, "textInput"),
        (GuiControlKind::ScrollView, "scroll"),
        (GuiControlKind::VirtualList, "scroll"),
        (GuiControlKind::Color, "color"),
    ] {
        assert_eq!(default_look(kind).name, name);
    }
}

#[test]
fn colours_are_linear_and_every_cut_pairs_the_top_left_and_bottom_right_corners() {
    assert_eq!(srgb(0xffffff), [1.0; 4]);
    assert_eq!(srgb(0x000000), [0.0, 0.0, 0.0, 1.0]);
    // sRGB 0x80 is 0.2158605 linear; 0x0a lies on the linear segment.
    let mid = srgb(0x800a00);
    assert!((mid[0] - 0.215_860_5).abs() < 1e-6, "{mid:?}");
    assert!((mid[1] - 10.0 / 255.0 / 12.92).abs() < 1e-7, "{mid:?}");

    // Every cut element of every look uses the one pattern in one of its two
    // sizes, or none where a button docks; only tracks point both ends, half
    // the default bar, and a numeric field's step parts, divisions of the
    // field, each keep only the field's cut at their end.
    let tracks = [
        GuiPrimitivePart::ScrollTrackX,
        GuiPrimitivePart::ScrollTrackY,
    ]
    .map(|part| GuiPartId::base(part).index().unwrap());
    let [decrement, increment] = [GuiPrimitivePart::Decrement, GuiPrimitivePart::Increment]
        .map(|part| GuiPartId::base(part).index().unwrap());
    for look in gui_skin_looks() {
        for row in &look.parts {
            let Some(corners) = row.corner_cut else {
                continue;
            };
            if tracks.contains(&row.part) {
                assert_eq!(corners, [4.0; 4], "{}", look.name);
            } else if row.part == decrement {
                assert_eq!(corners, [8.0, 0.0, 0.0, 0.0], "{}", look.name);
            } else if row.part == increment {
                assert_eq!(corners, [0.0, 0.0, 8.0, 0.0], "{}", look.name);
            } else {
                assert!(
                    [[8.0, 0.0, 8.0, 0.0], [4.0, 0.0, 4.0, 0.0], [0.0; 4]].contains(&corners),
                    "{} {corners:?}",
                    look.name
                );
            }
        }
    }
}

#[test]
fn every_interactive_part_takes_the_same_state_treatments() {
    let background = |state| GuiPartId::state(GuiPrimitivePart::Background, state);
    let lit = srgb(0x00f4fb);
    for name in [
        "button",
        "checkbox",
        "switch",
        "textInput",
        "secondary",
        "docked",
        "dial",
    ] {
        // Hover: the lit line with the half glow of a frame's reach.
        let hover = look(name).row(background(GuiSkinState::Hovered)).unwrap();
        assert_eq!(
            (hover.border_width, hover.border_color, hover.glow_color),
            (Some(LIT_LINE), Some(lit), Some(lit)),
            "{name}"
        );
        assert_eq!(
            (
                hover.glow_intensity,
                hover.glow_radius,
                hover.glow_inner_radius
            ),
            (Some(0.02), Some(16.0), Some(16.0)),
            "{name}"
        );

        // Focus: the lit line with the full glow, on the control's own cut.
        let ring = look(name)
            .row(GuiPartId::base(GuiPrimitivePart::FocusRing))
            .unwrap();
        let frame = look(name).row(GuiPartId::base(GuiPrimitivePart::Background));
        // A ring's colour styles its outline; its interior stays clear.
        assert_eq!(
            (ring.border_width, ring.color, ring.glow_intensity),
            (Some(LIT_LINE), Some(lit), Some(0.04)),
            "{name}"
        );
        assert_eq!(ring.corner_cut, frame.unwrap().corner_cut, "{name}");
    }

    // Moving parts, and a numeric field's step parts, glow at half the
    // reach.
    for (name, part) in [
        ("slider", GuiPrimitivePart::Icon),
        ("scroll", GuiPrimitivePart::ScrollThumbY),
        ("textInput", GuiPrimitivePart::Decrement),
        ("textInput", GuiPrimitivePart::Increment),
    ] {
        let hover = look(name)
            .row(GuiPartId::state(part, GuiSkinState::Hovered))
            .unwrap();
        assert_eq!(hover.glow_radius, Some(8.0), "{name}");
    }

    // Pressed parts fill with the lit colour under the hover edge.
    for (name, part) in [
        ("button", GuiPrimitivePart::Background),
        ("checkbox", GuiPrimitivePart::Background),
        ("slider", GuiPrimitivePart::Icon),
        ("textInput", GuiPrimitivePart::Decrement),
        ("textInput", GuiPrimitivePart::Increment),
    ] {
        let pressed = look(name)
            .row(GuiPartId::state(part, GuiSkinState::Pressed))
            .unwrap();
        assert_eq!(
            (pressed.color, pressed.border_width),
            (Some(lit), Some(LIT_LINE)),
            "{name}"
        );
    }
}

#[test]
fn the_dial_look_draws_its_rings_inside_a_cut_housing() {
    let dial = look("dial");
    let row = |part, state: Option<GuiSkinState>| {
        let identity = match state {
            Some(state) => GuiPartId::state(part, state),
            None => GuiPartId::base(part),
        };
        dial.row(identity).unwrap()
    };
    let (lit, line, neutral, surface) = (
        srgb(0x00f4fb),
        srgb(0x355c70),
        srgb(0x90b0c4),
        srgb(0x00131c),
    );

    // The housing is a frame-cut control frame; a press lights its edge but
    // never fills it, so the dial stays visible while dragged.
    let housing = row(GuiPrimitivePart::Background, None);
    assert_eq!(
        (housing.color, housing.border_color, housing.corner_cut),
        (Some(surface), Some(neutral), Some([8.0, 0.0, 8.0, 0.0]))
    );
    let pressed = row(GuiPrimitivePart::Background, Some(GuiSkinState::Pressed));
    assert_eq!(
        (pressed.color, pressed.border_color, pressed.border_width),
        (None, Some(lit), Some(LIT_LINE))
    );

    // Ticks and track are arcs in the quiet line; the value arc is lit and
    // the pointer a lit stroke, both `neutral` while disabled.
    for (part, colour, shape) in [
        (GuiPrimitivePart::Ticks, line, 2.0),
        (GuiPrimitivePart::Track, line, 2.0),
        (GuiPrimitivePart::Fill, lit, 2.0),
        (GuiPrimitivePart::Icon, lit, 1.0),
    ] {
        let rest = row(part, None);
        assert_eq!(
            (rest.color, rest.shape),
            (Some(colour), Some(shape)),
            "{part:?}"
        );
    }
    for part in [GuiPrimitivePart::Fill, GuiPrimitivePart::Icon] {
        assert_eq!(
            row(part, Some(GuiSkinState::Disabled)).color,
            Some(neutral),
            "{part:?}"
        );
    }

    // A tick on each tenth of the 270-degree sweep, ends included, each the
    // idle line's width at the middle of a default dial's ticks: 30 units
    // from its centre.
    let [cells, duty] = row(GuiPrimitivePart::Ticks, None).arc_dashes.unwrap();
    assert!((cells * 0.75 - 10.0).abs() < 1e-5, "{cells}");
    let width = duty / cells * std::f32::consts::TAU * 30.0;
    assert!((width - LINE).abs() < 1e-5, "{width}");

    // Dragging glows the pointer, the moving part, at the half glow.
    let dragged = row(GuiPrimitivePart::Icon, Some(GuiSkinState::Pressed));
    assert_eq!(
        (dragged.glow_intensity, dragged.glow_radius),
        (Some(0.02), Some(8.0))
    );
}

#[test]
fn hierarchy_and_variants_change_only_their_tokens() {
    let background = GuiPartId::base(GuiPrimitivePart::Background);
    let label = GuiPartId::base(GuiPrimitivePart::Label);
    let cut_of = |name| look(name).row(background).unwrap().corner_cut;
    let label_of = |name| look(name).row(label).unwrap().color;

    // Primary buttons: the frame cut and a lit label; secondary buttons the
    // part cut and a text label; docked buttons no cut.
    assert_eq!(cut_of("button"), Some([8.0, 0.0, 8.0, 0.0]));
    assert_eq!(cut_of("secondary"), Some([4.0, 0.0, 4.0, 0.0]));
    assert_eq!(cut_of("docked"), Some([0.0; 4]));
    assert_eq!(label_of("button"), Some(srgb(0x00f4fb)));
    assert_eq!(label_of("secondary"), Some(srgb(0xe5f5f7)));

    // Amber replaces the lit colour and the idle line in every row.
    let amber = srgb(0xfbdc6c);
    assert_eq!(
        look("amber").row(background).unwrap().border_color,
        Some(amber)
    );
    assert_eq!(label_of("amber"), Some(amber));
    assert_eq!(label_of("secondaryAmber"), Some(srgb(0xe5f5f7)));
    for (primary, variant) in [("button", "amber"), ("secondary", "secondaryAmber")] {
        let (primary, variant) = (look(primary), look(variant));
        assert_eq!(primary.parts.len(), variant.parts.len());
        for (row, other) in primary.parts.iter().zip(&variant.parts) {
            assert_eq!(
                (row.part, row.corner_cut, row.border_width, row.glow_radius),
                (
                    other.part,
                    other.corner_cut,
                    other.border_width,
                    other.glow_radius
                )
            );
        }
    }
}

#[test]
fn every_button_look_fills_a_selected_button_with_its_lit_colour() {
    // Selected is the checked variant: the lit fill and line in every
    // enabled state, `neutral` while disabled, the label in the surface
    // colour on either fill, at every hierarchy level and in either variant.
    let (cyan, amber) = (srgb(0x00f4fb), srgb(0xfbdc6c));
    let (surface, neutral) = (srgb(0x00131c), srgb(0x90b0c4));
    for (name, lit) in [
        ("button", cyan),
        ("secondary", cyan),
        ("docked", cyan),
        ("amber", amber),
        ("secondaryAmber", amber),
    ] {
        for state in [
            GuiSkinState::Idle,
            GuiSkinState::Hovered,
            GuiSkinState::Pressed,
            GuiSkinState::Disabled,
        ] {
            let selected = |part| GuiPartId::variant(part, state, GuiPartVariant::Checked);
            let fill = look(name)
                .row(selected(GuiPrimitivePart::Background))
                .unwrap();
            let expected = if state == GuiSkinState::Disabled {
                neutral
            } else {
                lit
            };
            assert_eq!(
                (fill.color, fill.border_color),
                (Some(expected), Some(expected)),
                "{name} {state:?}"
            );

            // Only colours change: the cut, line weights and glow stay those
            // of the state, so focus still lights the same contour.
            assert_eq!(
                (fill.corner_cut, fill.border_width, fill.glow_intensity),
                (None, None, None),
                "{name} {state:?}"
            );
            let label = look(name).row(selected(GuiPrimitivePart::Label)).unwrap();
            assert_eq!(label.color, Some(surface), "{name} {state:?}");
        }

        // An unselected button keeps its unqualified rows.
        let unselected = GuiPartId::variant(
            GuiPrimitivePart::Background,
            GuiSkinState::Idle,
            GuiPartVariant::Unchecked,
        );
        assert!(look(name).row(unselected).is_none(), "{name}");
    }
}

#[test]
fn the_switch_look_switches_off_what_its_checkbox_default_paints() {
    let icon = GuiPartId::base(GuiPrimitivePart::Icon);
    let default = default_look(GuiControlKind::Checkbox).row(icon).unwrap();
    let block = look("switch").row(icon).unwrap();
    assert_eq!(default.shape, Some(1.0));
    assert_eq!((block.shape, block.border_width), (Some(0.0), Some(0.0)));

    // The rail keeps its surface while on, and an unchecked switch shows its
    // off block in every state.
    for state in [
        GuiSkinState::Idle,
        GuiSkinState::Hovered,
        GuiSkinState::Pressed,
        GuiSkinState::Disabled,
    ] {
        let rail = look("switch")
            .row(GuiPartId::variant(
                GuiPrimitivePart::Background,
                state,
                GuiPartVariant::Checked,
            ))
            .unwrap();
        assert_eq!(rail.color, Some(srgb(0x00131c)));
        let off = GuiPartId::variant(GuiPrimitivePart::Icon, state, GuiPartVariant::Unchecked);
        assert_eq!(look("switch").row(off).unwrap().align_x, Some(-1.0));
    }
}

#[test]
fn track_points_follow_the_default_bar() {
    // Half the default bar at the looks' em, so a default track's points meet
    // on its centre line at every font size.
    assert_eq!(BAR, GUI_SCROLL_BAR_EMS * GUI_LOOK_EM);
    assert_eq!(pointed_ends(BAR), [BAR / 2.0; 4]);
}

/// The `(duration, easing, exit)` row of one part identity in a look's motion.
fn timing(
    look: &GuiSkinLook,
    identity: GuiPartId,
) -> Option<(Option<f32>, Option<u32>, Option<f32>)> {
    let index = identity.index()?;
    look.motion
        .iter()
        .find(|row| row.part == index)
        .map(|row| (row.duration, row.easing, row.exit))
}

#[test]
fn every_look_declares_the_sheets_transition_rules_as_valid_motion_rows() {
    use crate::systems::gui::motion::GuiThemeMotion;
    use GuiPartVariant::{Checked, Unchecked};
    use GuiPrimitivePart::{Background, Icon, Label};
    use GuiSkinState::{Disabled, Hovered, Idle, Pressed};

    for look in gui_skin_looks() {
        let mut parts = Rows::new();
        for row in &look.motion {
            parts.push(row.clone()).unwrap();
        }
        assert_eq!(
            GuiThemeMotion {
                parts,
            }
            .validate(),
            Ok(()),
            "{}",
            look.name
        );
    }

    // Hover in 80 ms and out 120 ms, an immediate press released in 100 ms,
    // an immediate disable both ways and a 100 ms fill, on every button.
    for name in ["button", "amber", "secondary", "secondaryAmber", "docked"] {
        let look = look(name);
        for part in [Background, Label] {
            assert_eq!(
                timing(look, GuiPartId::base(part)),
                Some((Some(0.12), Some(0), None))
            );
            assert_eq!(
                timing(look, GuiPartId::state(part, Hovered)),
                Some((Some(0.08), Some(0), None))
            );
            assert_eq!(
                timing(look, GuiPartId::state(part, Pressed)),
                Some((Some(0.0), Some(0), Some(0.1)))
            );
            assert_eq!(
                timing(look, GuiPartId::state(part, Disabled)),
                Some((Some(0.0), Some(0), Some(0.0)))
            );
            for state in [Idle, Hovered] {
                for variant in [Checked, Unchecked] {
                    assert_eq!(
                        timing(look, GuiPartId::variant(part, state, variant)),
                        Some((Some(0.1), Some(0), None)),
                        "{name}"
                    );
                }
            }
        }
    }

    // The check mark and every focus ring have no rows: they change at once.
    for look in gui_skin_looks() {
        assert_eq!(
            timing(look, GuiPartId::base(GuiPrimitivePart::FocusRing)),
            None
        );
    }
    assert_eq!(timing(look("checkbox"), GuiPartId::base(Icon)), None);

    // The switch block: 160 ms with an ease-out cubic either way.
    for state in [Idle, Hovered] {
        for variant in [Checked, Unchecked] {
            assert_eq!(
                timing(look("switch"), GuiPartId::variant(Icon, state, variant)),
                Some((Some(0.16), Some(2), None))
            );
        }
    }
}

fn token(name: &str) -> GuiSkinTokenValue {
    gui_skin_tokens()
        .iter()
        .find(|token| token.name == name)
        .unwrap_or_else(|| panic!("no token {name}"))
        .value
}

fn number(name: &str) -> f32 {
    match token(name) {
        GuiSkinTokenValue::Number(value) => value,
        other => panic!("{name} is {other:?}"),
    }
}

fn color(name: &str) -> [f32; 4] {
    match token(name) {
        GuiSkinTokenValue::Color(value) => value,
        other => panic!("{name} is {other:?}"),
    }
}

#[test]
fn tokens_are_the_values_the_looks_and_default_geometry_use() {
    let mut names = BTreeSet::new();
    for token in gui_skin_tokens() {
        assert!(names.insert(token.name), "{} repeats", token.name);
        assert!(token.value.lanes().iter().all(|lane| lane.is_finite()));
    }
    assert_eq!(number("em"), GUI_LOOK_EM);
    assert_eq!(number("textBody"), GUI_LOOK_EM);
    // An unsized dial's square side, five ems.
    assert_eq!(number("dial"), 80.0);

    // The button's idle frame, hover edge and focus ring are made of tokens.
    let button = look("button");
    let idle = button
        .row(GuiPartId::base(GuiPrimitivePart::Background))
        .unwrap();
    assert_eq!(idle.color, Some(color("surface")));
    assert_eq!(idle.border_color, Some(color("neutral")));
    assert_eq!(idle.border_width, Some(number("lineWidth")));
    assert_eq!(
        idle.corner_cut,
        Some([number("cut"), 0.0, number("cut"), 0.0])
    );
    let hover = button
        .row(GuiPartId::state(
            GuiPrimitivePart::Background,
            GuiSkinState::Hovered,
        ))
        .unwrap();
    assert_eq!(hover.border_color, Some(color("accent")));
    assert_eq!(hover.border_width, Some(number("litLineWidth")));
    assert_eq!(hover.glow_intensity, Some(number("hoverGlowIntensity")));
    assert_eq!(hover.glow_radius, Some(number("frameGlowReach")));
    assert_eq!(hover.glow_falloff, Some(number("glowFalloff")));
    let ring = button
        .row(GuiPartId::base(GuiPrimitivePart::FocusRing))
        .unwrap();
    assert_eq!(ring.glow_intensity, Some(number("focusGlowIntensity")));
    let secondary = look("secondary")
        .row(GuiPartId::base(GuiPrimitivePart::Background))
        .unwrap();
    assert_eq!(
        secondary.corner_cut,
        Some([number("partCut"), 0.0, number("partCut"), 0.0])
    );
    let thumb = look("slider")
        .row(GuiPartId::state(
            GuiPrimitivePart::Icon,
            GuiSkinState::Hovered,
        ))
        .unwrap();
    assert_eq!(thumb.glow_radius, Some(number("partGlowReach")));
    let rail = look("slider")
        .row(GuiPartId::base(GuiPrimitivePart::Background))
        .unwrap();
    assert_eq!(
        (rail.color, rail.border_color),
        (Some(color("railFill")), Some(color("line")))
    );
    assert_eq!(
        look("amber")
            .row(GuiPartId::base(GuiPrimitivePart::Background))
            .unwrap()
            .border_color,
        Some(color("amber"))
    );
    assert_eq!(
        look("textInput")
            .row(GuiPartId::base(GuiPrimitivePart::Selection))
            .unwrap()
            .color,
        Some(color("selection"))
    );
    assert_eq!(
        look("button")
            .row(GuiPartId::base(GuiPrimitivePart::Label))
            .unwrap()
            .color,
        Some(color("accent"))
    );
    assert_eq!(
        look("secondary")
            .row(GuiPartId::base(GuiPrimitivePart::Label))
            .unwrap()
            .color,
        Some(color("text"))
    );

    // Unsized controls measure to the size tokens at the looks' em, and the
    // default bar is the bar token.
    assert_eq!(number("controlHeight"), 40.0);
    assert_eq!(number("smallHeight"), 32.0);
    assert_eq!(number("inset"), 16.0);
    assert_eq!(number("bar"), BAR);

    // The tints are role colours at their stated alpha.
    let [r, g, b, _] = color("accent");
    assert_eq!(color("rowTint"), [r, g, b, 0.04]);
    let [r, g, b, _] = color("line");
    assert_eq!(color("railFill"), [r, g, b, 0.15]);
}

#[test]
fn a_numeric_fields_step_parts_mark_their_direction_and_mute_it_at_the_bound() {
    use GuiPrimitivePart::{DecrementMark, IncrementMark};
    use GuiSkinState::{Disabled, Pressed};

    let field = look("textInput");
    let (text, neutral, surface) = (srgb(0xe5f5f7), srgb(0x90b0c4), srgb(0x00131c));
    for (mark, upright) in [
        (DecrementMark, None),
        (IncrementMark, Some([0.5, 0.0, 0.5, 1.0])),
    ] {
        let row = field.row(GuiPartId::base(mark)).unwrap();
        assert_eq!(
            (row.shape, row.color, row.stroke_a),
            (Some(1.0), Some(text), Some([0.0, 0.5, 1.0, 0.5]))
        );
        assert_eq!(row.stroke_b, Some(upright.unwrap_or([0.0; 4])));
        assert_eq!(
            field.row(GuiPartId::state(mark, Disabled)).unwrap().color,
            Some(neutral)
        );
        assert_eq!(
            field.row(GuiPartId::state(mark, Pressed)).unwrap().color,
            Some(surface)
        );
    }
}

#[test]
fn the_color_look_keeps_its_data_square_and_unglowed_and_marks_it_with_outlines() {
    use GuiPrimitivePart::{Fill, FocusRing, Icon, Marker, Track};
    use GuiSkinState::{Disabled, Hovered, Pressed};

    let color_look = look("color");
    let base = |part| color_look.row(GuiPartId::base(part)).unwrap();
    let state = |part, state| color_look.row(GuiPartId::state(part, state)).unwrap();

    // The surfaces and the swatch are uncut data in the quiet line, over the
    // checker their translucent colours show.
    for part in [Track, Fill] {
        let row = base(part);
        assert_eq!(row.corner_cut, None, "{part:?}");
        assert_eq!(
            (row.border_width, row.border_color, row.checker_size),
            (
                Some(number("lineWidth")),
                Some(color("line")),
                Some(number("checker"))
            ),
            "{part:?}"
        );
        // Paint gives the fill from the value.
        assert_eq!((row.color, row.fill_mode), (None, None), "{part:?}");
    }

    // Hover, a press and focus light a surface's edge with glow reaching
    // outward only, so nothing tints the colours.
    for (row, intensity) in [
        (state(Track, Hovered), "hoverGlowIntensity"),
        (state(Track, Pressed), "hoverGlowIntensity"),
        (base(FocusRing), "focusGlowIntensity"),
    ] {
        assert_eq!(row.border_width, Some(number("litLineWidth")));
        assert_eq!(row.glow_intensity, Some(number(intensity)));
        assert_eq!(row.glow_radius, Some(number("frameGlowReach")));
        assert_eq!(row.glow_inner_radius, Some(0.0));
        assert_eq!(row.color.filter(|_| row.part != base(FocusRing).part), None);
    }
    assert_eq!(base(FocusRing).corner_cut, Some([0.0; 4]));

    // The marker is a ring and each thumb a part-cut bar: clear outlines in
    // text with a dark halo in every enabled state, neutral while disabled.
    let marker = base(Marker);
    assert_eq!(marker.corner_radius, Some([6.0; 2]));
    let thumb = base(Icon);
    assert_eq!(
        thumb.corner_cut,
        Some([number("partCut"), 0.0, number("partCut"), 0.0])
    );
    for part in [Marker, Icon] {
        let row = base(part);
        assert_eq!(
            (row.color, row.border_color, row.glow_color),
            (Some([0.0; 4]), Some(color("text")), Some(color("surface"))),
            "{part:?}"
        );
        assert_eq!(row.glow_inner_radius, Some(0.0), "{part:?}");
        for each in [Hovered, Pressed] {
            assert!(color_look.row(GuiPartId::state(part, each)).is_none());
        }
        assert_eq!(state(part, Disabled).border_color, Some(color("neutral")));
    }

    // Disabled dims the surfaces and keeps the swatch's colour.
    assert_eq!(state(Track, Disabled).opacity, Some(0.4));
    assert!(color_look.row(GuiPartId::state(Fill, Disabled)).is_none());
}
