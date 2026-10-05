use crate::systems::camera::CameraPublication;
use crate::systems::canvas::{CanvasPaintEntry, CanvasPublication};
use crate::systems::geometry::GeometryShapeTransform;
use crate::{
    ErrorReason, HostRuntime, OutputKind, OutputRef, ViewDescriptor, ViewQueryTarget,
    WorldAttachmentMode, WorldAttachmentToken, WorldPublicationId,
};

/// Stateless projection through an exact completed path; not capture ownership or input authority.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiProjectedPoint {
    /// Exact source view used for all mappings.
    pub root: ViewDescriptor,
    /// Final containing Camera or Canvas output.
    pub output: OutputRef,
    /// Source of the final output.
    pub publication: WorldPublicationId,
    /// Final normalized Camera viewport or Canvas logical coordinates.
    pub point: [f32; 2],
    /// Projection extent, independent of raster size for nested Cameras.
    pub extent: [f64; 2],
}

/// Project through producer-write tokens into the selected completed view.
/// Tokens select that view's current provider; they are not historical provider proofs.
/// Retained input targets separately fence provider incarnations before using this query.
/// Content continuation allows points outside rectangles when requested.
/// Front-facing and Camera near/far constraints always apply; there is no raw-coordinate fallback.
/// Gating does not erase readable geometry. The input owner must separately validate capture/gates.
///
/// Where a Surface in a camera's domain separates its canvas's layers, the ray
/// meets the shell of the layer the path continues on: the next nested slot's
/// plane, or `layer`, the target's current-publication plane ID in the final
/// canvas. The plane table supplies its offset, multiplied by spacing.
pub fn project_composed_point(
    host: &HostRuntime,
    target: ViewQueryTarget,
    path: &[WorldAttachmentToken],
    point: [f32; 2],
    captured: bool,
    layer: u32,
) -> Result<Option<GuiProjectedPoint>, ErrorReason> {
    if !point.iter().all(|value| value.is_finite()) {
        return Err(ErrorReason::InvalidViewport);
    }
    let root = host.resolve_view(target)?;
    let mut output = root.output;
    let mut output_source = root.publication;
    let mut publication = host
        .publication(root.publication)
        .ok_or(ErrorReason::InvalidEntity)?;
    let mut extent = [
        f64::from(root.viewport.width),
        f64::from(root.viewport.height),
    ];
    let mut point = point;
    let mut placement = GeometryShapeTransform::default();
    for (step, token) in path.iter().enumerate() {
        let edge = publication
            .attachments
            .iter()
            .find(|edge| &edge.token == token)
            .ok_or(ErrorReason::InvalidEntity)?;
        if edge.placement_output.is_some_and(|owner| owner != output) {
            return Err(ErrorReason::InvalidEntity);
        }
        let affine = GeometryShapeTransform::new(edge.placement)?.then(&placement)?;
        if edge.mode == WorldAttachmentMode::Spatial {
            if output.kind() != OutputKind::Camera {
                return Err(ErrorReason::InvalidEntity);
            }
            placement = affine;
            publication = host
                .attached_publication(edge)
                .ok_or(ErrorReason::InvalidEntity)?;
            continue;
        }
        let physical = edge.surface_extent.ok_or(ErrorReason::InvalidGeometry)?;
        if !physical
            .iter()
            .all(|value| value.is_finite() && *value > 0.0)
        {
            return Err(ErrorReason::InvalidGeometry);
        }
        let local = match output.kind() {
            OutputKind::Camera => {
                let camera = host
                    .output(output_source, output)
                    .and_then(|chunk| chunk.data::<CameraPublication>())
                    .ok_or(ErrorReason::InvalidEntity)?;
                let ray = camera.ray_for_extent(point, extent)?;
                let local = affine.inverse_ray(&ray.ray);
                let offset = surface_layer_offset(host, edge, path.get(step + 1), layer)?
                    * f64::from(edge.layer_spacing);
                let geometry = edge
                    .surface_geometry
                    .as_ref()
                    .ok_or(ErrorReason::InvalidGeometry)?;
                let child = host.attached_publication(edge);
                let offsets = super::surface_offset_range(host, edge, child);
                geometry.validate_offsets(offsets)?;
                let domain = if captured {
                    crate::systems::surface::SurfaceDomain::Continuation
                } else {
                    crate::systems::surface::SurfaceDomain::Content
                };
                let Some(hit) = geometry
                    .ray_intersections(&local, offset, domain)?
                    .into_iter()
                    .find(|hit| {
                        hit.distance >= ray.near
                            && hit.distance <= ray.far
                            && hit
                                .front_normal
                                .iter()
                                .zip(local.direction)
                                .map(|(normal, direction)| normal * direction)
                                .sum::<f64>()
                                < 0.0
                    })
                else {
                    return Ok(None);
                };
                [
                    hit.content[0] - physical[0] * 0.5,
                    physical[1] * 0.5 - hit.content[1],
                ]
            }
            OutputKind::Canvas => {
                if edge.placement_output != Some(output) {
                    return Err(ErrorReason::InvalidEntity);
                }
                let canvas = host
                    .output(output_source, output)
                    .and_then(|chunk| chunk.data::<CanvasPublication>())
                    .ok_or(ErrorReason::InvalidEntity)?;
                let logical = std::array::from_fn(|axis| point[axis] * canvas.logical_extent[axis]);
                let slot = canvas
                    .entries
                    .iter()
                    .find_map(|entry| match entry.as_ref() {
                        CanvasPaintEntry::Attachment(slot)
                            if slot.token == *token
                                && slot.anchor == edge.anchor
                                && slot.physical_extent == physical =>
                        {
                            Some(slot)
                        }
                        _ => None,
                    })
                    .ok_or(ErrorReason::InvalidEntity)?;
                if !captured && (slot.opacity <= 0.0 || !canvas.hits.iter().any(|hit| {
                    matches!(&hit.kind, crate::systems::canvas::CanvasHitKind::Attachment { token: hit_token, .. } if hit_token == token) && hit.contains(logical)
                })) { return Ok(None); }
                slot.from_canvas(logical.map(f64::from))
                    .ok_or(ErrorReason::InvalidGeometry)?
            }
        };
        let normalized = [local[0] / physical[0] + 0.5, 0.5 - local[1] / physical[1]];
        if !normalized.iter().all(|value| value.is_finite()) {
            return Err(ErrorReason::InvalidGeometry);
        }
        if !captured && !normalized.iter().all(|value| (0.0..1.0).contains(value)) {
            return Ok(None);
        }
        point = normalized.map(|value| value as f32);
        extent = physical;
        output = edge.output.ok_or(ErrorReason::InvalidEntity)?;
        publication = host
            .attached_publication(edge)
            .ok_or(ErrorReason::InvalidEntity)?;
        output_source = publication.id;
        placement = GeometryShapeTransform::default();
    }
    if output.kind() == OutputKind::Canvas {
        let canvas = host
            .output(output_source, output)
            .and_then(|chunk| chunk.data::<CanvasPublication>())
            .ok_or(ErrorReason::InvalidEntity)?;
        point = std::array::from_fn(|axis| point[axis] * canvas.logical_extent[axis]);
        if !captured
            && !point
                .iter()
                .enumerate()
                .all(|(axis, value)| *value >= 0.0 && *value < canvas.logical_extent[axis])
        {
            return Ok(None);
        }
    }
    Ok(Some(GuiProjectedPoint {
        root,
        output,
        publication: output_source,
        point,
        extent,
    }))
}

/// Current plane offset whose shell a camera-domain Surface edge projects onto: zero without
/// layer separation, the layer of the `next` slot the path enters in its
/// canvas, or the final target's `layer`.
fn surface_layer_offset(
    host: &HostRuntime,
    edge: &crate::PublishedWorldAttachment,
    next: Option<&WorldAttachmentToken>,
    layer: u32,
) -> Result<f64, ErrorReason> {
    if edge.layer_spacing == 0.0 {
        return Ok(0.0);
    }
    let child = host
        .attached_publication(edge)
        .ok_or(ErrorReason::InvalidEntity)?;
    let Some(canvas) = edge
        .output
        .and_then(|output| host.output(child.id, output))
        .and_then(|chunk| chunk.data::<CanvasPublication>())
    else {
        return Ok(0.0);
    };
    let plane = if let Some(next) = next {
        canvas
            .entries
            .iter()
            .find_map(|entry| match entry.as_ref() {
                CanvasPaintEntry::Attachment(slot) if slot.token == *next => Some(slot.layer),
                _ => None,
            })
            .ok_or(ErrorReason::InvalidEntity)?
    } else {
        layer
    };
    canvas.layer_offset(plane).ok_or(ErrorReason::InvalidEntity)
}
