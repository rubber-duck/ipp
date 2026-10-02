use crate::{ErrorReason, components::schema::ComponentLifecycle};
use ipp_schema_derive::SchemaComponent;

/// Clipped local-XY presentation plane attached to one entity. Its content
/// comes from ordinary entities, such as an attached Canvas World.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, SchemaComponent)]
pub struct Surface {
    /// Centred clipping width in metres.
    pub width: f32,
    /// Centred clipping height in metres.
    pub height: f32,
    /// Metres between consecutive layer plane ids of a presented canvas along
    /// the local +Z normal; zero keeps every layer on one plane. Layer `n`
    /// presents `n * layer_spacing` in front of the plane, for an exploded
    /// view, whatever other layers are in use.
    ///
    /// Only a Surface placed in a camera's 3D domain separates layers; a
    /// Surface that is a slot of another canvas presents its canvas on its
    /// slot's plane. Changing the spacing moves presentation and input planes
    /// without repainting the canvas.
    pub layer_spacing: f32,
}

impl Default for Surface {
    fn default() -> Self {
        Self {
            width: 1.0,
            height: 1.0,
            layer_spacing: 0.0,
        }
    }
}

impl Surface {
    /// Conservative local-space enclosure used by headless geometry consumers.
    pub fn local_bounding_geometry(&self) -> crate::systems::geometry::GeometryShape {
        crate::systems::geometry::GeometryShape::Box {
            min: [-(self.width as f64) * 0.5, -(self.height as f64) * 0.5, 0.0],
            max: [(self.width as f64) * 0.5, (self.height as f64) * 0.5, 0.0],
        }
    }

    /// Map a 2D point in Surface content coordinates ([0, width] x [0, height], +X right, +Y down)
    /// to centred entity-local 3D coordinates (+X right, +Y up, front +Z).
    #[inline]
    pub fn content_to_entity_local(&self, x: f32, y: f32) -> [f32; 3] {
        [x - self.width * 0.5, self.height * 0.5 - y, 0.0]
    }

    /// Map a 2D point in centred entity-local coordinates to 2D Surface content coordinates.
    #[inline]
    pub fn entity_local_to_content(&self, entity_x: f32, entity_y: f32) -> [f32; 2] {
        [entity_x + self.width * 0.5, self.height * 0.5 - entity_y]
    }

    /// Whether a point in Surface content coordinates falls within the content bounds [0, width] x [0, height].
    #[inline]
    pub fn contains_content_point(&self, point: [f32; 2]) -> bool {
        point[0] >= 0.0 && point[0] <= self.width && point[1] >= 0.0 && point[1] <= self.height
    }

    /// Map an entity-local XY plane hit to Surface content coordinates if it falls within the surface bounds.
    #[inline]
    pub fn plane_hit_to_content(&self, entity_x: f32, entity_y: f32) -> Option<[f32; 2]> {
        let content = self.entity_local_to_content(entity_x, entity_y);
        self.contains_content_point(content).then_some(content)
    }

    /// The 2D content rectangle bounds [min_x, min_y, max_x, max_y] in content coordinates.
    #[inline]
    pub fn content_bounds(&self) -> [f32; 4] {
        [0.0, 0.0, self.width, self.height]
    }
}

impl ComponentLifecycle for Surface {
    fn required_components() -> &'static [u16] {
        &[
            crate::ComponentValue::TRANSFORM,
            crate::ComponentValue::BOUNDING_GEOMETRY,
        ]
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        if !self.width.is_finite()
            || !self.height.is_finite()
            || self.width <= 0.0
            || self.height <= 0.0
            || !self.layer_spacing.is_finite()
        {
            return Err(ErrorReason::InvalidValue);
        }
        Ok(())
    }
}

/// Opt-in whole-Surface texture caching for the Surface on the same entity.
///
/// Absence keeps direct presentation. The component carries only authored
/// thresholds; [`super::SurfaceCachePolicy`] documents the distance bands,
/// hysteresis and refresh caps derived from them. Cached images, deadlines and
/// interaction priority stay transient renderer state.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, SchemaComponent)]
pub struct SurfaceCache {
    /// Camera-to-anchor distance in metres below which presentation stays
    /// direct. Zero caches at every distance.
    pub direct_distance: f32,
    /// Cache texel density in the first cached band, per Surface metre.
    pub texels_per_metre: f32,
    /// Maximum content refresh rate in the first cached band, in hertz.
    pub max_refresh_hz: f32,
}

impl Default for SurfaceCache {
    fn default() -> Self {
        Self {
            direct_distance: 4.0,
            texels_per_metre: 512.0,
            max_refresh_hz: 30.0,
        }
    }
}

impl ComponentLifecycle for SurfaceCache {
    fn validate(&self) -> Result<(), ErrorReason> {
        super::SurfaceCachePolicy::new(self).map(|_| ())
    }

    fn validate_field(&self, _offset: u32) -> Result<(), ErrorReason> {
        // Every field is independently bounded; checking the whole value keeps
        // one validation rule for partial writes and complete insertion.
        self.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_coordinates_top_left_y_down_conversions() {
        let surface = Surface {
            width: 4.0,
            height: 2.0,
            ..Default::default()
        };

        // Origin (top-left) in content coords is (-w/2, +h/2) in entity local coords
        assert_eq!(surface.content_to_entity_local(0.0, 0.0), [-2.0, 1.0, 0.0]);
        assert_eq!(surface.entity_local_to_content(-2.0, 1.0), [0.0, 0.0]);

        // Bottom-right in content coords is (+w/2, -h/2) in entity local coords
        assert_eq!(surface.content_to_entity_local(4.0, 2.0), [2.0, -1.0, 0.0]);
        assert_eq!(surface.entity_local_to_content(2.0, -1.0), [4.0, 2.0]);

        // Center in content coords is (w/2, h/2) and (0, 0) in entity local coords
        assert_eq!(surface.content_to_entity_local(2.0, 1.0), [0.0, 0.0, 0.0]);
        assert_eq!(surface.entity_local_to_content(0.0, 0.0), [2.0, 1.0]);

        // Bounds and hit tests
        assert_eq!(surface.content_bounds(), [0.0, 0.0, 4.0, 2.0]);
        assert!(surface.contains_content_point([0.0, 0.0]));
        assert!(surface.contains_content_point([4.0, 2.0]));
        assert!(!surface.contains_content_point([-0.1, 1.0]));
        assert!(!surface.contains_content_point([4.1, 1.0]));
        assert!(!surface.contains_content_point([2.0, -0.1]));
        assert!(!surface.contains_content_point([2.0, 2.1]));

        assert_eq!(surface.plane_hit_to_content(0.0, 0.0), Some([2.0, 1.0]));
        assert_eq!(surface.plane_hit_to_content(-2.0, 1.0), Some([0.0, 0.0]));
        assert_eq!(surface.plane_hit_to_content(2.1, 0.0), None);
    }
}
