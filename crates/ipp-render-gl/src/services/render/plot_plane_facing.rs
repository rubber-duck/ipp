//! View-dependent annotation placement over immutable Plot paint and glyph streams.

use super::RenderError;
use ipp_core::systems::plot::PlotPlaneFacing;

/// Compose first, then face this view: spatial attachments and entity transforms
/// place the exact data anchor, while positive basis lengths retain physical size.
/// Camera translation is irrelevant; using its right/up columns also keeps text
/// upright under roll and avoids mirroring from a reflected chart transform.
pub(super) fn model(
    mut model: [f32; 16],
    facing: PlotPlaneFacing,
    camera: [f32; 16],
) -> Result<[f32; 16], RenderError> {
    if facing == PlotPlaneFacing::Fixed {
        return Ok(model);
    }
    let length = |matrix: &[f32; 16], column: usize| {
        matrix[column]
            .hypot(matrix[column + 1])
            .hypot(matrix[column + 2])
    };
    let width = length(&model, 0);
    let height = length(&model, 4);
    let right = length(&camera, 0);
    let up = length(&camera, 4);
    if right == 0.0 || up == 0.0 || !right.is_finite() || !up.is_finite() {
        return Err(RenderError::InvalidTransform);
    }
    for axis in 0..3 {
        model[axis] = camera[axis] / right * width;
        model[4 + axis] = -camera[4 + axis] / up * height;
    }
    // Plane content has local z=0; retain a finite normal column for the ordinary
    // shared plane transform without changing its world-space anchor/depth.
    model[8] = model[1] * model[6] - model[2] * model[5];
    model[9] = model[2] * model[4] - model[0] * model[6];
    model[10] = model[0] * model[5] - model[1] * model[4];
    if model.iter().all(|value| value.is_finite()) {
        Ok(model)
    } else {
        Err(RenderError::InvalidTransform)
    }
}

#[cfg(test)]
#[path = "plot_plane_facing_tests.rs"]
mod tests;
