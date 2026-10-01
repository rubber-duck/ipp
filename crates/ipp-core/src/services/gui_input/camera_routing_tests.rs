//! Ported legacy projected routing: panels presented on Surfaces in a Camera
//! root, pointer selection by view distance, captured projection, viewport
//! mapping, camera-only motion and keyboard entry by camera view order.

use super::query::{GuiQueryOptions, GuiQueryOutcome, query_composed_input};
use super::router_test_support::*;
use super::*;
use crate::components::{Camera, GuiCheckbox, GuiSlider, Transform};
use crate::services::gui_input::router::*;

/// A Camera root World at `z = 10` holding `build`'s panels, presented at
/// `800 x 800` pixels.
fn camera_scene(
    lens: Camera,
    build: impl FnOnce(&mut HostRuntime, WorldRef) -> Vec<Panel>,
) -> (Rig, Vec<Panel>) {
    let (mut host, _) = host();
    let world = scene_world(&mut host);
    let root = camera_root(&mut host, world, lens);
    let panels = build(&mut host, world);
    (Rig::new(host, root, viewport(800, 800)), panels)
}

fn checkbox() -> ComponentValue {
    ComponentValue::GuiCheckbox(GuiCheckbox::default())
}

/// Near panel at `z = 5` over a far one at the origin. The far panel is
/// created first, so the nearer panel has the greater entity.
fn near_and_far(lens: Camera, near: Transform, control: ComponentValue) -> (Rig, Panel, Panel) {
    let (rig, mut panels) = camera_scene(lens, |host, world| {
        let far = panel(host, world, at(0.0, 0.0, 0.0), checkbox());
        let near = panel(host, world, near, control);
        vec![far, near]
    });
    assert!(panels[0].anchor < panels[1].anchor);
    let near = panels.pop().unwrap();
    let far = panels.pop().unwrap();
    (rig, near, far)
}

/// Output and Canvas-logical point the composed query selects.
fn hit_at(rig: &Rig, point: [f32; 2]) -> Option<(OutputRef, [f32; 2])> {
    match query_composed_input(&rig.host, rig.query(), point, GuiQueryOptions::default())
        .unwrap()
        .outcome
    {
        GuiQueryOutcome::Hit(hit) => Some((hit.output, hit.point)),
        _ => None,
    }
}

fn assert_near(actual: [f32; 2], expected: [f32; 2]) {
    for axis in 0..2 {
        assert!(
            (actual[axis] - expected[axis]).abs() < 1e-3,
            "{actual:?} != {expected:?}"
        );
    }
}

fn tap(rig: &mut Rig, pointer: u64, point: [f32; 2]) -> GuiRoutingDisposition {
    let disposition = rig.send(press(pointer, point));
    rig.send(release(pointer, point));
    disposition
}

fn checked(rig: &mut Rig, panel: &Panel) -> bool {
    rig.value(panel.world(), panel.controls[0]) == GuiTestValue::Bool(true)
}

#[test]
fn perspective_pointer_selects_the_nearest_panel_not_the_greatest_entity() {
    let (mut rig, near, far) = near_and_far(Camera::default(), at(0.0, 0.0, 5.0), checkbox());
    // The viewport centre meets the z = 5 Surface at its logical centre.
    let (output, point) = hit_at(&rig, [0.5, 0.5]).unwrap();
    assert_eq!(output, near.output);
    assert_near(point, [2.0, 1.5]);
    tap(&mut rig, 1, [0.5, 0.5]);
    assert!(checked(&mut rig, &near));
    assert!(!checked(&mut rig, &far));
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

#[test]
fn a_perspective_off_centre_pixel_maps_through_the_surface() {
    let (mut rig, near, _) = near_and_far(Camera::default(), at(0.0, 0.0, 5.0), checkbox());
    // World (1, 0.5, 5) seen from z = 10 with a 45 degree field of view:
    // tan(22.5 deg) = 0.41421356 and 1 / 5 = 0.2, so the normalized point is
    // ((0.2 / 0.41421356 + 1) / 2, (1 - 0.1 / 0.41421356) / 2). On the 4 x 3
    // Surface that is logical (3, 1).
    let at = [0.741_421_36, 0.379_289_32];
    let (output, point) = hit_at(&rig, at).unwrap();
    assert_eq!(output, near.output);
    assert_near(point, [3.0, 1.0]);
    tap(&mut rig, 1, at);
    assert!(checked(&mut rig, &near));
    rig.finish();
}

#[test]
fn a_captured_slider_clamps_beyond_a_tilted_surface_and_cancel_keeps_its_value() {
    let angle = 20.0_f32.to_radians();
    let (mut rig, near, far) = near_and_far(
        Camera::default(),
        Transform {
            z: 5.0,
            qy: (angle * 0.5).sin(),
            qw: (angle * 0.5).cos(),
            ..Default::default()
        },
        ComponentValue::GuiSlider(GuiSlider {
            value: 0.5,
            min: 0.0,
            max: 1.0,
            step: 0.05,
        }),
    );
    let (world, slider) = (near.world(), near.controls[0]);
    let value = |rig: &mut Rig| rig.value(world, slider);
    // The pointer leaves the viewport past the panel's +X edge: captured
    // motion keeps the panel-space coordinate, so the slider clamps to max.
    rig.send(press(11, [0.5, 0.5]));
    rig.send(movement(11, [1.2, 0.5]));
    assert_eq!(value(&mut rig), GuiTestValue::Scalar(1.0));
    rig.send(release(11, [1.2, 0.5]));
    assert!(!rig.snapshot(world, slider).interaction.captured);
    assert_eq!(value(&mut rig), GuiTestValue::Scalar(1.0));
    // The opposite direction clamps to min and its off-panel release keeps it.
    rig.send(press(11, [0.5, 0.5]));
    rig.send(movement(11, [-0.2, 0.5]));
    rig.send(release(11, [-0.2, 0.5]));
    assert_eq!(value(&mut rig), GuiTestValue::Scalar(0.0));
    // Cancellation ends the capture without reverting or adding a commit.
    rig.send(press(11, [0.5, 0.5]));
    rig.send(movement(11, [1.2, 0.5]));
    let commits = rig.commits().len();
    rig.send(GuiPhysicalInput::PointerCancel {
        pointer: 11,
    });
    rig.send(movement(11, [-0.2, 0.5]));
    let snapshot = rig.snapshot(world, slider);
    assert!(!snapshot.interaction.pressed && !snapshot.interaction.captured);
    assert_eq!(snapshot.value, GuiTestValue::Scalar(1.0));
    assert_eq!(rig.commits().len(), commits);
    assert!(!checked(&mut rig, &far));
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

#[test]
fn orthographic_pointer_selects_the_nearest_panel() {
    let (mut rig, near, far) = near_and_far(
        Camera {
            projection: 1,
            ortho_height: 4.0,
            ..Default::default()
        },
        at(0.0, 0.0, 5.0),
        checkbox(),
    );
    // Half-extent 2 at aspect 1: world (1, 0.5) is ((1 + 2) / 4, (2 - 0.5) / 4).
    let at = [0.75, 0.375];
    let (output, point) = hit_at(&rig, at).unwrap();
    assert_eq!(output, near.output);
    assert_near(point, [3.0, 1.0]);
    tap(&mut rig, 1, at);
    assert!(checked(&mut rig, &near));
    assert!(!checked(&mut rig, &far));
    rig.finish();
}

#[test]
fn a_back_facing_panel_never_wins() {
    // The nearer panel turns its front away; the ray meets its back first and
    // falls through to the farther panel.
    let (mut rig, near, far) = near_and_far(Camera::default(), turned(0.0, 0.0, 5.0), checkbox());
    let (output, point) = hit_at(&rig, [0.5, 0.5]).unwrap();
    assert_eq!(output, far.output);
    assert_near(point, [2.0, 1.5]);
    tap(&mut rig, 1, [0.5, 0.5]);
    assert!(checked(&mut rig, &far));
    assert!(!checked(&mut rig, &near));
    rig.finish();
}

#[test]
fn viewport_density_and_aspect_keep_the_viewport_mapping() {
    let (mut rig, near, _) = near_and_far(Camera::default(), at(0.0, 0.0, 5.0), checkbox());
    // Doubled density at the same aspect maps identical normalized points.
    rig.present(WorldViewport {
        width: 1600,
        height: 1600,
        device_pixel_ratio: 2.0,
    });
    let (output, point) = hit_at(&rig, [0.741_421_36, 0.379_289_32]).unwrap();
    assert_eq!(output, near.output);
    assert_near(point, [3.0, 1.0]);
    // Aspect 2 halves the horizontal extent: world x = 1 sits at
    // (0.2 / 0.82842712 + 1) / 2.
    rig.present(viewport(800, 400));
    let at = [0.620_710_7, 0.379_289_3];
    let (output, point) = hit_at(&rig, at).unwrap();
    assert_eq!(output, near.output);
    assert_near(point, [3.0, 1.0]);
    tap(&mut rig, 1, at);
    assert!(checked(&mut rig, &near));
    rig.finish();
}

#[test]
fn camera_only_motion_retargets_without_reflow() {
    let (mut rig, near, far) = near_and_far(Camera::default(), at(0.0, 0.0, 5.0), checkbox());
    let revisions = |rig: &Rig| {
        [&near, &far].map(|panel| {
            let canvas = rig.canvas(panel.output);
            (canvas.layout_revision, canvas.paint_revision)
        })
    };
    let before = revisions(&rig);
    // Moving the camera aside swings the centre ray clear of both panels.
    let camera = rig.root;
    place_at(
        &mut rig.host,
        camera.world(),
        camera.camera_entity().unwrap(),
        at(20.0, 0.0, 10.0),
    );
    rig.frame();
    assert_eq!(revisions(&rig), before);
    assert_eq!(hit_at(&rig, [0.5, 0.5]), None);
    assert_eq!(tap(&mut rig, 1, [0.5, 0.5]), GuiRoutingDisposition::Miss);
    assert!(!checked(&mut rig, &near) && !checked(&mut rig, &far));
    rig.finish();
}

#[test]
fn a_panel_keeps_presses_on_content_without_a_control_from_the_scene() {
    let (mut rig, panels) = camera_scene(
        Camera {
            projection: 1,
            ortho_height: 4.0,
            ..Default::default()
        },
        |host, world| vec![column_panel(host, world, at(0.0, 0.0, 5.0), 1)],
    );
    let panel = &panels[0];
    // Half-extent 2 at aspect 1: logical (x, y) on the 4 x 3 panel sits at
    // (x / 4, (y + 0.5) / 4). The one checkbox fills logical rows [0, 1).
    let (control, content, beside) = ([0.5, 0.25], [0.5, 0.625], [0.5, 0.95]);
    assert_eq!(hit_at(&rig, content), None);
    // The panel is opaque to the scene for every button.
    for (pointer, button) in [
        (1, GuiPhysicalButton::Primary),
        (2, GuiPhysicalButton::Secondary),
        (3, GuiPhysicalButton::Auxiliary),
    ] {
        let down = GuiPhysicalInput::PointerDown {
            button,
            pointer,
            point: content,
        };
        assert_eq!(
            rig.send(down),
            GuiRoutingDisposition::Blocked,
            "{button:?} press on panel content"
        );
        rig.send(GuiPhysicalInput::PointerUp {
            button,
            pointer,
            point: content,
        });
    }
    assert!(!checked(&mut rig, panel));
    // Beside the panel the press reaches the scene, and an unconsumed wheel
    // over the panel still does.
    assert_eq!(tap(&mut rig, 4, beside), GuiRoutingDisposition::Miss);
    assert_eq!(
        rig.send(wheel(content, [0.0, 1.0])),
        GuiRoutingDisposition::Unhandled
    );
    // The control on the same panel still takes its press.
    assert!(matches!(
        tap(&mut rig, 5, control),
        GuiRoutingDisposition::Routed { .. }
    ));
    assert!(checked(&mut rig, panel));
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

/// Keyboard walk over column panels, reporting `(panel, control)` indices.
fn focus_of(rig: &mut Rig, panels: &[Panel]) -> Option<(usize, usize)> {
    let targets: Vec<_> = panels.iter().flat_map(Panel::targets).collect();
    let focused = rig.focused(&targets)?;
    panels.iter().enumerate().find_map(|(index, panel)| {
        panel
            .targets()
            .iter()
            .position(|target| *target == focused)
            .map(|control| (index, control))
    })
}

fn walk(rig: &mut Rig, panels: &[Panel], keys: &[GuiPhysicalKey]) -> Vec<Option<(usize, usize)>> {
    keys.iter()
        .map(|&pressed| {
            rig.send(key(pressed));
            focus_of(rig, panels)
        })
        .collect()
}

use GuiPhysicalKey::{BackTab, Escape, Tab};

#[test]
fn keyboard_entry_takes_the_nearest_front_facing_panel_and_follows_the_camera() {
    let (mut rig, panels) = camera_scene(Camera::default(), |host, world| {
        vec![
            column_panel(host, world, at(-6.0, 0.0, 0.0), 2),
            column_panel(host, world, at(6.0, 0.0, 0.0), 2),
        ]
    });
    // Equidistant panels tie on the lesser anchor entity, as pointer routing
    // breaks panel ties.
    assert!(panels[0].anchor < panels[1].anchor);
    assert_eq!(
        walk(&mut rig, &panels, &[Tab, Escape, Tab]),
        [Some((0, 0)), None, Some((0, 0))]
    );
    // Over the right panel, it becomes the entry panel for both directions.
    let camera = rig.root;
    place_at(
        &mut rig.host,
        camera.world(),
        camera.camera_entity().unwrap(),
        at(6.0, 0.0, 10.0),
    );
    rig.frame();
    assert_eq!(
        walk(&mut rig, &panels, &[Escape, Tab, Escape, BackTab]),
        [None, Some((1, 0)), None, Some((1, 1))]
    );
    // The most recently focused panel is no entry hint.
    assert_eq!(
        walk(&mut rig, &panels, &[Tab, Escape, Tab]),
        [Some((0, 0)), None, Some((1, 0))]
    );
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

#[test]
fn an_orthographic_view_orders_panels_by_depth_along_the_view_direction() {
    // Off-axis panels: depth, not eye distance, decides an orthographic view.
    let (mut rig, panels) = camera_scene(
        Camera {
            projection: 1,
            ortho_height: 4.0,
            ..Default::default()
        },
        |host, world| {
            vec![
                column_panel(host, world, turned(0.0, 0.0, 8.0), 2),
                column_panel(host, world, at(0.0, 0.0, 4.0), 2),
                column_panel(host, world, at(30.0, 0.0, 6.0), 2),
            ]
        },
    );
    let (back, deeper, shallower) = (0, 1, 2);
    assert_eq!(
        walk(&mut rig, &panels, &[Tab, Tab, Tab, Tab, Tab]),
        [
            Some((shallower, 0)),
            Some((shallower, 1)),
            Some((deeper, 0)),
            Some((deeper, 1)),
            Some((back, 0)),
        ]
    );
    rig.finish();
}

#[test]
fn back_facing_panels_follow_every_front_facing_panel_in_traversal() {
    // A back-facing panel 2 m from the camera, a front-facing one 10 m away
    // and one 6 m away; their entity order differs from the view order.
    let (mut rig, panels) = camera_scene(Camera::default(), |host, world| {
        vec![
            column_panel(host, world, turned(0.0, 0.0, 8.0), 2),
            column_panel(host, world, at(0.0, 0.0, 0.0), 2),
            column_panel(host, world, at(0.0, 0.0, 4.0), 2),
        ]
    });
    let (back, far, near) = (0, 1, 2);
    assert_eq!(
        walk(&mut rig, &panels, &[Tab, Escape, BackTab]),
        [Some((near, 0)), None, Some((near, 1))]
    );
    // Continued Tab crosses the front-facing panels by distance, then reaches
    // the back-facing one before wrapping; BackTab reverses it.
    assert_eq!(
        walk(&mut rig, &panels, &[Tab, Tab, Tab, Tab, Tab, Tab]),
        [
            Some((far, 0)),
            Some((far, 1)),
            Some((back, 0)),
            Some((back, 1)),
            Some((near, 0)),
            Some((near, 1)),
        ]
    );
    assert_eq!(
        walk(&mut rig, &panels, &[Tab, BackTab, BackTab, BackTab]),
        [
            Some((far, 0)),
            Some((near, 1)),
            Some((near, 0)),
            Some((back, 1))
        ]
    );
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}
