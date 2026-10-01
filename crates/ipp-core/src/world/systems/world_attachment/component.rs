//! Ordinary producer component for cross-World composition.

use crate::{OutputKind, OutputRef, WorldAttachmentMode, WorldRef};

/// Producer-authored graph identity on an ordinary entity. Placement stays in Transform.
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, ipp_schema_derive::SchemaComponent)]
pub struct WorldAttachment {
    /// Exact child lifetime; absent on an unconfigured component.
    pub child: Option<WorldRef>,
    /// The child's Camera output presented by SurfaceCamera; empty for Spatial
    /// and SurfaceCanvas, which presents the child's canvas.
    pub output: Option<OutputRef>,
    /// 0: spatial, 1: SurfaceCanvas, 2: SurfaceCamera.
    pub mode: u32,
}

impl WorldAttachment {
    /// Select a spatial contribution without another camera domain.
    pub fn spatial(child: WorldRef) -> Self {
        Self {
            child: Some(child),
            output: None,
            mode: 0,
        }
    }

    /// Present a child output: SurfaceCanvas for its canvas, SurfaceCamera for
    /// one of its cameras.
    pub fn surface(output: OutputRef) -> Self {
        match output.kind() {
            OutputKind::Canvas => Self::surface_canvas(output.world),
            OutputKind::Camera => Self {
                child: Some(output.world),
                output: Some(output),
                mode: 2,
            },
        }
    }

    /// Present the child World's canvas.
    pub fn surface_canvas(child: WorldRef) -> Self {
        Self {
            child: Some(child),
            output: None,
            mode: 1,
        }
    }

    /// The child output this attachment presents: the child's canvas in
    /// SurfaceCanvas mode, the selected camera in SurfaceCamera mode.
    pub fn presented_output(&self) -> Option<OutputRef> {
        match self.mode() {
            WorldAttachmentMode::Spatial => None,
            WorldAttachmentMode::SurfaceCanvas => self.child.map(OutputRef::canvas),
            WorldAttachmentMode::SurfaceCamera => self.output,
        }
    }

    /// Producer-selected child independently of readiness.
    pub fn child(&self) -> Option<WorldRef> {
        self.child
    }

    /// Decode the admitted component's validated mode.
    pub fn mode(&self) -> WorldAttachmentMode {
        match self.mode {
            1 => WorldAttachmentMode::SurfaceCanvas,
            2 => WorldAttachmentMode::SurfaceCamera,
            _ => WorldAttachmentMode::Spatial,
        }
    }
}

impl crate::components::schema::ComponentLifecycle for WorldAttachment {
    fn animatable_field(_: u32) -> bool {
        false
    }

    fn validate(&self) -> Result<(), crate::ErrorReason> {
        match (self.mode, self.child, self.output) {
            (0, _, None) | (1, Some(_), None) => Ok(()),
            (2, Some(child), Some(output))
                if output.world == child && output.kind() == OutputKind::Camera =>
            {
                Ok(())
            }
            _ => Err(crate::ErrorReason::InvalidValue),
        }
    }
}
