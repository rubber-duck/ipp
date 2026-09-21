use ipp_schema_derive::SchemaComponent;

/// Projection of a scene camera; its effective Transform supplies its pose.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, SchemaComponent)]
pub struct Camera {
    /// 0 selects perspective; 1 selects orthographic projection.
    pub projection: u32,
    /// Perspective vertical field of view in radians, strictly between 0 and pi.
    pub fov_y: f32,
    /// Positive distance to the near clipping plane in camera space.
    pub near: f32,
    /// Distance to the far clipping plane, greater than near.
    pub far: f32,
    /// Full vertical orthographic extent in camera space.
    pub ortho_height: f32,
    /// Positive camera-space distance along local -Z to the navigation pivot.
    pub focus_distance: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            projection: 0,
            fov_y: std::f32::consts::FRAC_PI_4,
            near: 0.1,
            far: 100.0,
            ortho_height: 2.0,
            focus_distance: 6.0,
        }
    }
}

impl crate::components::schema::ComponentLifecycle for Camera {
    fn validate(&self) -> Result<(), crate::ErrorReason> {
        if self.projection > 1
            || ![
                self.fov_y,
                self.near,
                self.far,
                self.ortho_height,
                self.focus_distance,
            ]
            .iter()
            .all(|value| value.is_finite())
            || self.near <= 0.0
            || self.far <= self.near
            || self.fov_y <= 0.0
            || self.fov_y >= std::f32::consts::PI
            || self.ortho_height <= 0.0
            || self.focus_distance <= 0.0
        {
            return Err(crate::ErrorReason::InvalidValue);
        }
        Ok(())
    }
}
