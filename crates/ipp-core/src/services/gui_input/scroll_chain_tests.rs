//! Ported legacy scroll routing inside one Canvas World: wheel chains, clamped
//! and unconsumed movement, revealed and clipped hits, content drags against
//! taps and sliders, scroll-bar thumbs, track paging and VirtualList bars.

use super::router_test_support::*;
use super::*;
use crate::components::{GuiCheckbox, GuiScrollView, GuiSlider, GuiVirtualList};
use crate::services::gui_input::router::*;
use crate::systems::canvas::{CanvasAxis, CanvasHitKind};

/// A column-laid entity with an explicit logical box and extra components.
fn node(
    host: &mut HostRuntime,
    world: WorldRef,
    parent: EntityId,
    kind: u32,
    size: [f32; 2],
    extra: Vec<ComponentValue>,
) -> EntityId {
    let mut values = vec![sized(kind, size[0], size[1])];
    values.extend(extra);
    create(host, world, values, Some(parent))
}

fn scroll_view() -> Vec<ComponentValue> {
    vec![ComponentValue::GuiScrollView(GuiScrollView::default())]
}

fn checkbox() -> Vec<ComponentValue> {
    vec![ComponentValue::GuiCheckbox(GuiCheckbox::default())]
}

/// One `10 x 10` Canvas World, presented at ten pixels per unit:
///
/// ```text
/// root Column
/// └── outer ScrollView 10 x 6            capacity 4
///     └── Column 10 x 10
///         ├── inner ScrollView 10 x 4    capacity 4
///         │   └── Column 10 x 8
///         │       ├── spacer 10 x 4
///         │       ├── checkbox 10 x 1    inner content y 4..5
///         │       └── spacer 10 x 3
///         └── filler 10 x 6
/// ```
struct Nested {
    rig: Rig,
    world: WorldRef,
    outer: EntityId,
    inner: EntityId,
    checkbox: EntityId,
}

impl Nested {
    fn new() -> Self {
        let (mut host, _) = host();
        let world = world(&mut host);
        let (root, root_entity) = canvas_root(&mut host, world, 10.0, 10.0);
        let outer = node(&mut host, world, root_entity, 0, [10.0, 6.0], scroll_view());
        let outer_content = node(&mut host, world, outer, 2, [10.0, 10.0], Vec::new());
        let inner = node(
            &mut host,
            world,
            outer_content,
            0,
            [10.0, 4.0],
            scroll_view(),
        );
        let inner_content = node(&mut host, world, inner, 2, [10.0, 8.0], Vec::new());
        node(&mut host, world, inner_content, 0, [10.0, 4.0], Vec::new());
        let checkbox = node(&mut host, world, inner_content, 0, [10.0, 1.0], checkbox());
        node(&mut host, world, inner_content, 0, [10.0, 3.0], Vec::new());
        node(&mut host, world, outer_content, 0, [10.0, 6.0], Vec::new());
        Self {
            rig: Rig::new(host, root, viewport(100, 100)),
            world,
            outer,
            inner,
            checkbox,
        }
    }

    /// `(inner, outer)` vertical offsets.
    fn offsets(&mut self) -> [f32; 2] {
        [
            self.rig.scroll(self.world, self.inner)[1],
            self.rig.scroll(self.world, self.outer)[1],
        ]
    }

    fn checked(&mut self) -> bool {
        self.rig.value(self.world, self.checkbox) == GuiTestValue::Bool(true)
    }

    /// Wheel at a logical point and apply it; returns the unconsumed remainder.
    fn wheel(&mut self, point: [f32; 2], delta: f32) -> [f32; 2] {
        self.rig.send(wheel(self.rig.logical(point), [0.0, delta]));
        self.rig.wheel_remainder()
    }

    fn tap(&mut self, pointer: u64, point: [f32; 2]) {
        let point = self.rig.logical(point);
        self.rig.send(press(pointer, point));
        self.rig.send(release(pointer, point));
    }

    fn pointer(&mut self, input: fn(u64, [f32; 2]) -> GuiPhysicalInput, id: u64, point: [f32; 2]) {
        let point = self.rig.logical(point);
        self.rig.send(input(id, point));
    }

    /// Published logical bounds of one scroll-bar part of `entity`.
    fn bar(&self, entity: EntityId, thumb: bool) -> [f32; 4] {
        self.rig
            .canvas(self.rig.root)
            .hits
            .iter()
            .find(|hit| {
                hit.target.entity == entity
                    && match hit.kind {
                        CanvasHitKind::ScrollThumb {
                            axis: CanvasAxis::Vertical,
                        } => thumb,
                        CanvasHitKind::ScrollTrack {
                            axis: CanvasAxis::Vertical,
                        } => !thumb,
                        _ => false,
                    }
            })
            .map(|hit| hit.bounds)
            .expect("published vertical scroll-bar part")
    }

    fn finish(self) {
        assert!(self.rig.rejected().is_empty(), "{:?}", self.rig.rejected());
        self.rig.finish();
    }
}

/// Over the inner viewport, where the chain is inner then outer.
const INNER: [f32; 2] = [5.0, 1.0];

#[test]
fn a_wheel_over_content_without_a_scroll_view_is_unhandled_without_phantom_scroll() {
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas_root(&mut host, world, 10.0, 10.0);
    let checkbox = node(&mut host, world, root_entity, 0, [10.0, 2.0], checkbox());
    let mut rig = Rig::new(host, root, viewport(100, 100));
    let before = (
        rig.canvas(root).layout_revision,
        rig.canvas(root).paint_revision,
    );
    let at = rig.point_in(checkbox, [0.5, 0.5]);
    assert_eq!(
        rig.send(wheel(at, [0.0, 5.0])),
        GuiRoutingDisposition::Unhandled
    );
    assert!(terminals(&rig.ledger).is_empty());
    assert_eq!(rig.canvas(root).layout_revision, before.0);
    // A routed toggle there refreshes paint without geometry work.
    rig.send(press(1, at));
    rig.send(release(1, at));
    assert_eq!(rig.commits(), [(checkbox, true)]);
    assert_eq!(rig.canvas(root).layout_revision, before.0);
    assert!(rig.canvas(root).paint_revision > before.1);
    rig.finish();
}

#[test]
fn nested_scroll_views_in_one_canvas_consume_innermost_first_and_clamp() {
    let mut nested = Nested::new();
    assert_eq!(nested.wheel(INNER, 6.0), [0.0, 0.0]);
    assert_eq!(nested.offsets(), [4.0, 2.0]);
    // Both views reach their ends; the leftover is reported unconsumed.
    assert_eq!(nested.wheel(INNER, 10.0), [0.0, 8.0]);
    assert_eq!(nested.offsets(), [4.0, 4.0]);
    // Clamped at both edges, a further wheel consumes nothing. The legacy
    // lane also published no scroll change here; the ordinary lane still
    // commits a new revision (reported against gui/local, not asserted).
    assert_eq!(nested.wheel(INNER, 10.0), [0.0, 10.0]);
    assert_eq!(nested.offsets(), [4.0, 4.0]);
    nested.finish();
}

#[test]
fn wheel_movement_no_scroll_view_consumes_is_reported_unconsumed_at_every_edge() {
    let mut nested = Nested::new();
    // Toward the start at rest neither view can consume.
    assert_eq!(nested.wheel(INNER, -3.0), [0.0, -3.0]);
    assert_eq!(nested.offsets(), [0.0, 0.0]);
    // Partial consumption reports only the remainder past the outer edge.
    assert_eq!(nested.wheel(INNER, 20.0), [0.0, 12.0]);
    // At both far edges the same wheel is wholly unconsumed again.
    assert_eq!(nested.wheel([5.0, 5.0], 1.0), [0.0, 1.0]);
    nested.finish();
}

#[test]
fn pipelined_wheels_chain_outward_like_separately_applied_ones() {
    let mut pipelined = Nested::new();
    // Two wheels routed against one completed frame, each exactly the inner
    // capacity: the second passes outward instead of re-consuming it.
    for _ in 0..2 {
        let at = pipelined.rig.logical(INNER);
        pipelined.rig.route(wheel(at, [0.0, 4.0])).unwrap();
    }
    pipelined.rig.frame();
    let mut separate = Nested::new();
    separate.wheel(INNER, 4.0);
    separate.wheel(INNER, 4.0);
    assert_eq!(pipelined.offsets(), [4.0, 4.0]);
    assert_eq!(separate.offsets(), pipelined.offsets());
    // A later reversal over the outer content consumes against the
    // committed offsets: only the outer view is under the point now.
    assert_eq!(pipelined.wheel(INNER, -1.0), [0.0, 0.0]);
    assert_eq!(pipelined.offsets(), [4.0, 3.0]);
    pipelined.finish();
    separate.finish();
}

#[test]
fn scrolled_content_reveals_a_clipped_target_for_hits() {
    let mut nested = Nested::new();
    // Below the inner viewport the checkbox is clipped away: a tap where it
    // would lie meets the outer content instead.
    nested.tap(1, [5.0, 4.5]);
    // Over the inner viewport the tap meets plain content: no control.
    nested.tap(2, [5.0, 0.5]);
    assert!(!nested.checked());
    // Scrolling the inner view by its capacity brings the checkbox to the
    // viewport top, where the same tap reaches it exactly once.
    nested.wheel(INNER, 4.0);
    nested.tap(3, [5.0, 0.5]);
    assert!(nested.checked());
    assert_eq!(nested.rig.commits(), [(nested.checkbox, true)]);
    nested.finish();
}

#[test]
fn a_nested_clip_follows_the_outer_scroll_for_hits() {
    let mut nested = Nested::new();
    // Scroll only the outer view by 2, over its filler.
    nested.wheel([5.0, 5.0], 2.0);
    assert_eq!(nested.offsets(), [0.0, 2.0]);
    // The inner viewport now spans y -2..2, so the checkbox at y 2..3 lies
    // below it: a tap there reaches no checkbox.
    nested.tap(1, [5.0, 2.5]);
    assert!(!nested.checked());
    // Scrolling the inner view by 2 lifts the checkbox into the moved
    // viewport, where the same kind of tap now reaches it.
    nested.wheel([5.0, 1.0], 2.0);
    assert_eq!(nested.offsets(), [2.0, 2.0]);
    nested.tap(2, [5.0, 0.5]);
    assert!(nested.checked());
    nested.finish();
}

#[test]
fn a_content_drag_scrolls_innermost_first_passes_travel_outward_and_reverses() {
    let mut nested = Nested::new();
    nested.pointer(press, 5, [5.0, 3.0]);
    nested.pointer(movement, 5, [5.0, 2.0]);
    nested.pointer(movement, 5, [5.0, 1.0]);
    // Two units of upward travel scroll the inner view by two.
    assert_eq!(nested.offsets(), [2.0, 0.0]);
    // Further travel fills the inner capacity and passes the unused remainder
    // outward; the rest clamps away.
    nested.pointer(movement, 5, [5.0, -3.0]);
    nested.pointer(movement, 5, [5.0, -9.0]);
    assert_eq!(nested.offsets(), [4.0, 4.0]);
    // Reversing scrolls back innermost-first; the release completes nothing.
    nested.pointer(movement, 5, [5.0, -8.0]);
    nested.pointer(release, 5, [5.0, -8.0]);
    assert_eq!(nested.offsets(), [3.0, 4.0]);
    // The drag ended: later moves of the same pointer only hover.
    nested.pointer(movement, 5, [5.0, 0.0]);
    assert_eq!(nested.offsets(), [3.0, 4.0]);
    assert!(!nested.checked());
    nested.finish();
}

#[test]
fn travel_within_the_slop_keeps_the_tap() {
    let mut nested = Nested::new();
    nested.wheel(INNER, 4.0);
    // The checkbox sits at the viewport top; a press that wobbles within the
    // slop (1% of the viewport) still completes the tap.
    nested.pointer(press, 1, [5.0, 0.5]);
    nested.pointer(movement, 1, [5.0, 0.55]);
    nested.pointer(release, 1, [5.0, 0.55]);
    assert!(nested.checked());
    assert_eq!(nested.offsets(), [4.0, 0.0]);
    nested.finish();
}

#[test]
fn a_drag_starting_on_a_checkbox_scrolls_and_commits_nothing() {
    let mut nested = Nested::new();
    nested.wheel(INNER, 4.0);
    nested.pointer(press, 1, [5.0, 0.5]);
    assert!(
        nested
            .rig
            .snapshot(nested.world, nested.checkbox)
            .interaction
            .pressed
    );
    // Dragging down past the slop wins the gesture: the tap disarms and the
    // content follows the pointer back toward the start.
    nested.pointer(movement, 1, [5.0, 2.5]);
    assert!(
        !nested
            .rig
            .snapshot(nested.world, nested.checkbox)
            .interaction
            .pressed
    );
    nested.pointer(release, 1, [5.0, 2.5]);
    assert!(!nested.checked());
    assert!(nested.rig.commits().is_empty());
    assert_eq!(nested.offsets(), [2.0, 0.0]);
    nested.finish();
}

#[test]
fn a_slider_keeps_its_capture_while_another_pointer_drags_the_content() {
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas_root(&mut host, world, 10.0, 10.0);
    let view = node(&mut host, world, root_entity, 0, [10.0, 4.0], scroll_view());
    let content = node(&mut host, world, view, 2, [10.0, 9.0], Vec::new());
    let slider = node(
        &mut host,
        world,
        content,
        0,
        [10.0, 1.0],
        vec![ComponentValue::GuiSlider(GuiSlider {
            value: 0.0,
            min: 0.0,
            max: 1.0,
            step: 0.0,
        })],
    );
    node(&mut host, world, content, 0, [10.0, 8.0], Vec::new());
    let mut rig = Rig::new(host, root, viewport(100, 100));
    let at = |rig: &Rig, x: f32, y: f32| rig.logical([x, y]);
    // The slider keeps its capture across vertical travel: no scroll.
    rig.send(press(1, at(&rig, 1.0, 0.5)));
    rig.send(movement(1, at(&rig, 5.0, 2.5)));
    // A second pointer drags plain content meanwhile.
    rig.send(press(2, at(&rig, 5.0, 3.5)));
    rig.send(movement(2, at(&rig, 5.0, 1.5)));
    rig.send(release(1, at(&rig, 5.0, 2.5)));
    rig.send(release(2, at(&rig, 5.0, 1.5)));
    assert_eq!(rig.value(world, slider), GuiTestValue::Scalar(0.5));
    assert_eq!(rig.scroll(world, view), [0.0, 2.0]);
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

#[test]
fn a_thumb_drag_holds_its_capture_and_clamps_without_passing_movement_outward() {
    let mut nested = Nested::new();
    let thumb = nested.bar(nested.outer, true);
    let track = nested.bar(nested.outer, false);
    let x = (thumb[0] + thumb[2]) * 0.5;
    let grab = thumb[1] + 1.0;
    nested.pointer(press, 1, [x, grab]);
    assert_eq!(nested.offsets(), [0.0, 0.0]);
    // Half the thumb travel scrolls half the capacity, and the capture holds
    // while the pointer wanders off the bar over content.
    let travel = (track[3] - track[1]) - (thumb[3] - thumb[1]);
    nested.pointer(movement, 1, [5.0, grab + travel * 0.5]);
    let offsets = nested.offsets();
    assert_eq!(offsets[0], 0.0);
    assert!((offsets[1] - 2.0).abs() < 1e-4, "{offsets:?}");
    // Past the track end the thumb clamps instead of passing movement outward.
    nested.pointer(movement, 1, [5.0, 40.0]);
    assert_eq!(nested.offsets(), [0.0, 4.0]);
    nested.pointer(release, 1, [5.0, 40.0]);
    let outer = nested.rig.snapshot(nested.world, nested.outer);
    assert!(!outer.interaction.pressed && !outer.interaction.captured);
    // Content under the released pointer toggled nothing.
    assert!(!nested.checked());
    nested.finish();
}

#[test]
fn track_presses_page_by_the_viewport_toward_the_pressed_side() {
    let mut nested = Nested::new();
    let track = nested.bar(nested.outer, false);
    let x = (track[0] + track[2]) * 0.5;
    // Below the thumb: one viewport forward, clamped to the capacity of 4.
    nested.pointer(press, 1, [x, 5.5]);
    nested.pointer(release, 1, [x, 5.5]);
    assert_eq!(nested.offsets(), [0.0, 4.0]);
    // The thumb moved to the track end; a press above it pages back.
    assert!(nested.bar(nested.outer, true)[1] > 0.5);
    nested.pointer(press, 2, [x, 0.5]);
    nested.pointer(release, 2, [x, 0.5]);
    assert_eq!(nested.offsets(), [0.0, 0.0]);
    nested.finish();
}

/// A `10 x 4` ScrollView over a column of a full-width checkbox and `filler`.
fn single_scroll(filler: f32) -> (Rig, EntityId, EntityId) {
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas_root(&mut host, world, 10.0, 10.0);
    let view = node(&mut host, world, root_entity, 0, [10.0, 4.0], scroll_view());
    let content = node(&mut host, world, view, 2, [10.0, 1.0 + filler], Vec::new());
    let checkbox = node(&mut host, world, content, 0, [10.0, 1.0], checkbox());
    node(&mut host, world, content, 0, [10.0, filler], Vec::new());
    (Rig::new(host, root, viewport(100, 100)), view, checkbox)
}

#[test]
fn scroll_bars_appear_only_with_overflow_and_cover_the_content_beneath_them() {
    // Content that fits shows no bar: the checkbox's full width stays
    // hittable at the viewport edge.
    let (mut fitting, _, checkbox) = single_scroll(1.0);
    assert!(fitting.canvas(fitting.root).hits.iter().all(|hit| {
        !matches!(
            hit.kind,
            CanvasHitKind::ScrollThumb { .. } | CanvasHitKind::ScrollTrack { .. }
        )
    }));
    let edge = fitting.logical([9.95, 0.5]);
    fitting.send(press(1, edge));
    fitting.send(release(1, edge));
    assert_eq!(fitting.commits(), [(checkbox, true)]);
    fitting.finish();

    // Overflowing content shows a bar over the checkbox's right edge:
    // pressing there takes the bar, while the rest keeps its hit target.
    let (mut overflowing, view, checkbox) = single_scroll(9.0);
    let world = overflowing.root.world();
    overflowing.send(press(1, edge));
    overflowing.send(release(1, edge));
    assert!(overflowing.commits().is_empty());
    let middle = overflowing.logical([5.0, 0.5]);
    overflowing.send(press(2, middle));
    overflowing.send(release(2, middle));
    assert_eq!(overflowing.commits(), [(checkbox, true)]);
    assert_eq!(overflowing.scroll(world, view), [0.0, 0.0]);
    overflowing.finish();
}

#[test]
fn removing_a_scroll_view_mid_thumb_drag_drops_the_bar_press() {
    let mut nested = Nested::new();
    let thumb = nested.bar(nested.outer, true);
    nested.pointer(press, 1, [(thumb[0] + thumb[2]) * 0.5, thumb[1] + 0.5]);
    assert!(
        nested
            .rig
            .snapshot(nested.world, nested.outer)
            .interaction
            .captured
    );
    let commands = [nested.outer, nested.inner]
        .into_iter()
        .map(|entity| Command::Delete {
            entity: EntityRef::Handle(entity),
        })
        .collect();
    apply(&mut nested.rig.host, nested.world, commands);
    nested.rig.frame();
    assert_eq!(nested.rig.synchronize().pointers(), [1]);
    let settled = terminals(&nested.rig.ledger).len();
    // The release finds nothing to complete.
    assert_eq!(
        nested.rig.send(release(1, nested.rig.logical([5.0, 2.0]))),
        GuiRoutingDisposition::Unhandled
    );
    assert_eq!(terminals(&nested.rig.ledger).len(), settled);
    nested.finish();
}

#[test]
fn scroll_bars_and_content_drags_scroll_a_virtual_list_like_a_scroll_view() {
    const COUNT: u32 = 100_000;
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas_root(&mut host, world, 10.0, 10.0);
    let list = node(
        &mut host,
        world,
        root_entity,
        0,
        [10.0, 6.0],
        vec![ComponentValue::GuiVirtualList(GuiVirtualList {
            item_count: COUNT,
            item_extent: 1.0,
            axis: 1,
            overscan: 0,
            ..Default::default()
        })],
    );
    let mut nested = Nested {
        rig: Rig::new(host, root, viewport(100, 100)),
        world,
        outer: list,
        inner: list,
        checkbox: list,
    };
    let capacity = COUNT as f32 - 6.0;
    // Over this many items the thumb keeps a minimum length; dragging it by
    // half its travel scrolls about half the capacity.
    let thumb = nested.bar(list, true);
    let track = nested.bar(list, false);
    let travel = (track[3] - track[1]) - (thumb[3] - thumb[1]);
    assert!(thumb[3] - thumb[1] > 0.0 && travel > 0.0);
    let x = (thumb[0] + thumb[2]) * 0.5;
    let grab = (thumb[1] + thumb[3]) * 0.5;
    nested.pointer(press, 1, [x, grab]);
    nested.pointer(movement, 1, [x, grab + travel * 0.5]);
    nested.pointer(release, 1, [x, grab + travel * 0.5]);
    let offset = nested.rig.scroll(world, list)[1];
    assert!((offset - capacity / 2.0).abs() < 1.0, "{offset}");
    // A primary drag over content past the slop scrolls by the dragged distance.
    nested.pointer(press, 2, [5.0, 4.0]);
    nested.pointer(movement, 2, [5.0, 3.0]);
    nested.pointer(movement, 2, [5.0, 2.0]);
    nested.pointer(release, 2, [5.0, 2.0]);
    let after = nested.rig.scroll(world, list)[1];
    assert!((after - offset - 2.0).abs() < 1e-2, "{offset} -> {after}");
    nested.finish();
}
