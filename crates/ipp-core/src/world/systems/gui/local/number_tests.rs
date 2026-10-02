use super::*;

fn input(value: f32) -> GuiTextInput {
    GuiTextInput {
        numeric: true,
        value,
        min: -4.0,
        max: 4.0,
        step: 0.25,
        fine_step: 0.05,
        precision: 2,
        step_parts: true,
        ..Default::default()
    }
}

#[test]
fn plain_decimal_numbers_parse_and_nothing_else_does() {
    for (text, expected) in [
        ("1.25", 1.25),
        ("-4", -4.0),
        ("+2.5", 2.5),
        (" 7 ", 7.0),
        (".5", 0.5),
        ("5.", 5.0),
        ("-0", 0.0),
        ("0012", 12.0),
    ] {
        assert_eq!(parse_number(text), Some(expected), "{text}");
    }
    for text in [
        "", " ", "-", "+", ".", "-.", "1.2.3", "1e3", "1E-2", "1,5", "1 000", "--1", "+-1", "inf",
        "NaN", "0x10", "1/2", "2+2", "١٢",
    ] {
        assert_eq!(parse_number(text), None, "{text}");
    }
}

#[test]
fn numbers_format_at_their_precision_without_a_signed_zero() {
    assert_eq!(&*format_number(1.25, 2), "1.25");
    assert_eq!(&*format_number(4.0, 2), "4.00");
    assert_eq!(&*format_number(1.255, 0), "1");
    assert_eq!(&*format_number(-0.001, 2), "0.00");
    assert_eq!(&*format_number(-0.0, 0), "0");
    assert_eq!(&*format_number(-1.5, 1), "-1.5");
}

#[test]
fn commits_clamp_to_the_range_without_snapping_to_the_step() {
    let number = input(1.25);
    assert_eq!(number.committed("9"), Some(4.0));
    assert_eq!(number.committed("-1e40"), None);
    assert_eq!(
        number.committed("-99999999999999999999999999999999999999999"),
        Some(-4.0)
    );
    assert_eq!(number.committed("1.3"), Some(1.3));
    assert_eq!(
        number.committed("-0").map(f32::is_sign_negative),
        Some(false)
    );
    let unbounded = GuiTextInput {
        numeric: true,
        ..Default::default()
    };
    assert_eq!(unbounded.committed("1e3"), None);
    assert_eq!(unbounded.committed("123456"), Some(123_456.0));
}

#[test]
fn each_step_part_is_disabled_at_its_bound_and_both_without_a_step() {
    assert_eq!(input(1.25).step_enabled(), [true, true]);
    assert_eq!(input(4.0).step_enabled(), [true, false]);
    assert_eq!(input(-4.0).step_enabled(), [false, true]);
    let still = GuiTextInput {
        step: 0.0,
        ..input(1.25)
    };
    assert_eq!(still.step_enabled(), [false, false]);
}

#[test]
fn steps_commit_a_pending_edit_first_and_a_part_at_its_bound_does_nothing() {
    let step = |part: Option<GuiNumberStep>, edit: Option<&str>, value| {
        let edit = edit.map(Arc::<str>::from);
        let outcome = input(value).number_outcome(
            GuiNumberOperation::Step {
                steps: part.map_or(1.0, GuiNumberStep::steps),
                fine: false,
                part,
            },
            edit.as_ref(),
        );
        (
            outcome.value,
            outcome.rejected.map(|text| text.to_string()),
            outcome.resets,
            outcome.repeats,
        )
    };
    let up = Some(GuiNumberStep::Increment);
    assert_eq!(step(up, None, 1.25), (1.5, None, true, true));
    assert_eq!(step(up, Some("2"), 1.25), (2.25, None, true, true));
    assert_eq!(
        step(up, Some("x"), 1.25),
        (1.5, Some("x".into()), true, true)
    );
    assert_eq!(step(up, Some("3.9"), 1.25), (4.0, None, true, true));
    assert_eq!(step(up, Some("4"), 1.25), (4.0, None, true, false));
    assert_eq!(step(up, Some("2"), 4.0), (4.0, None, false, false));
    assert_eq!(step(None, Some("2"), 4.0), (2.25, None, true, false));
}

#[test]
fn step_parts_are_squares_of_the_height_at_the_ends_that_never_overlap() {
    assert_eq!(
        number_step_rects([196.0, 40.0]),
        [[0.0, 0.0, 40.0, 40.0], [156.0, 0.0, 40.0, 40.0]]
    );
    assert_eq!(
        number_step_rects([60.0, 40.0]),
        [[0.0, 0.0, 30.0, 40.0], [30.0, 0.0, 30.0, 40.0]]
    );
}
