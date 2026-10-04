//! Overlay placement against a parent's evaluated box or the canvas.
//!
//! An overlay ([`GuiOverlay`]) leaves its parent's flow: the constraint pass
//! skips it among its siblings and lays it out afterwards as its own root, once
//! every box it can depend on is placed, so an overlay opened from another
//! overlay is placed after that one. Nothing in the flow depends on an
//! overlay, so no pass repeats.
//!
//! [`GuiOverlayInputs`] reads what one overlay's placement depends on: whether
//! it is open, the box it is placed against in canvas coordinates,
//! and the canvas transform of its parent, composed from the ancestors' layout
//! origins and `CanvasStyle` the same way the Canvas walk composes them. The
//! layout keeps these inputs, and on a frame with no other layout work
//! recomputes them for each overlay, so an anchor that scrolls, moves or
//! scales, or an overlay that opens, lays the canvas out again before the
//! Canvas walk of the same frame. A canvas without overlays keeps no inputs
//! and does no such work.
//!
//! [`GuiOverlayPlacement`] sizes the overlay by its content: the first pass
//! leaves both axes unbounded, except a stretched axis, which is held to the
//! box's extent. Containers that fill a bounded constraint therefore fit their
//! content, while a scroll view in an overlay needs a bounded extent on both
//! axes, from its own or the overlay's explicit or maximum size or from a
//! stretched axis. The measured size picks the side: the
//! preferred one, or the opposite one when the preferred side lacks room and
//! the opposite side has more. An axis whose content exceeds its room, the
//! chosen side's along the main axis and the canvas extent across it, is laid
//! out again limited to that room, at most once per axis. The overlay then
//! takes its position on the side, aligned along it and offset by its own
//! translation, and shifts across the side to stay inside the canvas; its
//! layout margins do not apply. Centred over its box, or against the canvas,
//! where it rests inside on the named edge, it shifts along both axes.

use super::component::GuiOverlay;
use super::entity_layout::GuiEntityLayout;
use super::geometry::Constraints;
use crate::EntityId;
use crate::world::WorldSimulationState;
use std::collections::BTreeMap;

/// Canvas units below which a size is taken to fit its room, absorbing
/// rounding in constraint arithmetic.
const FIT_TOLERANCE: f32 = 1.0e-3;

/// Evaluated canvas position and signed scale of an entity's local space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::world::systems) struct GuiCanvasTransform {
    pub position: [f32; 2],
    pub scale: [f32; 2],
}

impl GuiCanvasTransform {
    const IDENTITY: Self = Self {
        position: [0.0; 2],
        scale: [1.0; 2],
    };

    /// Canvas `[min_x, min_y, max_x, max_y]` of a local box at the origin.
    fn bounds(self, size: [f32; 2]) -> [f32; 4] {
        let end = [
            self.position[0] + self.scale[0] * size[0],
            self.position[1] + self.scale[1] * size[1],
        ];
        [
            self.position[0].min(end[0]),
            self.position[1].min(end[1]),
            self.position[0].max(end[0]),
            self.position[1].max(end[1]),
        ]
    }
}

/// The frame one shown overlay is placed in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::world::systems) struct GuiOverlayFrame {
    /// Canvas transform of the parent's local space, in which the overlay's
    /// origin is expressed; the identity at the top level.
    pub parent: GuiCanvasTransform,
    /// Canvas box placed against: the parent's evaluated box, a point where
    /// the parent has no layout box, or the canvas at the top level.
    pub anchor: [f32; 4],
    /// Whether the box is the canvas itself.
    pub canvas: bool,
    /// The overlay's own `CanvasStyle` translation, in parent-local units.
    pub translation: [f32; 2],
    /// The overlay's own `CanvasStyle` scale.
    pub scale: [f32; 2],
}

/// Everything one overlay's placement reads, kept to detect changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::world::systems) struct GuiOverlayInputs {
    /// The overlay's own `GuiBehavior.visible`, its open state.
    pub open: bool,
    /// Placement frame, read only while the overlay is open.
    pub frame: Option<GuiOverlayFrame>,
}

impl GuiOverlayInputs {
    /// Read the inputs of `entity`, a child of `parent`, from the placements
    /// of a layout whose boxes outside overlays are final.
    pub fn read(
        world: &WorldSimulationState,
        placements: &BTreeMap<EntityId, GuiEntityLayout>,
        extent: [f32; 2],
        entity: EntityId,
        parent: Option<EntityId>,
    ) -> Self {
        let index = entity.index() as usize;
        let open = world
            .components
            .gui_behavior(index)
            .is_none_or(|behavior| behavior.visible);
        let style = world.components.canvas_style(index).copied();
        let frame = open.then(|| {
            let style = style.unwrap_or_default();
            let (transform, anchor) = match parent {
                None => (
                    GuiCanvasTransform::IDENTITY,
                    [0.0, 0.0, extent[0], extent[1]],
                ),
                Some(parent) => {
                    let transform = canvas_transform(world, placements, parent);
                    let size = placements.get(&parent).map_or([0.0; 2], |box_| box_.size);
                    (transform, transform.bounds(size))
                }
            };
            GuiOverlayFrame {
                parent: transform,
                anchor,
                canvas: parent.is_none(),
                translation: [style.x, style.y],
                scale: [style.scale_x, style.scale_y],
            }
        });
        Self {
            open,
            frame,
        }
    }

    /// Whether the overlay is laid out and painted.
    pub fn shown(&self) -> bool {
        self.frame.is_some()
    }
}

/// One overlay found by the last layout and the inputs its placement read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::world::systems) struct GuiOverlayLayout {
    pub entity: EntityId,
    /// Its parent, or none at the top level.
    pub parent: Option<EntityId>,
    pub inputs: GuiOverlayInputs,
}

/// Canvas transform of `entity`'s local space: the layout origins and
/// `CanvasStyle` of every ancestor, root first, composed as the Canvas walk
/// composes them.
fn canvas_transform(
    world: &WorldSimulationState,
    placements: &BTreeMap<EntityId, GuiEntityLayout>,
    entity: EntityId,
) -> GuiCanvasTransform {
    let ancestry = super::super::local::control::ancestry(&world.state, entity);
    let mut transform = GuiCanvasTransform::IDENTITY;
    for ancestor in ancestry.iter() {
        if let Some(placement) = placements.get(ancestor) {
            for axis in 0..2 {
                transform.position[axis] += placement.origin[axis] * transform.scale[axis];
            }
        }
        if let Some(style) = world.components.canvas_style(ancestor.index() as usize) {
            (transform.position, transform.scale) =
                style.compose(transform.position, transform.scale);
        }
    }
    transform
}

/// Where one overlay goes relative to its box along the main axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GuiOverlaySide {
    /// Outside the box past its maximum edge: below or right.
    After,
    /// Outside the box before its minimum edge: above or left.
    Before,
    /// Inside the canvas against its maximum edge.
    InsideAfter,
    /// Inside the canvas against its minimum edge.
    InsideBefore,
    /// Centred over the box.
    Centre,
}

/// Measurement and placement of one shown overlay in canvas units.
pub(super) struct GuiOverlayPlacement {
    overlay: GuiOverlay,
    frame: GuiOverlayFrame,
    extent: [f32; 2],
    /// Signed canvas units per overlay-local unit.
    scale: [f32; 2],
    /// The translation in canvas units.
    offset: [f32; 2],
    /// Axis the side is on: 1 for bottom, top and centre, 0 for right and left.
    main: usize,
}

impl GuiOverlayPlacement {
    pub fn new(overlay: GuiOverlay, frame: GuiOverlayFrame, extent: [f32; 2]) -> Self {
        Self {
            overlay,
            frame,
            extent,
            scale: std::array::from_fn(|axis| frame.parent.scale[axis] * frame.scale[axis]),
            offset: std::array::from_fn(|axis| frame.parent.scale[axis] * frame.translation[axis]),
            main: usize::from(!matches!(overlay.side, 2 | 3)),
        }
    }

    /// Local length of `canvas` units along `axis`; unbounded where the
    /// overlay has no extent on the canvas.
    fn local(&self, axis: usize, canvas: f32) -> f32 {
        let scale = self.scale[axis].abs();
        if scale > 0.0 && scale.is_finite() {
            canvas / scale
        } else {
            f32::INFINITY
        }
    }

    fn stretched(&self) -> bool {
        self.overlay.align == GuiOverlay::ALIGN_STRETCH
    }

    /// Axis `align` applies to.
    fn cross(&self) -> usize {
        if self.overlay.side == GuiOverlay::SIDE_CENTRE {
            0
        } else {
            1 - self.main
        }
    }

    /// Constraints of the first measurement: unbounded, but a stretched axis
    /// held to the box's extent within the canvas.
    pub fn measure(&self) -> Constraints {
        let mut constraints = Constraints::loose(f32::INFINITY, f32::INFINITY);
        if self.stretched() {
            let axis = self.cross();
            let extent = (self.frame.anchor[axis + 2] - self.frame.anchor[axis])
                .min(self.extent[axis])
                .max(0.0);
            let length = self.local(axis, extent);
            if axis == 0 {
                constraints.min_w = length;
                constraints.max_w = length;
            } else {
                constraints.min_h = length;
                constraints.max_h = length;
            }
        }
        constraints
    }

    /// Distance the translation moves the overlay away from its box along the
    /// main axis, kept when it flips.
    fn gap(&self) -> f32 {
        if matches!(self.overlay.side, 0 | 2) {
            self.offset[self.main]
        } else {
            -self.offset[self.main]
        }
    }

    /// Canvas room past the box's maximum and before its minimum edge.
    fn rooms(&self) -> [f32; 2] {
        let gap = self.gap();
        let anchor = self.frame.anchor;
        [
            (self.extent[self.main] - (anchor[self.main + 2] + gap)).max(0.0),
            (anchor[self.main] - gap).max(0.0),
        ]
    }

    /// The side for a measured local `size`.
    pub fn side(&self, size: [f32; 2]) -> GuiOverlaySide {
        let after = matches!(self.overlay.side, 0 | 2);
        if self.overlay.side == GuiOverlay::SIDE_CENTRE {
            return GuiOverlaySide::Centre;
        }
        if self.frame.canvas {
            return if after {
                GuiOverlaySide::InsideAfter
            } else {
                GuiOverlaySide::InsideBefore
            };
        }
        let [past, before] = self.rooms();
        let (preferred, opposite) = if after {
            (past, before)
        } else {
            (before, past)
        };
        let needed = size[self.main] * self.scale[self.main].abs();
        let flipped = needed > preferred + FIT_TOLERANCE && opposite > preferred;
        if after != flipped {
            GuiOverlaySide::After
        } else {
            GuiOverlaySide::Before
        }
    }

    /// Canvas room along each axis on `side`.
    fn room(&self, side: GuiOverlaySide) -> [f32; 2] {
        let mut room = self.extent;
        match side {
            GuiOverlaySide::After => room[self.main] = self.rooms()[0],
            GuiOverlaySide::Before => room[self.main] = self.rooms()[1],
            _ => {}
        }
        room
    }

    /// `constraints` limited on every axis whose laid-out `size` exceeds its
    /// room and that is not limited yet, or none when the size fits.
    pub fn limit(
        &self,
        constraints: Constraints,
        size: [f32; 2],
        side: GuiOverlaySide,
    ) -> Option<Constraints> {
        let room = self.room(side);
        let mut limited = constraints;
        let mut changed = false;
        for axis in 0..2 {
            let local = self.local(axis, room[axis]);
            let (min, max) = if axis == 0 {
                (&mut limited.min_w, &mut limited.max_w)
            } else {
                (&mut limited.min_h, &mut limited.max_h)
            };
            if size[axis] * self.scale[axis].abs() > room[axis] + FIT_TOLERANCE && *max > local {
                *max = local;
                *min = min.min(local);
                changed = true;
            }
        }
        changed.then_some(limited)
    }

    /// Parent-local origin that places an overlay of laid-out local `size` on
    /// `side`, before its own translation, which the Canvas walk adds.
    pub fn origin(&self, size: [f32; 2], side: GuiOverlaySide) -> [f32; 2] {
        let anchor = self.frame.anchor;
        let canvas: [f32; 2] = std::array::from_fn(|axis| size[axis] * self.scale[axis].abs());
        let mut start = [0.0_f32; 2];

        let main = self.main;
        let gap = self.gap();
        start[main] = match side {
            GuiOverlaySide::After => anchor[main + 2] + gap,
            GuiOverlaySide::Before => anchor[main] - gap - canvas[main],
            GuiOverlaySide::InsideAfter => self.extent[main] - canvas[main] + self.offset[main],
            GuiOverlaySide::InsideBefore => self.offset[main],
            GuiOverlaySide::Centre => {
                (anchor[main] + anchor[main + 2] - canvas[main]) / 2.0 + self.offset[main]
            }
        };
        if !matches!(side, GuiOverlaySide::After | GuiOverlaySide::Before) {
            start[main] = shift(start[main], canvas[main], self.extent[main]);
        }

        let cross = self.cross();
        let aligned = match self.overlay.align {
            1 => (anchor[cross] + anchor[cross + 2] - canvas[cross]) / 2.0,
            2 => anchor[cross + 2] - canvas[cross],
            _ => anchor[cross],
        };
        start[cross] = shift(
            aligned + self.offset[cross],
            canvas[cross],
            self.extent[cross],
        );

        // The canvas box starts at the transformed origin plus the signed
        // extent where a negative scale mirrors it; the Canvas walk adds the
        // translation after the origin.
        std::array::from_fn(|axis| {
            let position = start[axis] - (self.scale[axis] * size[axis]).min(0.0);
            let parent = self.frame.parent.scale[axis];
            if parent == 0.0 || !parent.is_finite() {
                0.0
            } else {
                (position - self.frame.parent.position[axis]) / parent
                    - self.frame.translation[axis]
            }
        })
    }
}

/// Shift a span of `length` starting at `start` to lie inside `0..extent`,
/// keeping its start at zero when it is longer.
fn shift(start: f32, length: f32, extent: f32) -> f32 {
    start.min(extent - length).max(0.0)
}
