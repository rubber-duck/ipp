//! Analytic path instances: the atlas ranges of one quadratic path and the packed
//! layout of a contiguous Surface draw.

/// Renderer-private atlas ranges needed to draw one quadratic path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfacePathDescriptor {
    /// First curve texel and texel count, including contour terminators, in the shared atlas.
    pub curve_range: [u32; 2],
    /// First of the path's band header texels.
    pub band_offset: u32,
}

impl SurfacePathDescriptor {
    /// Construct one validated-at-draw descriptor.
    pub fn new(curve_range: [u32; 2], band_offset: u32) -> Self {
        Self {
            curve_range,
            band_offset,
        }
    }
}

/// One glyph/path instance packed for a contiguous Surface draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfacePathInstance {
    /// Local path bounds.
    pub bounds: [f32; 4],
    /// Local translation and scale.
    pub placement: [f32; 4],
    /// Straight linear RGBA.
    pub color: [f32; 4],
    /// Atlas curve/band lookup.
    pub descriptor: SurfacePathDescriptor,
}

pub(super) fn pack_surface_instances(
    instances: &[SurfacePathInstance],
    packed: &mut Vec<[f32; 16]>,
) {
    packed.clear();
    packed.extend(instances.iter().map(|instance| {
        [
            instance.bounds[0],
            instance.bounds[1],
            instance.bounds[2],
            instance.bounds[3],
            instance.placement[0],
            instance.placement[1],
            instance.placement[2],
            instance.placement[3],
            instance.color[0],
            instance.color[1],
            instance.color[2],
            instance.color[3],
            instance.descriptor.curve_range[0] as f32,
            instance.descriptor.curve_range[1] as f32,
            instance.descriptor.band_offset as f32,
            0.0,
        ]
    }));
}

pub(super) fn surface_instances_exact(instances: &[SurfacePathInstance]) -> bool {
    const MAX_EXACT_F32_INTEGER: u32 = 1 << 24;
    instances.iter().all(|instance| {
        instance.descriptor.curve_range[0] <= MAX_EXACT_F32_INTEGER
            && instance.descriptor.curve_range[1] <= MAX_EXACT_F32_INTEGER
            && instance.descriptor.band_offset <= MAX_EXACT_F32_INTEGER
    })
}

#[cfg(test)]
mod surface_instance_tests {
    use super::*;

    #[test]
    fn floating_instance_descriptors_reject_the_first_inexact_integer() {
        let instance = |band_offset| SurfacePathInstance {
            bounds: [0.0; 4],
            placement: [0.0; 4],
            color: [0.0; 4],
            descriptor: SurfacePathDescriptor::new([1 << 24, 1], band_offset),
        };
        assert!(surface_instances_exact(&[instance(1 << 24)]));
        assert!(!surface_instances_exact(&[instance((1 << 24) + 1)]));
    }

    #[test]
    fn packing_reuses_warmed_storage() {
        let instance = SurfacePathInstance {
            bounds: [1.0, 2.0, 3.0, 4.0],
            placement: [5.0, 6.0, 7.0, 8.0],
            color: [0.1, 0.2, 0.3, 0.4],
            descriptor: SurfacePathDescriptor::new([9, 10], 11),
        };
        let instances = vec![instance; 256];
        let mut packed = Vec::new();
        pack_surface_instances(&instances, &mut packed);
        let capacity = packed.capacity();
        let pointer = packed.as_ptr();

        pack_surface_instances(&instances[..8], &mut packed);
        assert_eq!(packed.as_ptr(), pointer);
        pack_surface_instances(&instances[..200], &mut packed);

        assert_eq!(packed.capacity(), capacity);
        assert_eq!(packed.as_ptr(), pointer);
        assert_eq!(packed.len(), 200);
        assert_eq!(
            packed[0][..12],
            [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 0.1, 0.2, 0.3, 0.4]
        );
        assert_eq!(packed[0][12..], [9.0, 10.0, 11.0, 0.0]);
    }
}
