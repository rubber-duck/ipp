//! Authored during refinement; execution waits for the combined integration gate.

use super::*;

#[test]
fn facing_retains_composed_anchor_scale_and_screen_direction() {
    let local = [
        -2.0, 0.0, 0.0, 0.0, 0.0, -3.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 7.0, 11.0, 13.0, 1.0,
    ];
    // A camera rotated 90 degrees about +Y, including irrelevant translation.
    let camera = [
        0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 20.0, 21.0, 22.0, 1.0,
    ];
    let faced = model(local, PlotPlaneFacing::Camera, camera).unwrap();
    assert_eq!(&faced[12..], &local[12..]);
    assert_eq!(&faced[..3], &[0.0, 0.0, -2.0]);
    assert_eq!(&faced[4..7], &[0.0, -3.0, 0.0]);
    assert_eq!(model(local, PlotPlaneFacing::Fixed, camera).unwrap(), local);
}

#[test]
fn roll_is_preserved_and_camera_edits_do_not_move_the_anchor() {
    let local = [
        1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 3.0, 4.0, 5.0, 1.0,
    ];
    let rolled = [
        0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let faced = model(local, PlotPlaneFacing::Camera, rolled).unwrap();
    assert_eq!(&faced[12..], &local[12..]);
    assert_eq!(&faced[..3], &[0.0, 1.0, 0.0]);
    assert_eq!(&faced[4..7], &[1.0, 0.0, 0.0]);
    let mut invalid = rolled;
    invalid[..3].fill(0.0);
    assert!(model(local, PlotPlaneFacing::Camera, invalid).is_err());
}
