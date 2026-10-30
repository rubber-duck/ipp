//! Analytic built-in Surface presentation. Covering quads bound work only; fragment
//! intersections supply content coordinates and scene depth, without a panel image.

use super::super::{
    canvas::{
        layering::{CanvasLayering, plane_matrix},
        scene::CanvasScene,
    },
    outputs::scene::SceneOutputSurface,
    statistics::RenderFrameWork,
};
use crate::{RenderDevice, RenderError, RenderService};
use ipp_core::systems::surface::{CylinderSurface, SphereSurface};
use ipp_core::{HostRuntime, OutputKind, WorldViewport, math};

#[derive(Clone, Copy)]
pub(in crate::services::render) struct AnalyticSurface {
    projection: [f32; 16],
    inverse: [f32; 16],
    parameters: [f32; 4],
}

fn mapping(surface: &SceneOutputSurface) -> Option<[f32; 2]> {
    let provider = surface.geometry.as_any();
    if let Some(cylinder) = provider.downcast_ref::<CylinderSurface>() {
        Some([1.0, cylinder.curvature])
    } else {
        provider
            .downcast_ref::<SphereSurface>()
            .map(|sphere| [2.0, sphere.curvature])
    }
}

pub(in crate::services::render) fn eligible(surface: &SceneOutputSurface) -> bool {
    surface.selection.kind() == OutputKind::Canvas
        && !surface.layered()
        && surface.cache_policy.is_none()
        && mapping(surface).is_some_and(|mapping| mapping[1] != 0.0)
}

/// Invert the completed clip transform in f64, including perspective and shear.
/// Partial pivoting avoids assumptions about camera or model matrix layout.
fn inverse(matrix: [f32; 16]) -> Result<[f32; 16], RenderError> {
    let mut rows = [[0.0_f64; 8]; 4];
    for row in 0..4 {
        for column in 0..4 {
            rows[row][column] = f64::from(matrix[column * 4 + row]);
        }
        rows[row][row + 4] = 1.0;
    }

    for column in 0..4 {
        let pivot = (column..4)
            .max_by(|&a, &b| rows[a][column].abs().total_cmp(&rows[b][column].abs()))
            .expect("nonempty rows");
        rows.swap(column, pivot);
        let divisor = rows[column][column];
        if divisor == 0.0 || !divisor.is_finite() {
            return Err(RenderError::InvalidTransform);
        }

        for value in &mut rows[column] {
            *value /= divisor;
        }

        for row in 0..4 {
            if row == column {
                continue;
            }

            let factor = rows[row][column];
            let pivot = rows[column];
            for (value, pivot_value) in rows[row].iter_mut().zip(pivot) {
                *value -= factor * pivot_value;
            }
        }
    }

    let result = std::array::from_fn(|i| rows[i % 4][4 + i / 4] as f32);
    if result.iter().any(|value| !value.is_finite()) {
        return Err(RenderError::InvalidTransform);
    }

    Ok(result)
}

pub(in crate::services::render) fn upload<D: RenderDevice>(
    surface: Option<&AnalyticSurface>,
    device: &mut D,
    program: &D::Program,
) -> Result<(), RenderError> {
    if let Some(surface) = surface {
        device.set_surface_intersection(
            program,
            &surface.projection,
            &surface.inverse,
            &surface.parameters,
        )
    } else {
        device.set_surface_intersection(program, &[0.0; 16], &[0.0; 16], &[0.0; 4])
    }
}

impl<D: RenderDevice> RenderService<D> {
    pub(in crate::services::render) fn draw_analytic_surface(
        &mut self,
        host: &HostRuntime,
        surface: &SceneOutputSurface,
        view_projection: [f32; 16],
        viewport: WorldViewport,
        stats: &mut RenderFrameWork,
    ) -> Result<(), RenderError> {
        let [kind, curvature] = mapping(surface).ok_or(RenderError::UnavailableOutput)?;
        let projection = math::multiply(view_projection, surface.model);
        let [w, h] = surface.geometry.physical_extent();
        let mut context = AnalyticSurface {
            projection,
            inverse: inverse(projection)?,
            parameters: [kind, curvature, 0.0, (w.hypot(h) * 0.5) as f32],
        };
        let scene = CanvasScene::new(host, surface.selection, surface.publication)?;
        let [cw, ch] = scene.canvas.logical_extent.map(f64::from);
        let content = plane_matrix([-w * 0.5, h * 0.5], [w / cw, -h / ch])?;
        let mvp = math::multiply(projection, content);
        let result = (|| {
            // A wrapped segment can intersect a ray twice. Compose the complete
            // far canvas before the near canvas; each keeps ordinary painter order.
            for branch in [0.0, 1.0] {
                context.parameters[2] = branch;
                self.analytic_surface = Some(context);
                self.draw_canvas(
                    &scene,
                    mvp,
                    scene.root_clip(),
                    1.0,
                    CanvasLayering::FLAT,
                    viewport,
                    stats,
                )?;
            }
            Ok(())
        })();
        self.analytic_surface = None;
        result
    }
}

#[cfg(test)]
#[path = "analytic_tests.rs"]
mod tests;
