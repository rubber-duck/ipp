//! Direct Canvas planes and attachment-authorized nested painting.

use super::super::canvas_scene::{CanvasMapping, CanvasScene};
use super::super::frame_statistics::RenderFrameWork;
use super::super::scene::SceneOutputSurface;
use super::{RenderError, RenderService};
use crate::RenderDevice;
use ipp_core::systems::camera;
use ipp_core::systems::canvas::{CanvasClip, CanvasPaintEntry};
use ipp_core::{HostRuntime, OutputKind, WorldViewport};

pub(super) fn plane_matrix(origin: [f64; 2], scale: [f64; 2]) -> Result<[f32; 16], RenderError> {
    let mut matrix = [0.0; 16];
    matrix[0] = scale[0] as f32;
    matrix[5] = scale[1] as f32;
    matrix[10] = 1.0;
    matrix[12] = origin[0] as f32;
    matrix[13] = origin[1] as f32;
    matrix[15] = 1.0;
    if matrix.iter().any(|value| !value.is_finite()) || matrix[0] == 0.0 || matrix[5] == 0.0 {
        return Err(RenderError::InvalidTransform);
    }
    Ok(matrix)
}

/// Where the camera views a layered Canvas from, in its content space, whose
/// Z is the Surface's local normal axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum CanvasLayerEye {
    /// A perspective camera's position along the normal.
    Point(f64),
    /// The normal component of an orthographic camera's view direction.
    Direction(f64),
}

/// How one Canvas presentation separates its layers along its normal.
///
/// Each layer's draws translate its content by its plane id times `spacing`
/// along content Z, folded into that draw's model-view-projection: retained
/// geometry, vertex layouts and uploads stay unchanged when the spacing
/// changes, and a layer keeps its depth whatever other layers are in use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CanvasLayering {
    /// Content Z between consecutive plane ids; zero keeps one plane.
    pub spacing: f32,
    /// Viewer choosing the order in which layer planes draw.
    pub eye: CanvasLayerEye,
}

impl CanvasLayering {
    /// One plane: layers only order paint, as in a root viewport or a nested slot.
    pub const FLAT: Self = Self {
        spacing: 0.0,
        eye: CanvasLayerEye::Direction(-1.0),
    };

    /// Content Z of `layer`'s plane.
    fn depth(&self, layer: u32) -> f64 {
        f64::from(layer) * f64::from(self.spacing)
    }

    /// Content model-view-projection of `layer`'s plane.
    pub fn layer_mvp(&self, mvp: &[f32; 16], layer: u32) -> [f32; 16] {
        let offset = self.depth(layer) as f32;
        let mut placed = *mvp;
        if offset != 0.0 {
            for row in 0..4 {
                placed[12 + row] += offset * mvp[8 + row];
            }
        }
        placed
    }

    /// The ascending plane ids `layers` in view-depth order, farthest plane
    /// first, so translucent planes compose from the front and from behind.
    /// Coincident planes keep plane order, which is painter order.
    pub fn draw_order(&self, layers: &[u32]) -> Vec<u32> {
        let mut order = layers.to_vec();
        if self.spacing == 0.0 {
            return order;
        }
        let depth = |layer: u32| match self.eye {
            CanvasLayerEye::Point(eye) => (self.depth(layer) - eye).abs(),
            CanvasLayerEye::Direction(direction) => self.depth(layer) * direction,
        };
        order.sort_by(|left, right| depth(*right).total_cmp(&depth(*left)).then(left.cmp(right)));
        order
    }
}

/// The camera's view of a Surface's content space, through the Surface's
/// placement in the camera's domain.
pub(super) fn layer_eye(
    camera: &ipp_core::systems::camera::CameraPublication,
    placement: &ipp_core::systems::geometry::GeometryShapeTransform,
) -> CanvasLayerEye {
    if camera.projection.projection == 0 {
        CanvasLayerEye::Point(placement.inverse_point(camera.pose.point([0.0; 3]))[2])
    } else {
        CanvasLayerEye::Direction(placement.inverse_vector(camera.pose.vector([0.0, 0.0, -1.0]))[2])
    }
}

pub(super) enum CanvasChild<'a> {
    Canvas {
        scene: CanvasScene<'a>,
        mvp: [f32; 16],
        clip: CanvasClip,
        opacity: f32,
    },
    Camera {
        selection: ipp_core::OutputRef,
        mvp: [f32; 16],
        extent: [f32; 2],
        clip: CanvasClip,
        opacity: f32,
    },
}

impl<D: RenderDevice> RenderService<D> {
    pub(super) fn draw_output_surface(
        &mut self,
        host: &HostRuntime,
        surface: &SceneOutputSurface,
        view: &camera::CameraPublication,
        view_projection: [f32; 16],
        viewport: WorldViewport,
        stats: &mut RenderFrameWork,
    ) -> Result<(), RenderError> {
        #[cfg(feature = "instrumentation")]
        let _gpu_surface = self.gpu_scope(
            super::RenderGpuScope::Surface,
            Some(surface.entity.world),
            Some(surface.entity.entity),
        );
        let placed = camera::multiply(view_projection, surface.model);
        let [width, height] = surface.extent.map(f64::from);
        match surface.selection.kind() {
            OutputKind::Canvas => {
                let scene = CanvasScene::new(host, surface.selection, surface.publication)?;
                let extent = scene.canvas.logical_extent.map(f64::from);
                let content = plane_matrix(
                    [-width * 0.5, height * 0.5],
                    [width / extent[0], -height / extent[1]],
                )?;
                let layering = if surface.layered() {
                    CanvasLayering {
                        spacing: surface.layer_spacing,
                        eye: layer_eye(view, &surface.placement),
                    }
                } else {
                    CanvasLayering::FLAT
                };
                self.draw_canvas(
                    &scene,
                    camera::multiply(placed, content),
                    scene.root_clip(),
                    1.0,
                    layering,
                    viewport,
                    stats,
                )
            }
            OutputKind::Camera => {
                let content = plane_matrix([-width * 0.5, -height * 0.5], [1.0, 1.0])?;
                self.draw_camera_image(
                    surface.selection,
                    camera::multiply(placed, content),
                    surface.extent,
                    [0.0, 0.0, surface.extent[0], surface.extent[1]],
                    1.0,
                    stats,
                )
            }
        }
    }
}

pub(super) fn canvas_attachment<'a>(
    scene: &CanvasScene<'a>,
    index: usize,
    mvp: [f32; 16],
    clip: CanvasClip,
    opacity: f32,
) -> Result<Option<CanvasChild<'a>>, RenderError> {
    let Some(CanvasPaintEntry::Attachment(slot)) =
        scene.canvas.entries.get(index).map(|entry| entry.as_ref())
    else {
        return Ok(None);
    };

    let opacity = opacity * slot.opacity;
    if opacity <= 0.0 {
        return Ok(None);
    }

    let attachment = scene.attachment(slot);
    let Some((selection, publication)) = attachment.output() else {
        return Ok(None);
    };

    let clip = [
        clip[0].max(slot.clip[0]),
        clip[1].max(slot.clip[1]),
        clip[2].min(slot.clip[2]),
        clip[3].min(slot.clip[3]),
    ];
    if clip[0] >= clip[2] || clip[1] >= clip[3] {
        return Ok(None);
    }

    match selection.kind() {
        OutputKind::Canvas => {
            let child = CanvasScene::new(scene.host, selection, publication)?;
            let extent = child.canvas.logical_extent;
            let Some(mapping) = attachment.child_mapping(extent, clip) else {
                return Err(RenderError::InvalidTransform);
            };

            if mapping.clip[0] >= mapping.clip[2] || mapping.clip[1] >= mapping.clip[3] {
                return Ok(None);
            }

            let content = plane_matrix(mapping.origin, mapping.scale)?;
            Ok(Some(CanvasChild::Canvas {
                scene: child,
                mvp: camera::multiply(mvp, content),
                clip: mapping.clip,
                opacity,
            }))
        }
        OutputKind::Camera => {
            let extent = slot.physical_extent;
            let origin = slot.to_canvas([-extent[0] * 0.5, -extent[1] * 0.5]);
            let extent = extent.map(|value| value as f32);
            let Some(mapping) = CanvasMapping::new(origin, slot.scale, clip, extent) else {
                return Err(RenderError::InvalidTransform);
            };

            let content = plane_matrix(mapping.origin, mapping.scale)?;
            Ok(Some(CanvasChild::Camera {
                selection,
                mvp: camera::multiply(mvp, content),
                extent,
                clip: mapping.clip,
                opacity,
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layering(spacing: f32, eye: CanvasLayerEye) -> CanvasLayering {
        CanvasLayering {
            spacing,
            eye,
        }
    }

    #[test]
    fn layer_planes_draw_farthest_first_from_any_side() {
        use CanvasLayerEye::{Direction, Point};

        // A perspective eye in front, behind, and between layer planes 0, 1, 2 m.
        let layers = [0, 1, 2];
        assert_eq!(layering(1.0, Point(5.0)).draw_order(&layers), [0, 1, 2]);
        assert_eq!(layering(1.0, Point(-5.0)).draw_order(&layers), [2, 1, 0]);
        assert_eq!(layering(1.0, Point(1.2)).draw_order(&layers), [0, 2, 1]);
        // Negative spacing stacks the layers behind the base plane.
        assert_eq!(layering(-1.0, Point(5.0)).draw_order(&layers), [2, 1, 0]);
        // Orthographic views looking down -Z from the front, and from behind.
        assert_eq!(
            layering(1.0, Direction(-0.5)).draw_order(&layers),
            [0, 1, 2]
        );
        assert_eq!(layering(1.0, Direction(0.5)).draw_order(&layers), [2, 1, 0]);
        // Coincident or edge-on planes keep painter order.
        assert_eq!(CanvasLayering::FLAT.draw_order(&layers), [0, 1, 2]);
        assert_eq!(layering(1.0, Direction(0.0)).draw_order(&layers), [0, 1, 2]);
    }

    #[test]
    fn unused_layers_leave_a_gap_in_depth_order() {
        use CanvasLayerEye::Point;

        // Planes 0, 1 and 3 in use, 2 empty, an eye at 1.8 m: plane 3 lies
        // 1.2 m away, farther than plane 1 at 0.8 m, so it draws before it.
        // Compacted onto 2 it would lie 0.2 m away and draw last.
        assert_eq!(layering(1.0, Point(1.8)).draw_order(&[0, 1, 3]), [0, 3, 1]);
    }

    #[test]
    fn a_layer_translates_content_by_its_id_times_the_spacing() {
        let mvp: [f32; 16] = std::array::from_fn(|index| index as f32 + 1.0);
        assert_eq!(CanvasLayering::FLAT.layer_mvp(&mvp, 3), mvp);
        let layered = layering(0.5, CanvasLayerEye::Point(1.0));
        assert_eq!(layered.layer_mvp(&mvp, 0), mvp);
        let raised = layered.layer_mvp(&mvp, 2);
        // Column 3 gains one unit of column 2; the other columns are unchanged.
        assert_eq!(raised[..12], mvp[..12]);
        assert_eq!(raised[12..], [22.0, 24.0, 26.0, 28.0]);
        // Plane 4 sits two units out whatever planes lie between it and the base.
        assert_eq!(layered.layer_mvp(&mvp, 4)[12..], [31.0, 34.0, 37.0, 40.0]);
    }
}
