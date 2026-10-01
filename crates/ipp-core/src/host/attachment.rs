//! Typed Host graph references and producer-only attachment declarations.

use crate::{EntityId, WorldId};

/// Exact runtime World lifetime, distinct from durable metadata and local entity IDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorldRef {
    pub(crate) id: WorldId,
    pub(crate) incarnation: usize,
}

impl WorldRef {
    /// Host-local identity; equality also checks the exact runtime lifetime.
    pub fn id(self) -> WorldId {
        self.id
    }

    /// Opaque live lifetime token for transport inspection, not durable persistence.
    pub fn incarnation(self) -> u64 {
        self.incarnation as u64
    }
}

/// Output domains never change kind when their selected producer becomes unavailable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OutputKind {
    /// Retained Canvas vector/glyph output.
    Canvas,
    /// An independently selected camera domain.
    Camera,
}

/// What an output selects within its World.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OutputTarget {
    /// The World's one canvas; it exists while the World selects the Canvas
    /// System, and its lifetime is the World's.
    Canvas,
    /// A Camera entity's view, fenced by its Camera component lifetime.
    Camera {
        /// Generational Camera entity within the selected World.
        entity: EntityId,
        /// Exact Camera component lifetime.
        incarnation: u64,
    },
}

/// One explicit output selection, fenced to its World lifetime and, for a
/// Camera, to the Camera component lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OutputRef {
    pub(crate) world: WorldRef,
    pub(crate) target: OutputTarget,
}

impl OutputRef {
    /// The canvas of an exact World lifetime. Holding it asserts nothing:
    /// every use validates that the World still selects the Canvas System.
    pub fn canvas(world: WorldRef) -> Self {
        Self {
            world,
            target: OutputTarget::Canvas,
        }
    }

    /// Exact independently living World.
    pub fn world(self) -> WorldRef {
        self.world
    }

    /// Selected output within the World.
    pub fn target(self) -> OutputTarget {
        self.target
    }

    /// Explicit domain, unchanged by temporary unavailability.
    pub fn kind(self) -> OutputKind {
        match self.target {
            OutputTarget::Canvas => OutputKind::Canvas,
            OutputTarget::Camera {
                ..
            } => OutputKind::Camera,
        }
    }

    /// Selected Camera entity; absent for the World canvas.
    pub fn camera_entity(self) -> Option<EntityId> {
        match self.target {
            OutputTarget::Canvas => None,
            OutputTarget::Camera {
                entity,
                ..
            } => Some(entity),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Explicit composition mode independent of component presence.
pub enum WorldAttachmentMode {
    /// Contribute to the containing camera, depth and lighting domain.
    Spatial,
    /// Present the child World's canvas.
    SurfaceCanvas,
    /// Present the selected child's camera color/depth output.
    SurfaceCamera,
}

/// A selected System's derived placement claim, never a second authored attachment.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum AttachmentPlacement {
    /// This System does not manage the anchor; ordinary spatial placement remains eligible.
    #[default]
    Unmanaged,
    /// The owning output cannot place this anchor; spatial fallback is forbidden.
    Unavailable {
        /// Exact parent-local output owning the placement domain.
        owner: OutputRef,
    },
    /// Final parent-local placement replacing, rather than multiplying, spatial placement.
    Ready {
        /// Exact parent-local output owning the placement domain.
        owner: OutputRef,
        /// Finite invertible column-major affine from the anchor into its parent World.
        affine: [f64; 16],
    },
}

/// Physical per-view dimensions; Canvas density is not part of this context.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldViewport {
    /// Physical view width in pixels.
    pub width: u32,
    /// Physical view height in pixels.
    pub height: u32,
    /// Platform display scale, not Canvas density.
    pub device_pixel_ratio: f64,
}

impl WorldViewport {
    pub(crate) fn validate(self) -> Result<(), crate::ErrorReason> {
        if self.width == 0
            || self.height == 0
            || !self.device_pixel_ratio.is_finite()
            || self.device_pixel_ratio <= 0.0
        {
            return Err(crate::ErrorReason::InvalidValue);
        }
        Ok(())
    }
}

/// Current parent-derived values borrowed by local evaluation, not a completed scene.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldFrameContext {
    /// Host frame identity shared by all eligible Worlds.
    pub frame: u64,
    /// Host time increment for this frame.
    pub delta: f64,
    /// Current parent-derived affine placement, retaining shear.
    pub placement: [f64; 16],
    /// Current physical view constraints when this branch has a view.
    pub viewport: Option<WorldViewport>,
    /// Exact local output receiving these constraints, absent for spatial contributions.
    /// Invalidated selections never constrain a replacement producer incarnation.
    pub selected_output: Option<OutputRef>,
    /// Physical Surface dimensions in metres; Canvas owns conversion to logical units.
    pub surface_extent: Option<[f64; 2]>,
}

pub(crate) const IDENTITY: [f64; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];
