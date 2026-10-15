use super::*;

fn at(x: f32, y: f32) -> GuiCanvasTransform {
    GuiCanvasTransform {
        position: [x, y],
        scale: [1.0, 1.0],
    }
}

#[test]
fn popup_shares_anchor_sheet_below_and_after_flipping_above() {
    assert_eq!(
        joined_rects(at(20.0, 40.0), [200.0, 32.0], at(20.0, 72.0), [200.0, 96.0]),
        Some(([0.0, 0.0, 200.0, 128.0], [0.0, -32.0, 200.0, 128.0]))
    );
    assert_eq!(
        joined_rects(
            at(20.0, 140.0),
            [200.0, 32.0],
            at(20.0, 44.0),
            [200.0, 96.0]
        ),
        Some(([0.0, -96.0, 200.0, 128.0], [0.0, 0.0, 200.0, 128.0]))
    );
}

#[test]
fn scaled_and_mirrored_sheets_keep_local_coordinates() {
    let a = GuiCanvasTransform {
        position: [420.0, 10.0],
        scale: [-2.0, 2.0],
    };
    let b = GuiCanvasTransform {
        position: [420.0, 74.0],
        scale: [-2.0, 2.0],
    };
    assert_eq!(
        joined_rects(a, [200.0, 32.0], b, [200.0, 70.0]),
        Some(([0.0, 0.0, 200.0, 102.0], [0.0, -32.0, 200.0, 102.0]))
    );
}

#[test]
fn gaps_overlap_shift_and_incompatible_scale_do_not_join() {
    let a = at(20.0, 40.0);
    for b in [
        at(20.0, 75.0),
        at(20.0, 70.0),
        at(19.0, 72.0),
        GuiCanvasTransform {
            position: [20.0, 72.0],
            scale: [0.5, 1.0],
        },
    ] {
        assert_eq!(joined_rects(a, [200.0, 32.0], b, [200.0, 96.0]), None);
    }
}
