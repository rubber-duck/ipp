//! Shared component primitives and re-exports of subsystem-owned definitions.
//! Effective instances live in the World's stable typed storage.
//!
//! Component index: `Scalar` and `Transform` here; camera, geometry,
//! hierarchy, look-at, render materials and debug shapes in their system
//! modules; `Surface` in the surface system with `GuiRoot` panels in the
//! GUI system; skeleton/skin, mesh poses and particles behind their
//! capabilities. Each subsystem module owns its definitions: this module
//! only re-exports them. GUI layout output, semantic snapshots and action
//! translation belong to the GUI subsystem.

use ipp_schema_derive::SchemaComponent;

/// A finite scalar, useful independently and as a constraint input/output.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, SchemaComponent)]
pub struct Scalar {
    /// Authored or evaluated scalar value.
    pub value: f32,
}

impl super::schema::ComponentLifecycle for Scalar {}

/// Authored local TRS, using metres and an xyzw quaternion.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, SchemaComponent)]
pub struct Transform {
    /// Translation along +X, in metres.
    pub x: f32,
    /// Translation along +Y, in metres.
    pub y: f32,
    /// Translation along +Z, in metres.
    pub z: f32,
    /// Quaternion X coordinate.
    pub qx: f32,
    /// Quaternion Y coordinate.
    pub qy: f32,
    /// Quaternion Z coordinate.
    pub qz: f32,
    /// Quaternion W coordinate.
    pub qw: f32,
    /// Positive local X scale.
    pub sx: f32,
    /// Positive local Y scale.
    pub sy: f32,
    /// Positive local Z scale.
    pub sz: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            qx: 0.0,
            qy: 0.0,
            qz: 0.0,
            qw: 1.0,
            sx: 1.0,
            sy: 1.0,
            sz: 1.0,
        }
    }
}

impl super::schema::ComponentLifecycle for Transform {
    fn validate(&self) -> Result<(), crate::ErrorReason> {
        if ![
            self.x, self.y, self.z, self.qx, self.qy, self.qz, self.qw, self.sx, self.sy, self.sz,
        ]
        .iter()
        .all(|value| value.is_finite())
            || [self.qx, self.qy, self.qz, self.qw]
                .iter()
                .all(|&value| value == 0.0)
            || self.sx <= 0.0
            || self.sy <= 0.0
            || self.sz <= 0.0
        {
            Err(crate::ErrorReason::InvalidValue)
        } else {
            Ok(())
        }
    }
}
