//! Selected-camera stations over retained Cartesian paint. Bases and increasing
//! data coordinates never reflect; composed chart depth chooses enclosure sides.

use super::{RenderError, scene::ScenePlotPlane};
use ipp_core::systems::plot::{PlotPlanePlacement, PlotPreparedGeometry};
use ipp_core::{WorldRef, WorldViewport, systems::canvas::CanvasTarget};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Weak};

// Composed f32 rotations can put the same edge-on view on either side of zero.
// Classify directions, not world distances: this angular tie band is invariant
// under chart/camera scale and comfortably covers composition roundoff.
const SUPPORT_TIE: f64 = 64.0 * f32::EPSILON as f64;

fn direction(matrix: [f32; 16], column: usize) -> [f64; 3] {
    let vector = std::array::from_fn(|row| f64::from(matrix[column + row]));
    let length = vector[0].hypot(vector[1]).hypot(vector[2]);
    if length > 0.0 {
        vector.map(|value| value / length)
    } else {
        [0.0; 3]
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|axis| a[axis] * b[axis]).sum()
}

pub(super) fn point(model: [f32; 16], local: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|row| {
        model[12 + row]
            + (0..3)
                .map(|col| model[col * 4 + row] * local[col])
                .sum::<f32>()
    })
}

pub(super) fn far_bounds(chart: [f32; 16], camera: [f32; 16], extent: [f32; 3]) -> [f32; 3] {
    let back = direction(camera, 8);
    std::array::from_fn(|axis| {
        // Indistinguishable far faces use local zero deterministically.
        if dot(direction(chart, axis * 4), back) < -SUPPORT_TIE {
            extent[axis]
        } else {
            0.0
        }
    })
}

pub(super) fn model(
    mut plane: [f32; 16],
    chart: [f32; 16],
    camera: [f32; 16],
    placement: PlotPlanePlacement,
) -> Result<[f32; 16], RenderError> {
    let (extent, moved): ([f32; 3], [bool; 3]) = match placement {
        PlotPlanePlacement::Grid {
            extent,
            normal,
        } => (
            extent,
            std::array::from_fn(|axis| axis == usize::from(normal)),
        ),
        _ => return Ok(plane),
    };
    let far = far_bounds(chart, camera, extent);
    for row in 0..3 {
        // All retained movable stations start at zero on their movable axes.
        plane[12 + row] += (0..3)
            .filter(|&axis| moved[axis])
            .map(|axis| chart[axis * 4 + row] * far[axis])
            .sum::<f32>();
    }
    if plane.iter().all(|value| value.is_finite()) {
        Ok(plane)
    } else {
        Err(RenderError::InvalidTransform)
    }
}

/// Presentation choices are centralized independently of the visibility estimator
/// and perimeter driver. Corners follow (low,low), (high,low), (high,high),
/// (low,high) in the two other axes' increasing order.
#[derive(Clone, Copy)]
pub(super) struct AxisPlacementPolicy {
    pub preferred: [u8; 3],
    pub departure: f32,
    pub returning: f32,
    pub alternative_gain: f32,
    /// Nonpositive or nonfinite durations settle immediately.
    pub duration: f64,
    pub label_clearance: f32,
    pub tick_clearance: f32,
}

impl Default for AxisPlacementPolicy {
    fn default() -> Self {
        Self {
            // X: front floor; Y: front left; Z: upper left.
            preferred: [3; 3],
            departure: 0.20,
            returning: 0.24,
            alternative_gain: 0.05,
            duration: 2.0,
            label_clearance: 0.9,
            tick_clearance: 0.7,
        }
    }
}

#[derive(Clone, Copy)]
struct AxisTransition {
    start: f64,
    end: f64,
    started: f64,
    target: u8,
}

impl AxisTransition {
    fn new(target: u8, time: f64) -> Self {
        Self {
            start: f64::from(target),
            end: f64::from(target),
            started: time,
            target,
        }
    }

    fn sample(self, time: f64, duration: f64) -> f64 {
        if !duration.is_finite() || duration <= 0.0 {
            return self.end;
        }
        let t = ((time - self.started) / duration).clamp(0.0, 1.0);
        self.start + (self.end - self.start) * t * t * (3.0 - 2.0 * t)
    }

    fn update(
        &mut self,
        target: u8,
        scores: [f32; 4],
        time: f64,
        policy: AxisPlacementPolicy,
        lengths: [f64; 2],
    ) {
        if target == self.target {
            return;
        }
        let current = self.sample(time, policy.duration);
        let forward = (f64::from(target) - current).rem_euclid(4.0);
        let backward = forward - 4.0;
        let position = perimeter_distance(current, lengths);
        let forward_length = (perimeter_distance(current + forward, lengths) - position).abs();
        let backward_length = (perimeter_distance(current + backward, lengths) - position).abs();
        let distance = if (forward_length - backward_length).abs()
            <= (forward_length + backward_length) * 1e-6
        {
            // Opposite corners have equal route lengths. Prefer the clearer
            // adjacent face, retaining a fixed increasing-direction tie.
            let positive = (current.floor() as i64 + 1).rem_euclid(4) as usize;
            let negative = (current.ceil() as i64 - 1).rem_euclid(4) as usize;
            if scores[negative] > scores[positive] + f32::EPSILON {
                backward
            } else {
                forward
            }
        } else if forward_length <= backward_length {
            forward
        } else {
            backward
        };
        self.start = current.rem_euclid(4.0);
        self.end = self.start + distance;
        self.started = time;
        self.target = target;
    }
}

struct ChartPlacement {
    source: Weak<PlotPreparedGeometry>,
    content: Option<[[f32; 3]; 2]>,
    axes: [Option<AxisTransition>; 3],
}

/// Exact view state is owned by the renderer's existing per-output layout cache.
/// Source replacement refreshes the content estimate, never a live chart's driver.
#[derive(Default)]
pub(super) struct PlotViewPlacementState {
    charts: BTreeMap<(WorldRef, CanvasTarget), ChartPlacement>,
    time: Option<f64>,
    policy: AxisPlacementPolicy,
}

impl PlotViewPlacementState {
    pub(super) fn policy(&self) -> AxisPlacementPolicy {
        self.policy
    }

    pub(super) fn place<'a>(
        &mut self,
        planes: &[ScenePlotPlane<'a>],
        camera: [f32; 16],
        projection: [f32; 16],
        viewport: WorldViewport,
        time: f64,
    ) -> Result<Vec<ScenePlotPlane<'a>>, RenderError> {
        if self.time.is_some_and(|previous| time < previous) {
            self.charts.clear();
        }
        self.time = Some(time);
        let mut active = BTreeSet::new();
        let mut stations = BTreeMap::new();
        let mut arranged = planes.to_vec();
        for plane in &mut arranged {
            let PlotPlanePlacement::Axis {
                extent,
                axis,
            } = plane.plane.placement
            else {
                continue;
            };
            let key = (plane.entity.world, plane.target);
            active.insert(key);
            let station = stations.entry((key, axis)).or_insert_with(|| {
                let chart = self.charts.entry(key).or_insert_with(|| ChartPlacement {
                    source: Weak::new(),
                    content: None,
                    axes: [None; 3],
                });
                if !chart
                    .source
                    .upgrade()
                    .is_some_and(|source| Arc::ptr_eq(&source, plane.geometry))
                {
                    chart.source = Arc::downgrade(plane.geometry);
                    chart.content = content_bounds(plane.geometry);
                }
                let scores = visibility_scores(
                    plane.chart_model,
                    camera,
                    projection,
                    viewport,
                    extent,
                    axis,
                    chart.content,
                );
                let standard = self.policy.preferred[usize::from(axis)];
                let driver = &mut chart.axes[usize::from(axis)];
                let target = choose_edge(
                    scores,
                    standard,
                    driver.map(|driver| driver.target),
                    self.policy,
                );
                let driver = driver.get_or_insert_with(|| AxisTransition::new(target, time));
                let lengths = other_axes(axis).map(|other| {
                    let column = other * 4;
                    f64::from(plane.chart_model[column])
                        .hypot(f64::from(plane.chart_model[column + 1]))
                        .hypot(f64::from(plane.chart_model[column + 2]))
                        * f64::from(extent[other])
                });
                driver.update(target, scores, time, self.policy, lengths);
                perimeter_point(driver.sample(time, self.policy.duration), extent, axis)
            });
            let mut model =
                ipp_core::systems::camera::multiply(plane.chart_model, plane.plane.model);
            for row in 0..3 {
                model[12 + row] += (0..3)
                    .map(|col| plane.chart_model[col * 4 + row] * station[col])
                    .sum::<f32>();
            }
            plane.model = super::plot_plane_facing::model(model, plane.plane.facing, camera)?;
        }
        self.charts.retain(|key, _| active.contains(key));
        Ok(arranged)
    }
}

fn choose_edge(
    scores: [f32; 4],
    standard: u8,
    current: Option<u8>,
    policy: AxisPlacementPolicy,
) -> u8 {
    let current = current.unwrap_or(standard);
    if (current == standard && scores[usize::from(standard)] >= policy.departure)
        || (current != standard && scores[usize::from(standard)] > policy.returning)
    {
        return standard;
    }
    let mut best = current;
    for candidate in 0..4u8 {
        if current != standard && candidate == standard {
            continue;
        }
        if scores[usize::from(candidate)] > scores[usize::from(best)] + f32::EPSILON {
            best = candidate;
        }
    }
    if scores[usize::from(best)] >= scores[usize::from(current)] + policy.alternative_gain {
        best
    } else {
        current
    }
}

fn other_axes(axis: u8) -> [usize; 2] {
    match axis {
        0 => [1, 2],
        1 => [0, 2],
        _ => [0, 1],
    }
}

fn perimeter_distance(phase: f64, lengths: [f64; 2]) -> f64 {
    let corner = phase.rem_euclid(4.0);
    let fraction = corner.fract();
    let fraction = fraction * fraction * (3.0 - 2.0 * fraction);
    let segment = corner.floor() as usize;
    (phase / 4.0).floor() * 2.0 * (lengths[0] + lengths[1])
        + [
            0.0,
            lengths[0],
            lengths[0] + lengths[1],
            lengths[0] * 2.0 + lengths[1],
        ][segment]
        + lengths[segment % 2] * fraction
}

fn perimeter_point(phase: f64, extent: [f32; 3], axis: u8) -> [f32; 3] {
    let phase = phase.rem_euclid(4.0);
    let t = phase.fract() as f32;
    // Ease each face independently. Velocity reaches zero at the corner without
    // replacing the required right-angle route with a curve through the volume.
    let t = t * t * (3.0 - 2.0 * t);
    let coordinate = match phase.floor() as u8 {
        0 => [t, 0.0],
        1 => [1.0, t],
        2 => [1.0 - t, 1.0],
        _ => [0.0, 1.0 - t],
    };
    let other = other_axes(axis);
    let mut point = [0.0; 3];
    for index in 0..2 {
        point[other[index]] = coordinate[index] * extent[other[index]];
    }
    point
}

/// A once-per-publication estimate excludes frame planes and annotation ink.
/// Gaps in scattered marks can be overestimated; no exact triangle occlusion is claimed.
fn content_bounds(geometry: &PlotPreparedGeometry) -> Option<[[f32; 3]; 2]> {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for point in geometry.meshes.iter().flat_map(|mesh| &mesh.positions) {
        for axis in 0..3 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
        }
    }
    min.iter()
        .all(|value| value.is_finite())
        .then_some([min, max])
}

/// Bounded estimator: five stations per candidate edge, with directional
/// foreshortening, viewport clipping and conservative content-box ray occlusion.
fn visibility_scores(
    chart: [f32; 16],
    camera: [f32; 16],
    projection: [f32; 16],
    viewport: WorldViewport,
    extent: [f32; 3],
    axis: u8,
    content: Option<[[f32; 3]; 2]>,
) -> [f32; 4] {
    let Ok(inverse) =
        ipp_core::systems::geometry::GeometryShapeTransform::new(chart.map(f64::from))
    else {
        return [0.0; 4];
    };
    let eye = [camera[12], camera[13], camera[14]].map(f64::from);
    let back = direction(camera, 8);
    // Mark radii/widths can extend beyond the authored enclosure. The estimator
    // concerns content inside that enclosure; overhang must not classify every
    // boundary station as strictly inside the content box.
    let content = content.map(|bounds| {
        [
            std::array::from_fn(|axis| bounds[0][axis].max(0.0)),
            std::array::from_fn(|axis| bounds[1][axis].min(extent[axis])),
        ]
    });
    let perspective = [projection[3], projection[7], projection[11]]
        .iter()
        .any(|value| *value != 0.0);
    let axis_direction = direction(chart, usize::from(axis) * 4);
    std::array::from_fn(|corner| {
        let mut station = perimeter_point(corner as f64, extent, axis);
        let mut visibility = 0.0;
        for fraction in [0.1, 0.3, 0.5, 0.7, 0.9] {
            station[usize::from(axis)] = extent[usize::from(axis)] * fraction;
            let world = point(chart, station);
            let Some(pixel) = super::plot_label_layout::project(
                world,
                projection,
                [viewport.width, viewport.height],
            ) else {
                continue;
            };
            if pixel[0] < 0.0
                || pixel[1] < 0.0
                || pixel[0] >= viewport.width as f32
                || pixel[1] >= viewport.height as f32
            {
                continue;
            }
            let ray = if perspective {
                std::array::from_fn(|row| eye[row] - f64::from(world[row]))
            } else {
                back
            };
            let length = ray[0].hypot(ray[1]).hypot(ray[2]);
            if length <= 0.0 {
                continue;
            }
            let ray = ray.map(|value| value / length);
            let reach = if perspective {
                length
            } else {
                f64::INFINITY
            };
            let hidden = content.is_some_and(|bounds| {
                hidden_by_content(station, inverse.inverse_vector(ray), bounds, reach)
            });
            if !hidden {
                visibility += (1.0 - dot(axis_direction, ray).powi(2)).max(0.0).sqrt() as f32 / 5.0;
            }
        }
        visibility.clamp(0.0, 1.0)
    })
}

fn hidden_by_content(station: [f32; 3], ray: [f64; 3], bounds: [[f32; 3]; 2], reach: f64) -> bool {
    let mut entry = 0.0_f64;
    let mut exit = reach;
    for axis in 0..3 {
        // Strict interior avoids treating an axis on a silhouette boundary as
        // occluded. Relative shrinkage also handles flat height/row envelopes.
        let margin = f64::from(bounds[1][axis] - bounds[0][axis]) * 1e-5;
        let min = f64::from(bounds[0][axis]) + margin;
        let max = f64::from(bounds[1][axis]) - margin;
        if min >= max {
            return false;
        }
        let origin = f64::from(station[axis]);
        if ray[axis].abs() <= f64::EPSILON {
            if origin <= min || origin >= max {
                return false;
            }
        } else {
            let a = (min - origin) / ray[axis];
            let b = (max - origin) / ray[axis];
            entry = entry.max(a.min(b));
            exit = exit.min(a.max(b));
        }
    }
    exit > entry && exit > 0.0
}

#[cfg(test)]
#[path = "plot_view_placement_tests.rs"]
mod tests;
