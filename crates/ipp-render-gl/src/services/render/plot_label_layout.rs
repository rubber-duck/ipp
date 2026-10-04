//! Bounded view-local placement over retained Plot ink, panels and connector strips.
//!
//! No authored values or glyph/path streams change. Stable source order, local
//! candidates and a small retained offset preference avoid flicker under small
//! camera movement. History is scoped to the exact camera output and viewport,
//! with weak geometry ownership; it cannot retain a chart or cross a view lifetime.
//! Projected bounds belong to this presentation. Arranged planes are submitted
//! independently of headless mesh/mark bounds, which remain picking authority;
//! future plane culling must use these arranged bounds, never that enclosure.
//! Only ink intersecting this view participates in fitting and collision history.
//! Offscreen ink keeps its retained placement and ordinary GPU clipping; fitting
//! must not turn an invisible neighbouring chart into a viewport annotation.

use super::scene::ScenePlotPlane;
use ipp_core::systems::canvas::CanvasTarget;
use ipp_core::systems::plot::{PlotPlaneLayout, PlotPlanePlacement, PlotPreparedGeometry};
use ipp_core::{WorldRef, WorldViewport};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::{Arc, Weak};

type Key = (WorldRef, CanvasTarget, u32);
const PADDING: f32 = 4.0;
const HYSTERESIS: f32 = 6.0;
const RINGS: usize = 12;

struct Previous {
    source: Weak<PlotPreparedGeometry>,
    offset: [f32; 2],
}

#[derive(Default)]
pub(super) struct PlotLabelLayoutState {
    viewport: [u32; 2],
    previous: BTreeMap<Key, Previous>,
}

impl PlotLabelLayoutState {
    pub(super) fn arrange<'a>(
        &mut self,
        planes: &[ScenePlotPlane<'a>],
        projection: [f32; 16],
        viewport: WorldViewport,
    ) -> Vec<ScenePlotPlane<'a>> {
        let size = [viewport.width, viewport.height];
        if self.viewport != size {
            self.previous.clear();
            self.viewport = size;
        }
        let mut arranged = planes.to_vec();
        let mut order: Vec<_> = (0..planes.len())
            .filter(|&index| {
                matches!(
                    planes[index].plane.layout,
                    PlotPlaneLayout::Tick(_) | PlotPlaneLayout::Title(_) | PlotPlaneLayout::Callout
                )
            })
            .collect();
        order.sort_by_key(|&index| {
            let plane = &planes[index];
            let priority = match plane.plane.layout {
                PlotPlaneLayout::Tick(_) => 0,
                PlotPlaneLayout::Title(_) => 1,
                _ => 2,
            };
            (priority, plane.entity.world, plane.target, plane.plane.part)
        });
        let mut occupied = Occupancy::default();
        let mut next = BTreeMap::new();
        for index in order {
            let mut staged = planes[index].clone();
            let plane = &staged;
            let Some(local_bounds) = plane.plane.bounds else {
                continue;
            };
            let Some(bounds) = project_bounds(plane.model, local_bounds, projection, size) else {
                continue;
            };
            if bounds[2] <= 0.0
                || bounds[3] <= 0.0
                || bounds[0] >= size[0] as f32
                || bounds[1] >= size[1] as f32
            {
                continue;
            }
            let perimeter = axis_perimeter(plane, bounds, projection, size);
            if let Some((offset, _)) = perimeter
                && let Some(model) = shifted_model(staged.model, offset, projection, size)
            {
                staged.model = model;
            }
            let plane = &staged;
            let Some(bounds) = project_bounds(plane.model, local_bounds, projection, size) else {
                continue;
            };
            let key = (plane.entity.world, plane.target, plane.plane.part);
            let old = self
                .previous
                .get(&key)
                .filter(|old| {
                    old.source
                        .upgrade()
                        .is_some_and(|source| Arc::ptr_eq(&source, plane.geometry))
                })
                .map(|old| old.offset);
            let offset = if let Some((preferred, direction)) =
                radial_station(plane, bounds, projection, size)
            {
                place_radial(bounds, preferred, direction, size, &occupied, old)
            } else {
                if let Some((_, normal)) = perimeter {
                    if matches!(plane.plane.layout, PlotPlaneLayout::Tick(_)) {
                        place_tick(bounds, normal, size, &occupied, old)
                    } else {
                        place_constrained(
                            bounds,
                            plane.plane.layout,
                            size,
                            &occupied,
                            old,
                            Some(normal),
                        )
                    }
                } else {
                    place(bounds, plane.plane.layout, size, &occupied, old)
                }
            };
            occupied.insert(translated(bounds, offset));
            if let Some(model) = shifted_model(plane.model, offset, projection, size) {
                arranged[index].model = model;
                next.insert(
                    key,
                    Previous {
                        source: Arc::downgrade(plane.geometry),
                        offset,
                    },
                );
            }
        }
        self.previous = next;
        // Panel movement changes just the unit strip's presentation affine. The
        // first endpoint remains the exact data station, regardless of layout.
        for index in 0..arranged.len() {
            let PlotPlaneLayout::Connector {
                panel,
                endpoint,
                width,
            } = arranged[index].plane.layout
            else {
                continue;
            };
            let connector = &planes[index];
            if let Some(target) = arranged.iter().find(|candidate| {
                candidate.entity == connector.entity
                    && candidate.target == connector.target
                    && candidate.plane.part == panel
            }) {
                if !self.previous.contains_key(&(
                    target.entity.world,
                    target.target,
                    target.plane.part,
                )) {
                    continue;
                }
                let endpoint =
                    if matches!(target.plane.placement, PlotPlanePlacement::Radial { .. }) {
                        // Attach on the panel edge facing its data station, never on
                        // the old authored corner. The retained strip is unchanged.
                        target.plane.bounds.map_or(endpoint, |bounds| {
                            panel_edge(connector.model, target.model, bounds)
                        })
                    } else {
                        endpoint
                    };
                let model = connector_model(connector.model, target.model, endpoint, width);
                arranged[index].model = model;
            }
        }
        arranged
    }
}

/// Put numeric stations in a parallel projected perimeter lane. Only text moves:
/// the perimeter axis paint and its increasing data coordinates remain untouched. A
/// support line over eight retained frame corners clears ordinary data depth.
fn axis_perimeter(
    plane: &ScenePlotPlane<'_>,
    bounds: [f32; 4],
    projection: [f32; 16],
    size: [u32; 2],
) -> Option<([f32; 2], [f32; 2])> {
    let PlotPlanePlacement::Axis {
        extent,
        axis,
    } = plane.plane.placement
    else {
        return None;
    };
    let station = point(plane.model, [0.0; 2]);
    let origin = project(station, projection, size)?;
    let end = project(
        std::array::from_fn(|row| {
            station[row]
                + plane.chart_model[usize::from(axis) * 4 + row] * extent[usize::from(axis)]
        }),
        projection,
        size,
    )?;
    let vector = [end[0] - origin[0], end[1] - origin[1]];
    let length = vector[0].hypot(vector[1]);
    if length <= 0.5 {
        return None;
    }
    let normal = [vector[1] / length, -vector[0] / length];
    let mut corners = [[0.0; 2]; 8];
    for (index, corner) in corners.iter_mut().enumerate() {
        *corner = project(
            super::plot_view_placement::point(
                plane.chart_model,
                std::array::from_fn(|axis| {
                    if index & (1 << axis) == 0 {
                        0.0
                    } else {
                        extent[axis]
                    }
                }),
            ),
            projection,
            size,
        )?;
    }
    let height = (bounds[3] - bounds[1]).max(1.0);
    let padding = (height
        * if matches!(plane.plane.layout, PlotPlaneLayout::Title(_)) {
            3.0
        } else {
            0.9
        })
    .max(PADDING);
    // One midpoint chooses the side for every tick and title on this axis;
    // individual string width cannot split a lane or choose its opposite side.
    let midpoint = project(
        std::array::from_fn(|row| {
            station[row]
                + plane.chart_model[usize::from(axis) * 4 + row]
                    * (extent[usize::from(axis)] * 0.5 - plane.plane.model[12 + usize::from(axis)])
        }),
        projection,
        size,
    )?;
    let (_, normal) = perimeter_station(
        [midpoint[0], midpoint[1], midpoint[0], midpoint[1]],
        &corners,
        normal,
        0.0,
    );
    let mut offset = if axis == 1 {
        perimeter_offset(bounds, &corners, normal, padding)
    } else {
        [0.0; 2]
    };
    if axis == 1 && matches!(plane.plane.layout, PlotPlaneLayout::Tick(1)) {
        // Text uses a top-origin font position. Center its prepared ink on the
        // physical tick along +Y, independently of the perpendicular lane.
        let centered = tick_center_offset(bounds, origin, [vector[0] / length, vector[1] / length]);
        offset = [offset[0] + centered[0], offset[1] + centered[1]];
    }
    Some((offset, normal))
}

fn tick_center_offset(bounds: [f32; 4], station: [f32; 2], axis: [f32; 2]) -> [f32; 2] {
    let center = [(bounds[0] + bounds[2]) * 0.5, (bounds[1] + bounds[3]) * 0.5];
    let distance = (station[0] - center[0]) * axis[0] + (station[1] - center[1]) * axis[1];
    [axis[0] * distance, axis[1] * distance]
}

fn perimeter_station(
    bounds: [f32; 4],
    corners: &[[f32; 2]],
    normal: [f32; 2],
    padding: f32,
) -> ([f32; 2], [f32; 2]) {
    let positive = perimeter_offset(bounds, corners, normal, padding);
    let opposite = [-normal[0], -normal[1]];
    let negative = perimeter_offset(bounds, corners, opposite, padding);
    // A deterministic tie band avoids numerical side flips at symmetric views.
    if negative[0].hypot(negative[1]) + HYSTERESIS < positive[0].hypot(positive[1]) {
        (negative, opposite)
    } else {
        (positive, normal)
    }
}

fn perimeter_offset(
    bounds: [f32; 4],
    corners: &[[f32; 2]],
    normal: [f32; 2],
    padding: f32,
) -> [f32; 2] {
    let support = corners
        .iter()
        .map(|p| p[0] * normal[0] + p[1] * normal[1])
        .fold(f32::NEG_INFINITY, f32::max);
    let ink = [
        if normal[0] >= 0.0 {
            bounds[0]
        } else {
            bounds[2]
        },
        if normal[1] >= 0.0 {
            bounds[1]
        } else {
            bounds[3]
        },
    ];
    let length = (support + padding - ink[0] * normal[0] - ink[1] * normal[1]).max(0.0);
    [normal[0] * length, normal[1] * length]
}

/// Project the bisector at its own slice height: extrusion is not radial direction.
fn radial_station(
    plane: &ScenePlotPlane<'_>,
    bounds: [f32; 4],
    projection: [f32; 16],
    size: [u32; 2],
) -> Option<([f32; 2], [f32; 2])> {
    let PlotPlanePlacement::Radial {
        center,
        rim,
        spacing,
    } = plane.plane.placement
    else {
        return None;
    };
    let center = project(
        super::plot_view_placement::point(plane.chart_model, center),
        projection,
        size,
    )?;
    let rim = project(
        super::plot_view_placement::point(plane.chart_model, rim),
        projection,
        size,
    )?;
    let delta = [rim[0] - center[0], rim[1] - center[1]];
    let length = delta[0].hypot(delta[1]);
    // Edge-on bisectors have no usable projected ray. Stable part parity chooses
    // an outward horizontal lane without dividing by a tiny vector.
    let direction = if length > 0.5 {
        [delta[0] / length, delta[1] / length]
    } else {
        [
            if plane.plane.part.is_multiple_of(2) {
                1.0
            } else {
                -1.0
            },
            0.0,
        ]
    };
    let half = [(bounds[2] - bounds[0]) * 0.5, (bounds[3] - bounds[1]) * 0.5];
    // Leave a readable gap beyond the rim even when the extruded slice
    // silhouette projects below its top. This is one bounded style clearance.
    let margin = (half[1] * 4.0).max(PADDING);
    let origin = project(point(plane.model, [0.0; 2]), projection, size)?;
    let unit = project(point(plane.model, [1.0, 0.0]), projection, size)?;
    let spacing = spacing * (unit[0] - origin[0]).hypot(unit[1] - origin[1]);
    let reach = direction[0].abs() * half[0] + direction[1].abs() * half[1] + margin + spacing;
    let panel = [(bounds[0] + bounds[2]) * 0.5, (bounds[1] + bounds[3]) * 0.5];
    Some((
        std::array::from_fn(|axis| rim[axis] + direction[axis] * reach - panel[axis]),
        direction,
    ))
}

/// Only extend along the slice ray. Angular/side order cannot change through a
/// generic collision offset, and connector rays stay on their slice's side.
fn place_radial(
    bounds: [f32; 4],
    preferred: [f32; 2],
    direction: [f32; 2],
    size: [u32; 2],
    occupied: &Occupancy,
    previous: Option<[f32; 2]>,
) -> [f32; 2] {
    let height = (bounds[3] - bounds[1]).max(1.0);
    let padding = (height * 0.7).max(PADDING);
    let score = |offset: [f32; 2]| {
        let candidate = translated(bounds, offset);
        let overflow = (padding - candidate[0]).max(0.0)
            + (padding - candidate[1]).max(0.0)
            + (candidate[2] - size[0] as f32 + padding).max(0.0)
            + (candidate[3] - size[1] as f32 + padding).max(0.0);
        (occupied.intersections(candidate, padding) + overflow * height) * 10000.0
            + (offset[0] - preferred[0]).hypot(offset[1] - preferred[1])
    };
    let mut best = preferred;
    let mut best_score = score(best);
    for ring in 1..=RINGS {
        let distance = (height + padding) * ring as f32;
        let candidate = std::array::from_fn(|axis| preferred[axis] + direction[axis] * distance);
        let value = score(candidate);
        if value < best_score {
            best = candidate;
            best_score = value;
        }
    }
    // History only influences radial distance. Camera movement never retains a
    // stale tangential offset or crosses to the other projected slice side.
    if let Some(old) = previous {
        let distance = ((old[0] - preferred[0]) * direction[0]
            + (old[1] - preferred[1]) * direction[1])
            .max(0.0);
        let old = std::array::from_fn(|axis| preferred[axis] + direction[axis] * distance);
        if score(old) <= best_score + HYSTERESIS {
            return old;
        }
    }
    best
}

/// Numeric text may move across its axis lane, never along the axis. This
/// preserves station association and increasing order even during collision fit.
fn place_tick(
    bounds: [f32; 4],
    normal: [f32; 2],
    size: [u32; 2],
    occupied: &Occupancy,
    previous: Option<[f32; 2]>,
) -> [f32; 2] {
    place_radial(bounds, [0.0; 2], normal, size, occupied, previous)
}

fn panel_edge(anchor: [f32; 16], panel: [f32; 16], bounds: [f32; 4]) -> [f32; 2] {
    let local: [f32; 2] = std::array::from_fn(|col| {
        let basis: [f32; 3] = std::array::from_fn(|row| panel[col * 4 + row]);
        let squared = basis.iter().map(|value| value * value).sum::<f32>();
        (0..3)
            .map(|row| (anchor[12 + row] - panel[12 + row]) * basis[row])
            .sum::<f32>()
            / squared
    });
    let center = [(bounds[0] + bounds[2]) * 0.5, (bounds[1] + bounds[3]) * 0.5];
    let delta = [local[0] - center[0], local[1] - center[1]];
    let half = [(bounds[2] - bounds[0]) * 0.5, (bounds[3] - bounds[1]) * 0.5];
    let scale = (half[0] / delta[0].abs()).min(half[1] / delta[1].abs());
    if scale.is_finite() {
        std::array::from_fn(|axis| center[axis] + delta[axis] * scale)
    } else {
        center
    }
}

/// Local broad phase only, not a graph layout or geometric authority. Large
/// rectangles use one fallback list, so neither enormous coordinates nor long
/// labels generate unbounded cell loops. Float sums follow insertion order.
#[derive(Default)]
struct Occupancy {
    rectangles: Vec<[f32; 4]>,
    cells: BTreeMap<(i32, i32), Vec<usize>>,
    wide: Vec<usize>,
    // Candidate scoring repeats up to 97 times per label. Reuse temporary
    // indices within this arrangement; sorted order keeps float sums identical.
    candidates: RefCell<Vec<usize>>,
}

impl Occupancy {
    fn cells(bounds: [f32; 4]) -> Option<([i32; 2], [i32; 2])> {
        const CELL: f32 = 64.0;
        let min = [
            (bounds[0] / CELL).floor() as i32,
            (bounds[1] / CELL).floor() as i32,
        ];
        let max = [
            (bounds[2] / CELL).floor() as i32,
            (bounds[3] / CELL).floor() as i32,
        ];
        let width = i64::from(max[0]) - i64::from(min[0]) + 1;
        let height = i64::from(max[1]) - i64::from(min[1]) + 1;
        (width > 0 && height > 0 && width <= 64 && height <= 64 && width * height <= 64)
            .then_some((min, max))
    }

    fn insert(&mut self, bounds: [f32; 4]) {
        let index = self.rectangles.len();
        self.rectangles.push(bounds);
        if let Some((min, max)) = Self::cells(bounds) {
            for x in min[0]..=max[0] {
                for y in min[1]..=max[1] {
                    self.cells.entry((x, y)).or_default().push(index);
                }
            }
        } else {
            self.wide.push(index);
        }
    }

    fn intersections(&self, bounds: [f32; 4], padding: f32) -> f32 {
        let expanded = [
            bounds[0] - padding,
            bounds[1] - padding,
            bounds[2] + padding,
            bounds[3] + padding,
        ];
        let Some((min, max)) = Self::cells(expanded) else {
            return self
                .rectangles
                .iter()
                .map(|&other| overlap(bounds, other, padding))
                .sum();
        };
        let mut candidates = self.candidates.borrow_mut();
        candidates.clear();
        candidates.extend_from_slice(&self.wide);
        for x in min[0]..=max[0] {
            for y in min[1]..=max[1] {
                if let Some(cell) = self.cells.get(&(x, y)) {
                    candidates.extend(cell);
                }
            }
        }
        candidates.sort_unstable();
        candidates.dedup();
        candidates
            .iter()
            .map(|&index| overlap(bounds, self.rectangles[index], padding))
            .sum()
    }
}

fn translated(bounds: [f32; 4], offset: [f32; 2]) -> [f32; 4] {
    [
        bounds[0] + offset[0],
        bounds[1] + offset[1],
        bounds[2] + offset[0],
        bounds[3] + offset[1],
    ]
}

fn overlap(a: [f32; 4], b: [f32; 4], padding: f32) -> f32 {
    let width = (a[2].min(b[2]) - a[0].max(b[0]) + padding).max(0.0);
    let height = (a[3].min(b[3]) - a[1].max(b[1]) + padding).max(0.0);
    width * height
}

/// Fixed source order and bounded candidates keep work and tie decisions explicit.
/// If a viewport is physically too crowded, retain every label at the least
/// overlapping candidate: no hiding, truncation or font-size substitution.
fn place(
    bounds: [f32; 4],
    role: PlotPlaneLayout,
    size: [u32; 2],
    occupied: &Occupancy,
    previous: Option<[f32; 2]>,
) -> [f32; 2] {
    place_constrained(bounds, role, size, occupied, previous, None)
}

fn place_constrained(
    bounds: [f32; 4],
    role: PlotPlaneLayout,
    size: [u32; 2],
    occupied: &Occupancy,
    previous: Option<[f32; 2]>,
    outward: Option<[f32; 2]>,
) -> [f32; 2] {
    let height = (bounds[3] - bounds[1]).max(1.0);
    // Ink-relative padding survives a composed camera texture's supersampling
    // and downsampling, unlike a fixed number of offscreen texture pixels.
    let padding = (height * 0.7).max(PADDING)
        * if matches!(role, PlotPlaneLayout::Title(_)) {
            1.5
        } else {
            1.0
        };
    let lane = match role {
        PlotPlaneLayout::Title(1) => [0.0, -(height + padding)],
        PlotPlaneLayout::Title(_) => [0.0, height + padding],
        _ => [0.0; 2],
    };
    let directions: [[f32; 2]; 8] = match role {
        PlotPlaneLayout::Tick(1) | PlotPlaneLayout::Title(1) => [
            [-1.0, 0.0],
            [0.0, -1.0],
            [-1.0, -1.0],
            [0.0, 1.0],
            [-1.0, 1.0],
            [1.0, 0.0],
            [1.0, -1.0],
            [1.0, 1.0],
        ],
        PlotPlaneLayout::Tick(2) | PlotPlaneLayout::Title(2) => [
            [1.0, 0.0],
            [0.0, 1.0],
            [1.0, 1.0],
            [0.0, -1.0],
            [1.0, -1.0],
            [-1.0, 0.0],
            [-1.0, 1.0],
            [-1.0, -1.0],
        ],
        _ => [
            [0.0, 1.0],
            [0.0, -1.0],
            [1.0, 0.0],
            [-1.0, 0.0],
            [1.0, 1.0],
            [-1.0, 1.0],
            [1.0, -1.0],
            [-1.0, -1.0],
        ],
    };
    let score = |offset: [f32; 2]| {
        let candidate = translated(bounds, offset);
        let overflow = (padding - candidate[0]).max(0.0)
            + (padding - candidate[1]).max(0.0)
            + (candidate[2] - size[0] as f32 + padding).max(0.0)
            + (candidate[3] - size[1] as f32 + padding).max(0.0);
        let intersections = occupied.intersections(candidate, padding);
        // Prefer local movement only after clear bounds and viewport fit. The
        // minimum violation fallback still renders all labels if no fit exists.
        let distance = (offset[0] - lane[0]).hypot(offset[1] - lane[1]);
        (intersections + overflow * height) * 10000.0 + distance
    };
    let step = (height + padding).clamp(8.0, 28.0);
    let clamp = |mut candidate: [f32; 2]| {
        for axis in 0..2 {
            let low = padding - bounds[axis];
            let high = size[axis] as f32 - padding - bounds[axis + 2];
            if low <= high {
                let fitted = candidate[axis].clamp(low, high);
                // Fitting is local too: an offscreen authored station never
                // drags an unrelated far-away label across the entire view.
                if (fitted - candidate[axis]).abs() <= step * RINGS as f32 {
                    candidate[axis] = fitted;
                }
            }
        }
        if let Some(normal) = outward {
            let inward = (candidate[0] * normal[0] + candidate[1] * normal[1]).min(0.0);
            for axis in 0..2 {
                candidate[axis] -= normal[axis] * inward;
            }
        }
        candidate
    };
    let mut best = clamp(lane);
    let mut best_score = score(best);
    for ring in 1..=RINGS {
        for direction in directions {
            let candidate = clamp([
                lane[0] + direction[0] * step * ring as f32,
                lane[1] + direction[1] * step * ring as f32,
            ]);
            let value = score(candidate);
            if value < best_score {
                best = candidate;
                best_score = value;
            }
        }
    }
    // Do not jump between equally good local lanes for a subpixel camera change.
    if let Some(old) = previous
        .map(clamp)
        .filter(|old| score(*old) <= best_score + HYSTERESIS)
    {
        old
    } else {
        best
    }
}

fn point(model: [f32; 16], local: [f32; 2]) -> [f32; 3] {
    std::array::from_fn(|axis| {
        model[12 + axis] + model[axis] * local[0] + model[4 + axis] * local[1]
    })
}

fn project(world: [f32; 3], projection: [f32; 16], size: [u32; 2]) -> Option<[f32; 2]> {
    let clip: [f32; 4] = std::array::from_fn(|row| {
        projection[row] * world[0]
            + projection[4 + row] * world[1]
            + projection[8 + row] * world[2]
            + projection[12 + row]
    });
    // Layout uses the same depth interval as GL clipping. Positive W alone
    // admits near-eye stations with enormous, but finite, projected offsets.
    // Keep X/Y unrestricted here: offscreen frame corners still define the
    // perimeter of a partially visible chart, and ink may straddle a view edge.
    if clip[3] <= 0.0
        || !clip.iter().all(|value| value.is_finite())
        || clip[2] < -clip[3]
        || clip[2] > clip[3]
    {
        return None;
    }
    let pixel = [
        (clip[0] / clip[3] + 1.0) * size[0] as f32 * 0.5,
        (1.0 - clip[1] / clip[3]) * size[1] as f32 * 0.5,
    ];
    pixel.iter().all(|value| value.is_finite()).then_some(pixel)
}

fn project_bounds(
    model: [f32; 16],
    bounds: [f32; 4],
    projection: [f32; 16],
    size: [u32; 2],
) -> Option<[f32; 4]> {
    let mut output = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for corner in [
        [bounds[0], bounds[1]],
        [bounds[2], bounds[1]],
        [bounds[0], bounds[3]],
        [bounds[2], bounds[3]],
    ] {
        let p = project(point(model, corner), projection, size)?;
        output[0] = output[0].min(p[0]);
        output[1] = output[1].min(p[1]);
        output[2] = output[2].max(p[0]);
        output[3] = output[3].max(p[1]);
    }
    Some(output)
}

fn shifted_model(
    mut model: [f32; 16],
    offset: [f32; 2],
    projection: [f32; 16],
    size: [u32; 2],
) -> Option<[f32; 16]> {
    let origin = project(point(model, [0.0; 2]), projection, size)?;
    let u = project(point(model, [1.0, 0.0]), projection, size)?;
    let v = project(point(model, [0.0, 1.0]), projection, size)?;
    let a = [u[0] - origin[0], u[1] - origin[1]];
    let b = [v[0] - origin[0], v[1] - origin[1]];
    let determinant = a[0] * b[1] - a[1] * b[0];
    if determinant.abs() < f32::EPSILON {
        return None;
    }
    let local = [
        (offset[0] * b[1] - offset[1] * b[0]) / determinant,
        (a[0] * offset[1] - a[1] * offset[0]) / determinant,
    ];
    for axis in 0..3 {
        model[12 + axis] += model[axis] * local[0] + model[4 + axis] * local[1];
    }
    model.iter().all(|value| value.is_finite()).then_some(model)
}

fn connector_model(
    mut anchor: [f32; 16],
    panel: [f32; 16],
    endpoint: [f32; 2],
    width: f32,
) -> [f32; 16] {
    let end = point(panel, endpoint);
    let delta: [f32; 3] = std::array::from_fn(|axis| end[axis] - anchor[12 + axis]);
    let length = delta[0].hypot(delta[1]).hypot(delta[2]);
    let scale = anchor[4].hypot(anchor[5]).hypot(anchor[6]);
    let u: [f32; 3] = std::array::from_fn(|axis| anchor[axis]);
    let v: [f32; 3] = std::array::from_fn(|axis| anchor[4 + axis]);
    let normal = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let n = normal[0].hypot(normal[1]).hypot(normal[2]);
    if length > 0.0 && n > 0.0 {
        let perpendicular = [
            normal[1] * delta[2] - normal[2] * delta[1],
            normal[2] * delta[0] - normal[0] * delta[2],
            normal[0] * delta[1] - normal[1] * delta[0],
        ];
        for axis in 0..3 {
            anchor[axis] = delta[axis];
            anchor[4 + axis] = perpendicular[axis] / (n * length) * width * scale;
        }
    } else {
        anchor[..3].fill(0.0);
        anchor[4..7].fill(0.0);
    }
    anchor
}

#[cfg(test)]
#[path = "plot_label_layout_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "plot_profile_tests.rs"]
mod profile_tests;
