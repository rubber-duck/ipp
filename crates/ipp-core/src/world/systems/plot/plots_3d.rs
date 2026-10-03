//! Data-to-local-geometry preparation for spatial Plot components.
//!
//! Bars use physical cell widths and a zero baseline. Surfaces connect X/Z sites:
//! complete rectilinear grids use two faces per cell; other sites use deterministic
//! Delaunay triangulation. Invalid heights remain sites but remove incident faces,
//! preserving explicit holes. Duplicate X/Z sites are ambiguous and rejected.
//! Points remain disconnected. Pies normalize positive shares across visible
//! series; radius and height are independent physical values, never axis-scaled.
//! All meshes are chunked at the ordinary u16 address limit without sample reduction.

use super::*;
use crate::ErrorReason;
use crate::components::rows::Rows;
use crate::services::asset_management::drawing::FillRule;
use crate::services::asset_management::quadratic::{QuadraticContour, QuadraticSegment};
use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::TAU;
use std::sync::Arc;

struct Sample {
    row: PlotRowIdentity,
    point: [f64; 3],
    color: [f32; 4],
    valid: bool,
}

struct Columns<'a> {
    x: PlotPreparedColumn<'a>,
    y: PlotPreparedColumn<'a>,
    z: Option<PlotPreparedColumn<'a>>,
    color: Option<PlotPreparedColumn<'a>>,
}

impl<'a> Columns<'a> {
    fn new(
        input: &'a PlotPreparedInput<'_>,
        series: &PlotSeriesRow,
        bars: bool,
    ) -> Result<Self, ErrorReason> {
        Ok(Self {
            x: input.bind(&series.x)?,
            y: input.bind(if bars {
                &series.value
            } else {
                &series.y
            })?,
            z: (!series.z.is_empty())
                .then(|| input.bind(&series.z))
                .transpose()?,
            color: (!series.color_column.is_empty())
                .then(|| input.bind(&series.color_column))
                .transpose()?,
        })
    }

    fn samples(
        &self,
        input: &PlotPreparedInput<'_>,
        slot: u32,
        series: &PlotSeriesRow,
        preserve_holes: bool,
    ) -> Result<Vec<Sample>, ErrorReason> {
        let mut samples = Vec::new();
        for (index, &row_id) in input.row_ids.iter().enumerate() {
            let x = self.x.number(index)?;
            let y = self.y.number(index)?;
            let z = self
                .z
                .map_or(Ok(Some(0.0)), |column| column.number(index))?;
            let color = self
                .color
                .map_or(Ok(Some(series.color)), |column| column.color(index))?;
            if let (Some(x), Some(z)) = (x, z) {
                let valid = y.is_some() && color.is_some();
                if valid || preserve_holes {
                    samples.push(Sample {
                        row: PlotRowIdentity {
                            series: slot,
                            row_id,
                        },
                        point: [x, y.unwrap_or(0.0), z],
                        color: color.unwrap_or(series.color),
                        valid,
                    });
                }
            }
        }
        Ok(samples)
    }
}

fn collect(
    series: &Rows<PlotSeriesRow>,
    input: &PlotPreparedInput<'_>,
    bars: bool,
    holes: bool,
) -> Result<Vec<Vec<Sample>>, ErrorReason> {
    series
        .iter()
        .filter(|(_, row)| row.visible)
        .map(|(slot, row)| Columns::new(input, row, bars)?.samples(input, slot, row, holes))
        .collect()
}

fn mapping(
    frame: &PlotFrame3d,
    samples: &[Vec<Sample>],
    baseline: bool,
) -> Result<(PlotFrameMapping3d, [f64; 3], [f64; 3]), ErrorReason> {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for sample in samples.iter().flatten().filter(|sample| sample.valid) {
        for axis in 0..3 {
            min[axis] = min[axis].min(sample.point[axis]);
            max[axis] = max[axis].max(sample.point[axis]);
        }
    }
    if baseline {
        min[1] = min[1].min(0.0);
        max[1] = max[1].max(0.0);
    }
    let automatic = [frame.automatic_x, frame.automatic_y, frame.automatic_z];
    for axis in 0..3 {
        if !automatic[axis] || !min[axis].is_finite() || !max[axis].is_finite() {
            min[axis] = f64::from(frame.min()[axis]);
            max[axis] = f64::from(frame.max()[axis]);
        } else if min[axis] == max[axis] {
            let pad = min[axis].abs().max(1.0) * 0.5;
            min[axis] -= pad;
            max[axis] += pad;
        }
    }
    Ok((PlotFrameMapping3d::new(frame, min, max)?, min, max))
}

/// Prepare grid-positioned bars, including a single depth row.
pub fn prepare_bars(
    chart: &PlotGridBars3d,
    frame: &PlotFrame3d,
    input: &PlotPreparedInput<'_>,
) -> Result<PlotPreparedGeometry, ErrorReason> {
    let samples = collect(&chart.series, input, true, false)?;
    let (mapping, min, max) = mapping(frame, &samples, true)?;
    let labels = Labels::new(&chart.labels)?;
    let mut output = PlotPreparedGeometry::default();
    let mut meshes = MeshBuilder::default();
    for sample in samples.iter().flatten() {
        let point = checked_point(mapping.map(sample.point))?;
        let baseline = checked_point(mapping.map([sample.point[0], 0.0, sample.point[2]]))?[1];
        let low = [
            point[0] - chart.bar_width * 0.5,
            baseline.min(point[1]),
            point[2] - chart.bar_depth * 0.5,
        ];
        let high = [
            point[0] + chart.bar_width * 0.5,
            baseline.max(point[1]),
            point[2] + chart.bar_depth * 0.5,
        ];
        meshes.cuboid(low, high, sample.color)?;
        let highlighted = labels.highlighted(sample.row);
        meshes.outline(
            low,
            high,
            frame.line_width
                * if highlighted {
                    2.4
                } else {
                    1.0
                },
            if highlighted {
                [0.2, 1.0, 1.0, 1.0]
            } else {
                [0.0, 0.7, 0.85, 1.0]
            },
        )?;
        output.hits.push(PlotHit {
            row: sample.row,
            shape: PlotHitShape::Box {
                min: low,
                max: high,
            },
        });
        annotate(
            &mut output,
            &mut meshes,
            &labels,
            PlotMark {
                row: sample.row,
                point,
                color: sample.color,
                radial: None,
            },
            frame,
            0.0,
        )?;
    }
    axes(&mut output, frame, min, max)?;
    finish(output, meshes)
}

/// Prepare a connected height surface; never use this for disconnected point samples.
pub fn prepare_surface(
    chart: &PlotHeightSurface3d,
    frame: &PlotFrame3d,
    input: &PlotPreparedInput<'_>,
) -> Result<PlotPreparedGeometry, ErrorReason> {
    let samples = collect(&chart.series, input, false, true)?;
    let (mapping, min, max) = mapping(frame, &samples, false)?;
    let labels = Labels::new(&chart.labels)?;
    let mut output = PlotPreparedGeometry::default();
    let mut meshes = MeshBuilder::default();
    for series in &samples {
        let points = series
            .iter()
            .map(|sample| checked_point(mapping.map(sample.point)))
            .collect::<Result<Vec<_>, _>>()?;
        let triangles = triangulate(series)?;
        let mut edges = BTreeSet::new();
        for [a, b, c] in triangles {
            if !(series[a].valid && series[b].valid && series[c].valid) {
                continue;
            }
            // Positive X/Z orientation faces downward in the right-handed 3D frame.
            meshes.triangle(
                [points[a], points[c], points[b]],
                [series[a].color, series[c].color, series[b].color],
                true,
            )?;
            if chart.wireframe {
                for [a, b] in [[a, b], [b, c], [c, a]] {
                    edges.insert((a.min(b), a.max(b)));
                }
            }
        }
        for (a, b) in edges {
            meshes.line(
                points[a],
                points[b],
                chart.line_width,
                [0.0, 0.55, 0.7, 1.0],
            )?;
        }
        for (sample, &point) in series
            .iter()
            .zip(&points)
            .filter(|(sample, _)| sample.valid)
        {
            output.hits.push(PlotHit {
                row: sample.row,
                shape: PlotHitShape::Sphere {
                    center: point,
                    radius: frame.font_size * 0.4,
                },
            });
            annotate(
                &mut output,
                &mut meshes,
                &labels,
                PlotMark {
                    row: sample.row,
                    point,
                    color: sample.color,
                    radial: None,
                },
                frame,
                frame.font_size,
            )?;
        }
    }
    axes(&mut output, frame, min, max)?;
    finish(output, meshes)
}

/// Prepare disconnected spatial marks and their authored labels.
pub fn prepare_points(
    chart: &PlotPoints3d,
    frame: &PlotFrame3d,
    input: &PlotPreparedInput<'_>,
) -> Result<PlotPreparedGeometry, ErrorReason> {
    let samples = collect(&chart.series, input, false, false)?;
    let (mapping, min, max) = mapping(frame, &samples, false)?;
    let labels = Labels::new(&chart.labels)?;
    let mut output = PlotPreparedGeometry::default();
    let mut meshes = MeshBuilder::default();
    for sample in samples.iter().flatten() {
        let point = checked_point(mapping.map(sample.point))?;
        let radius = chart.marker_size * 0.5;
        if chart.marker_shape == 0 {
            let low = point.map(|v| v - radius);
            let high = point.map(|v| v + radius);
            meshes.cuboid(low, high, sample.color)?;
            meshes.outline(low, high, frame.line_width * 0.5, [0.0, 0.7, 0.85, 1.0])?;
        } else {
            meshes.octahedron(point, radius, sample.color)?;
        }
        output.hits.push(PlotHit {
            row: sample.row,
            shape: if chart.marker_shape == 0 {
                PlotHitShape::Box {
                    min: point.map(|v| v - radius),
                    max: point.map(|v| v + radius),
                }
            } else {
                PlotHitShape::Sphere {
                    center: point,
                    radius,
                }
            },
        });
        annotate(
            &mut output,
            &mut meshes,
            &labels,
            PlotMark {
                row: sample.row,
                point,
                color: sample.color,
                radial: None,
            },
            frame,
            chart.marker_size,
        )?;
    }
    axes(&mut output, frame, min, max)?;
    finish(output, meshes)
}

struct Slice {
    row: PlotRowIdentity,
    share: f64,
    radius: f32,
    height: f32,
    color: [f32; 4],
}

/// Prepare sectors with independent share/radius/height/colour inputs.
pub fn prepare_pie(
    chart: &PlotPie3d,
    frame: &PlotFrame3d,
    input: &PlotPreparedInput<'_>,
) -> Result<PlotPreparedGeometry, ErrorReason> {
    let mut slices = Vec::new();
    for (slot, series) in chart.series.iter().filter(|(_, series)| series.visible) {
        let value = input.bind(&series.value)?;
        let radius = (!series.radius.is_empty())
            .then(|| input.bind(&series.radius))
            .transpose()?;
        let height = (!series.height.is_empty())
            .then(|| input.bind(&series.height))
            .transpose()?;
        let color = (!series.color_column.is_empty())
            .then(|| input.bind(&series.color_column))
            .transpose()?;
        for (index, &row_id) in input.row_ids.iter().enumerate() {
            let share = value.number(index)?;
            let radius = radius.map_or(Ok(Some(f64::from(chart.radius))), |column| {
                column.number(index)
            })?;
            let height = height.map_or(Ok(Some(f64::from(chart.height))), |column| {
                column.number(index)
            })?;
            let color = color.map_or(Ok(Some(series.color)), |column| column.color(index))?;
            if let (Some(share), Some(radius), Some(height), Some(color)) =
                (share, radius, height, color)
                && share > 0.0
                && radius > 0.0
                && height > 0.0
            {
                slices.push(Slice {
                    row: PlotRowIdentity {
                        series: slot,
                        row_id,
                    },
                    share,
                    radius: radius as f32,
                    height: height as f32,
                    color,
                });
            }
        }
    }
    // Scale before summation to avoid overflow without changing share ratios.
    let largest = slices.iter().map(|slice| slice.share).fold(0.0, f64::max);
    let total: f64 = slices.iter().map(|slice| slice.share / largest).sum();
    let labels = Labels::new(&chart.labels)?;
    let mut output = PlotPreparedGeometry::default();
    let mut meshes = MeshBuilder::default();
    let center = [frame.width * 0.5, 0.0, frame.depth * 0.5];
    let mut start = f64::from(chart.start_angle).rem_euclid(TAU);
    for slice in slices {
        let sweep = TAU * (slice.share / largest) / total;
        let steps = (sweep / (TAU / 96.0)).ceil().max(1.0) as usize;
        let top = [center[0], slice.height, center[2]];
        let highlighted = labels.highlighted(slice.row);
        let edge_color = if highlighted {
            [0.0, 0.95, 1.0, 1.0]
        } else {
            [
                slice.color[0].max(0.1),
                slice.color[1].max(0.3),
                slice.color[2].max(0.4),
                slice.color[3],
            ]
        };
        let edge_width = frame.line_width
            * if highlighted {
                1.7
            } else {
                0.7
            };
        for step in 0..steps {
            let a = start + sweep * step as f64 / steps as f64;
            let b = start + sweep * (step + 1) as f64 / steps as f64;
            let bottom_a = radial(center, slice.radius, a, 0.0);
            let bottom_b = radial(center, slice.radius, b, 0.0);
            let top_a = radial(center, slice.radius, a, slice.height);
            let top_b = radial(center, slice.radius, b, slice.height);
            meshes.face([top, top_a, top_b], slice.color, true)?;
            meshes.face([center, bottom_b, bottom_a], slice.color, true)?;
            meshes.quad([bottom_a, bottom_b, top_b, top_a], slice.color, true)?;
            meshes.line(top_a, top_b, edge_width, edge_color)?;
        }
        let first = radial(center, slice.radius, start, 0.0);
        let last = radial(center, slice.radius, start + sweep, 0.0);
        if sweep < TAU - 1e-12 {
            let top_first = [first[0], slice.height, first[2]];
            let top_last = [last[0], slice.height, last[2]];
            meshes.quad([center, first, top_first, top], slice.color, true)?;
            meshes.quad([center, top, top_last, last], slice.color, true)?;
            meshes.line(top, top_first, edge_width, edge_color)?;
            meshes.line(top, top_last, edge_width, edge_color)?;
            meshes.line(first, top_first, edge_width, edge_color)?;
            meshes.line(last, top_last, edge_width, edge_color)?;
        }
        let point = radial(
            center,
            slice.radius * 0.72,
            start + sweep * 0.5,
            slice.height,
        );
        output.hits.push(PlotHit {
            row: slice.row,
            shape: PlotHitShape::RadialPrism {
                center,
                radius: slice.radius,
                start: start as f32,
                sweep: sweep as f32,
                min_y: 0.0,
                max_y: slice.height,
            },
        });
        annotate(
            &mut output,
            &mut meshes,
            &labels,
            PlotMark {
                row: slice.row,
                point,
                color: slice.color,
                radial: Some((
                    top,
                    radial(center, slice.radius, start + sweep * 0.5, slice.height),
                )),
            },
            frame,
            0.0,
        )?;
        start += sweep;
    }
    // Radial layout retains only the floor grid: Cartesian scales do not affect
    // independent per-slice physical radius or height.
    floor(&mut output, frame)?;
    finish(output, meshes)
}

fn radial(center: [f32; 3], radius: f32, angle: f64, height: f32) -> [f32; 3] {
    [
        center[0] + radius * angle.sin() as f32,
        height,
        center[2] + radius * angle.cos() as f32,
    ]
}

fn checked_point(point: [f32; 3]) -> Result<[f32; 3], ErrorReason> {
    if point.iter().all(|v| v.is_finite()) {
        Ok(point)
    } else {
        Err(ErrorReason::InvalidValue)
    }
}

#[derive(Default)]
struct MeshBuilder {
    chunks: Vec<PlotMesh>,
    current: BTreeMap<u32, usize>,
}

impl MeshBuilder {
    fn triangle(
        &mut self,
        points: [[f32; 3]; 3],
        colors: [[f32; 4]; 3],
        shade: bool,
    ) -> Result<(), ErrorReason> {
        for point in points {
            checked_point(point)?;
        }
        let a: [f64; 3] =
            std::array::from_fn(|axis| f64::from(points[1][axis]) - f64::from(points[0][axis]));
        let b: [f64; 3] =
            std::array::from_fn(|axis| f64::from(points[2][axis]) - f64::from(points[0][axis]));
        let normal = [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ];
        // Unlit edge strips need only exact degeneracy rejection. Normalizing
        // their unused normal adds cost to dense cuboid preparation; f64 cross
        // products of checked f32 positions remain finite and representable.
        if normal == [0.0; 3] {
            return Ok(());
        }
        // Compact unlit vertex colours retain restrained face contrast without
        // introducing a chart shader or requiring authored lights.
        let factor = if shade {
            let length = normal[0].hypot(normal[1]).hypot(normal[2]);
            let normal = normal.map(|v| (v / length) as f32);
            // Opposite X faces share a tone distinct from both Z faces, so
            // the same cuboid remains legible from either camera hemisphere.
            0.17 + normal[1].max(0.0) * 0.75 + normal[0].abs() * 0.19 + normal[2].abs() * 0.02
        } else {
            1.0
        };
        let alpha = (colors[0][3] + colors[1][3] + colors[2][3]) / 3.0;
        let key = alpha.to_bits();
        let index = match self.current.get(&key).copied() {
            Some(index) if self.chunks[index].positions.len() <= 65532 => index,
            _ => {
                let index = self.chunks.len();
                let part = u32::try_from(index).map_err(|_| ErrorReason::Capacity)?;
                self.chunks.push(PlotMesh {
                    part,
                    color: [1.0, 1.0, 1.0, alpha],
                    ..PlotMesh::default()
                });
                self.current.insert(key, index);
                index
            }
        };
        let mesh = &mut self.chunks[index];
        let base = mesh.positions.len() as u16;
        mesh.positions.extend(points);
        mesh.colors
            .extend(colors.map(|color| [color[0] * factor, color[1] * factor, color[2] * factor]));
        mesh.indices.extend([base, base + 1, base + 2]);
        Ok(())
    }

    fn face(
        &mut self,
        points: [[f32; 3]; 3],
        color: [f32; 4],
        shade: bool,
    ) -> Result<(), ErrorReason> {
        self.triangle(points, [color; 3], shade)
    }

    fn quad(
        &mut self,
        points: [[f32; 3]; 4],
        color: [f32; 4],
        shade: bool,
    ) -> Result<(), ErrorReason> {
        self.face([points[0], points[1], points[2]], color, shade)?;
        self.face([points[0], points[2], points[3]], color, shade)
    }

    fn outline(
        &mut self,
        low: [f32; 3],
        high: [f32; 3],
        width: f32,
        color: [f32; 4],
    ) -> Result<(), ErrorReason> {
        let corners: [[f32; 3]; 8] = std::array::from_fn(|i| {
            std::array::from_fn(|axis| {
                if i & (1 << axis) == 0 {
                    low[axis]
                } else {
                    high[axis]
                }
            })
        });
        for i in 0..8 {
            for axis in 0..3 {
                let j = i ^ (1 << axis);
                if i < j {
                    self.line(corners[i], corners[j], width, color)?;
                }
            }
        }
        Ok(())
    }

    fn cuboid(
        &mut self,
        low: [f32; 3],
        high: [f32; 3],
        color: [f32; 4],
    ) -> Result<(), ErrorReason> {
        let p: [[f32; 3]; 8] = std::array::from_fn(|i| {
            std::array::from_fn(|axis| {
                if i & (1 << axis) == 0 {
                    low[axis]
                } else {
                    high[axis]
                }
            })
        });
        for [a, b, c, d] in [
            [0, 4, 6, 2],
            [1, 3, 7, 5],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 2, 3, 1],
            [4, 5, 7, 6],
        ] {
            self.quad([p[a], p[b], p[c], p[d]], color, true)?;
        }
        Ok(())
    }

    fn octahedron(
        &mut self,
        center: [f32; 3],
        radius: f32,
        color: [f32; 4],
    ) -> Result<(), ErrorReason> {
        let p: [[f32; 3]; 6] = std::array::from_fn(|i| {
            let mut p = center;
            p[i / 2] += if i % 2 == 0 {
                radius
            } else {
                -radius
            };
            p
        });
        for a in 0..2 {
            for b in 2..4 {
                for c in 4..6 {
                    let mut face = [p[a], p[b], p[c]];
                    if dot(
                        cross(sub(face[1], face[0]), sub(face[2], face[0])),
                        sub(face[0], center),
                    ) < 0.0
                    {
                        face.swap(1, 2);
                    }
                    self.face(face, color, true)?;
                }
            }
        }
        Ok(())
    }

    fn line(
        &mut self,
        a: [f32; 3],
        b: [f32; 3],
        width: f32,
        color: [f32; 4],
    ) -> Result<(), ErrorReason> {
        let direction = sub(b, a);
        let length = dot(direction, direction).sqrt();
        if length == 0.0 {
            return Ok(());
        }
        let direction = direction.map(|v| v / length);
        let reference = if direction[1].abs() < 0.9 {
            [0.0, 1.0, 0.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        let u = cross(direction, reference);
        let ulength = dot(u, u).sqrt();
        let u = u.map(|v| v * width * 0.5 / ulength);
        let v = cross(direction, u);
        let corners: [[f32; 3]; 8] = std::array::from_fn(|i| {
            let center = if i < 4 {
                a
            } else {
                b
            };
            std::array::from_fn(|axis| {
                center[axis]
                    + if i & 1 == 0 {
                        u[axis]
                    } else {
                        -u[axis]
                    }
                    + if i & 2 == 0 {
                        v[axis]
                    } else {
                        -v[axis]
                    }
            })
        });
        for [a, b, c, d] in [
            [0, 1, 3, 2],
            [4, 6, 7, 5],
            [0, 4, 5, 1],
            [1, 5, 7, 3],
            [3, 7, 6, 2],
            [2, 6, 4, 0],
        ] {
            self.quad(
                [corners[a], corners[d], corners[c], corners[b]],
                color,
                false,
            )?;
        }
        Ok(())
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

fn finish(
    mut output: PlotPreparedGeometry,
    meshes: MeshBuilder,
) -> Result<PlotPreparedGeometry, ErrorReason> {
    output.meshes = meshes.chunks;
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for point in output.meshes.iter().flat_map(|mesh| &mesh.positions) {
        for axis in 0..3 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
        }
    }
    // Plane clipping bounds participate so text/grid cannot be culled at mesh bounds.
    for plane in &output.planes {
        let mut radius = 0.0_f32;
        for x in [plane.clip[0], plane.clip[2]] {
            for y in [plane.clip[1], plane.clip[3]] {
                let point = std::array::from_fn::<_, 3, _>(|axis| {
                    plane.model[12 + axis] + plane.model[axis] * x + plane.model[4 + axis] * y
                });
                checked_point(point)?;
                if plane.facing == PlotPlaneFacing::Camera {
                    let offset = sub(point, [plane.model[12], plane.model[13], plane.model[14]]);
                    radius = radius.max(offset[0].hypot(offset[1]).hypot(offset[2]));
                }
                for axis in 0..3 {
                    min[axis] = min[axis].min(point[axis]);
                    max[axis] = max[axis].max(point[axis]);
                }
            }
        }
        // An annotation may rotate without a new binding/geometry preparation.
        // Its local enclosure therefore covers every orientation about the anchor.
        if plane.facing == PlotPlaneFacing::Camera {
            for axis in 0..3 {
                min[axis] = min[axis].min(plane.model[12 + axis] - radius);
                max[axis] = max[axis].max(plane.model[12 + axis] + radius);
            }
        }
    }
    if min[0].is_finite() {
        output.bounds = Some([min, max]);
    }
    Ok(output)
}

fn stroke(part: u32, a: [f32; 2], b: [f32; 2], width: f32, color: [f32; 4]) -> PlotPrimitive {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let length = dx.hypot(dy).max(f32::MIN_POSITIVE);
    let n = [-dy * width * 0.5 / length, dx * width * 0.5 / length];
    let points = [
        [a[0] + n[0], a[1] + n[1]],
        [b[0] + n[0], b[1] + n[1]],
        [b[0] - n[0], b[1] - n[1]],
        [a[0] - n[0], a[1] - n[1]],
    ];
    polygon(part, &points, color)
}

fn polygon(part: u32, points: &[[f32; 2]], color: [f32; 4]) -> PlotPrimitive {
    let bounds = [
        points.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min),
        points.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min),
        points
            .iter()
            .map(|p| p[0])
            .fold(f32::NEG_INFINITY, f32::max),
        points
            .iter()
            .map(|p| p[1])
            .fold(f32::NEG_INFINITY, f32::max),
    ];
    PlotPrimitive::path(
        part,
        PlotPath {
            bounds,
            contours: Arc::from([QuadraticContour {
                start: points[0],
                segments: points[1..]
                    .iter()
                    .map(|&to| QuadraticSegment::Line {
                        to,
                    })
                    .collect(),
            }]),
            fill_rule: FillRule::NonZero,
        },
        color,
    )
}

fn plane_model(origin: [f32; 3], u: [f32; 3], v: [f32; 3]) -> [f32; 16] {
    let normal = cross(u, v);
    [
        u[0], u[1], u[2], 0.0, v[0], v[1], v[2], 0.0, normal[0], normal[1], normal[2], 0.0,
        origin[0], origin[1], origin[2], 1.0,
    ]
}

/// A new immutable geometry result owns compact deterministic plane identities.
/// Public sparse label slots never enter this renderer/cache namespace.
fn next_plane_part(output: &PlotPreparedGeometry) -> Result<u32, ErrorReason> {
    u32::try_from(output.planes.len()).map_err(|_| ErrorReason::Capacity)
}

fn floor(output: &mut PlotPreparedGeometry, frame: &PlotFrame3d) -> Result<(), ErrorReason> {
    let mut primitives = Vec::new();
    for tick in 0..=frame.ticks {
        let t = tick as f32 / frame.ticks as f32;
        let x = frame.width * t;
        let z = frame.depth * t;
        primitives.push(stroke(
            primitives.len() as u32,
            [x, 0.0],
            [x, frame.depth],
            frame.line_width,
            frame.grid_color(),
        ));
        primitives.push(stroke(
            primitives.len() as u32,
            [0.0, z],
            [frame.width, z],
            frame.line_width,
            frame.grid_color(),
        ));
    }
    output.planes.push(PlotPlane {
        part: next_plane_part(output)?,
        facing: PlotPlaneFacing::Fixed,
        layout: PlotPlaneLayout::None,
        placement: PlotPlanePlacement::Fixed,
        model: plane_model(
            [0.0, -frame.line_width, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
        ),
        clip: [
            -frame.font_size,
            -frame.font_size,
            frame.width + frame.font_size,
            frame.depth + frame.font_size,
        ],
        primitives,
    });
    Ok(())
}

fn axes(
    output: &mut PlotPreparedGeometry,
    frame: &PlotFrame3d,
    min: [f64; 3],
    max: [f64; 3],
) -> Result<(), ErrorReason> {
    let extent = [frame.width, frame.height, frame.depth];
    let font = frame.font_size;
    let color = frame.color();
    // Three retained enclosure grids; only their station changes with the view.
    for normal in 0..3u8 {
        let (origin, u, v, width, height) = match normal {
            0 => (
                [0.0, frame.height, 0.0],
                [0.0, 0.0, 1.0],
                [0.0, -1.0, 0.0],
                frame.depth,
                frame.height,
            ),
            1 => (
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                frame.width,
                frame.depth,
            ),
            _ => (
                [0.0, frame.height, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, -1.0, 0.0],
                frame.width,
                frame.height,
            ),
        };
        let mut primitives = Vec::new();
        for tick in 0..=frame.ticks {
            let t = tick as f32 / frame.ticks as f32;
            primitives.push(stroke(
                primitives.len() as u32,
                [width * t, 0.0],
                [width * t, height],
                frame.line_width,
                frame.grid_color(),
            ));
            primitives.push(stroke(
                primitives.len() as u32,
                [0.0, height * t],
                [width, height * t],
                frame.line_width,
                frame.grid_color(),
            ));
        }
        output.planes.push(PlotPlane {
            part: next_plane_part(output)?,
            model: plane_model(origin, u, v),
            facing: PlotPlaneFacing::Fixed,
            layout: PlotPlaneLayout::None,
            placement: PlotPlanePlacement::Grid {
                extent,
                normal,
            },
            clip: [-font, -font, width + font, height + font],
            primitives,
        });
    }
    // Separate positive-axis paint avoids reflecting arrows or numeric coordinates.
    for axis in 0..3u8 {
        let length = extent[axis as usize];
        let u = std::array::from_fn(|i| {
            if i == axis as usize {
                1.0
            } else {
                0.0
            }
        });
        let v = if axis == 1 {
            [-1.0, 0.0, 0.0]
        } else {
            [0.0, -1.0, 0.0]
        };
        let mut primitives = vec![
            stroke(0, [0.0, 0.0], [length + font, 0.0], frame.line_width, color),
            polygon(
                1,
                &[
                    [length + font, 0.0],
                    [length + font * 0.5, -font * 0.2],
                    [length + font * 0.5, font * 0.2],
                ],
                color,
            ),
        ];
        for tick in 0..=frame.ticks {
            let t = tick as f32 / frame.ticks as f32;
            primitives.push(stroke(
                primitives.len() as u32,
                [length * t, -font * 0.1],
                [length * t, font * 0.1],
                frame.line_width,
                color,
            ));
            let anchor = std::array::from_fn(|i| {
                if i == axis as usize {
                    length * t
                } else {
                    0.0
                }
            });
            let offset = if axis == 1 {
                [-font * 2.5, font * 0.3]
            } else {
                [-font * 0.3, font * 0.8]
            };
            axis_text(
                output,
                anchor,
                offset,
                PlotPlaneLayout::Tick(axis),
                number(
                    min[axis as usize] + (max[axis as usize] - min[axis as usize]) * f64::from(t),
                ),
                frame,
                color,
            )?;
        }
        let title = match axis {
            0 => &frame.x_title,
            1 => &frame.y_title,
            _ => &frame.z_title,
        };
        let anchor = std::array::from_fn(|i| {
            if i == axis as usize {
                length
                    * if axis == 1 {
                        1.0
                    } else {
                        0.5
                    }
            } else {
                0.0
            }
        });
        let offset = if axis == 1 {
            [-font, -font * 1.4]
        } else {
            [-font, font * 2.5]
        };
        axis_text(
            output,
            anchor,
            offset,
            PlotPlaneLayout::Title(axis),
            title.clone(),
            frame,
            color,
        )?;
        output.planes.push(PlotPlane {
            part: next_plane_part(output)?,
            model: plane_model([0.0; 3], u, v),
            facing: PlotPlaneFacing::Fixed,
            layout: PlotPlaneLayout::None,
            placement: PlotPlanePlacement::Axis {
                extent,
                axis,
            },
            clip: [-font * 3.0, -font * 3.0, length + font * 3.0, font * 3.0],
            primitives,
        });
    }
    Ok(())
}

/// Axis labels retain their station in chart coordinates; only the small text
/// plane faces a view. Plane identities are independent of public Rows slots.
fn axis_text(
    output: &mut PlotPreparedGeometry,
    anchor: [f32; 3],
    offset: [f32; 2],
    layout: PlotPlaneLayout,
    text: impl Into<Arc<str>>,
    frame: &PlotFrame3d,
    color: [f32; 4],
) -> Result<(), ErrorReason> {
    let font = frame.font_size;
    let text = text.into();
    let width = (text.chars().count() as f32 + 2.0) * font;
    output.planes.push(PlotPlane {
        part: next_plane_part(output)?,
        model: plane_model(anchor, [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
        facing: PlotPlaneFacing::Camera,
        layout,
        placement: PlotPlanePlacement::Axis {
            extent: [frame.width, frame.height, frame.depth],
            axis: match layout {
                PlotPlaneLayout::Tick(axis) | PlotPlaneLayout::Title(axis) => axis,
                _ => unreachable!(),
            },
        },
        clip: [
            offset[0] - font,
            offset[1] - font * 2.0,
            offset[0] + width,
            offset[1] + font * 3.0,
        ],
        primitives: vec![PlotPrimitive::text(0, offset, text, font, color)],
    });
    Ok(())
}

fn number(value: f64) -> String {
    if value.abs() >= 1e6 || (value != 0.0 && value.abs() < 0.001) {
        format!("{value:.2e}")
    } else {
        let text = format!("{value:.2}");
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

struct Labels<'a>(BTreeMap<(u32, u64), Vec<(u32, &'a PlotLabelRow)>>);

impl<'a> Labels<'a> {
    fn highlighted(&self, row: PlotRowIdentity) -> bool {
        self.0
            .get(&(row.series, row.row_id.0))
            .is_some_and(|labels| labels.iter().any(|(_, label)| label.highlighted))
    }

    fn new(rows: &'a Rows<PlotLabelRow>) -> Result<Self, ErrorReason> {
        let mut labels = BTreeMap::<_, Vec<_>>::new();
        for (slot, label) in rows.iter() {
            labels
                .entry((label.series, label.source_row()?.0))
                .or_default()
                .push((slot, label));
        }
        Ok(Self(labels))
    }
}

struct PlotMark {
    row: PlotRowIdentity,
    point: [f32; 3],
    color: [f32; 4],
    radial: Option<([f32; 3], [f32; 3])>,
}

fn annotate(
    output: &mut PlotPreparedGeometry,
    meshes: &mut MeshBuilder,
    labels: &Labels<'_>,
    mark: PlotMark,
    frame: &PlotFrame3d,
    mark_size: f32,
) -> Result<(), ErrorReason> {
    let PlotMark {
        row,
        point,
        color,
        radial,
    } = mark;

    for (_, label) in labels
        .0
        .get(&(row.series, row.row_id.0))
        .into_iter()
        .flatten()
    {
        if label.highlighted && mark_size > 0.0 {
            let r = mark_size * 0.65;
            let highlight = [0.0, 0.95, 1.0, 1.0];
            let corners: [[f32; 3]; 8] = std::array::from_fn(|i| {
                std::array::from_fn(|axis| {
                    point[axis]
                        + if i & (1 << axis) == 0 {
                            -r
                        } else {
                            r
                        }
                })
            });
            for i in 0..8 {
                for axis in 0..3 {
                    let j = i ^ (1 << axis);
                    if i < j {
                        meshes.line(corners[i], corners[j], frame.line_width * 1.4, highlight)?;
                    }
                }
            }
        }
        if label.text.is_empty() {
            continue;
        }
        let font = frame.font_size;
        // Pie direction is selected by projection, not a client-provided offset.
        let position = if radial.is_some() {
            [0.0, 0.0]
        } else {
            label.offset
        };
        let width = (label.text.chars().count() as f32 * font * 0.62 + font).max(font * 2.0);
        let height = font * 1.6;
        let mut primitives = Vec::new();
        let p = [position[0], position[1] - height];
        primitives.push(PlotPrimitive {
            part: 1,
            position: p,
            color: [0.002, 0.016, 0.025, 0.95],
            kind: PlotPrimitiveKind::Box {
                size: [width, height],
                border_width: frame.line_width,
                border_color: color,
            },
        });
        primitives.push(PlotPrimitive::text(
            2,
            // Text's position is a layout top, not an alphabetic baseline.
            [p[0] + font * 0.4, p[1] + font * 0.3],
            label.text.clone(),
            font,
            frame.color(),
        ));
        let panel = next_plane_part(output)?;
        output.planes.push(PlotPlane {
            part: panel,
            facing: PlotPlaneFacing::Camera,
            layout: PlotPlaneLayout::Callout,
            placement: radial.map_or(PlotPlanePlacement::Fixed, |(center, rim)| {
                PlotPlanePlacement::Radial {
                    center,
                    rim,
                    spacing: label.offset[0].hypot(label.offset[1]),
                }
            }),
            model: plane_model(point, [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
            clip: [
                p[0].min(0.0) - font,
                p[1].min(0.0) - font,
                (p[0] + width).max(0.0) + font,
                (p[1] + height).max(0.0) + font,
            ],
            primitives,
        });
        if label.connector {
            output.planes.push(PlotPlane {
                part: next_plane_part(output)?,
                facing: PlotPlaneFacing::Camera,
                layout: PlotPlaneLayout::Connector {
                    panel,
                    endpoint: position,
                    width: frame.line_width,
                },
                placement: PlotPlanePlacement::Fixed,
                model: plane_model(point, [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
                clip: [0.0, -0.5, 1.0, 0.5],
                // A retained unit strip: view placement alone stretches it to the
                // moved panel; no camera-dependent path generation or upload.
                primitives: vec![polygon(
                    0,
                    &[[0.0, -0.5], [1.0, -0.5], [1.0, 0.5], [0.0, 0.5]],
                    color,
                )],
            });
        }
    }
    Ok(())
}

fn coordinate_cmp(a: &f64, b: &f64) -> std::cmp::Ordering {
    if a == b {
        std::cmp::Ordering::Equal
    } else {
        a.total_cmp(b)
    }
}

fn triangulate(samples: &[Sample]) -> Result<Vec<[usize; 3]>, ErrorReason> {
    let mut order: Vec<_> = (0..samples.len()).collect();
    order.sort_by(|&a, &b| {
        coordinate_cmp(&samples[a].point[0], &samples[b].point[0])
            .then_with(|| coordinate_cmp(&samples[a].point[2], &samples[b].point[2]))
    });
    if order.windows(2).any(|pair| {
        samples[pair[0]].point[0] == samples[pair[1]].point[0]
            && samples[pair[0]].point[2] == samples[pair[1]].point[2]
    }) {
        return Err(ErrorReason::InvalidValue);
    }
    if samples.len() < 3 {
        return Ok(Vec::new());
    }
    let mut xs: Vec<_> = samples.iter().map(|s| s.point[0]).collect();
    let mut zs: Vec<_> = samples.iter().map(|s| s.point[2]).collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup();
    zs.sort_by(f64::total_cmp);
    zs.dedup();
    if xs.len().checked_mul(zs.len()) == Some(samples.len()) {
        let mut triangles = Vec::new();
        for x in 0..xs.len().saturating_sub(1) {
            for z in 0..zs.len().saturating_sub(1) {
                let a = order[x * zs.len() + z];
                let b = order[(x + 1) * zs.len() + z];
                let c = order[(x + 1) * zs.len() + z + 1];
                let d = order[x * zs.len() + z + 1];
                triangles.extend([[a, b, c], [a, c, d]]);
            }
        }
        return Ok(triangles);
    }
    let min = [xs[0], zs[0]];
    let span = (xs[xs.len() - 1] - min[0]).max(zs[zs.len() - 1] - min[1]);
    if span == 0.0 {
        return Ok(Vec::new());
    }
    let mut points: Vec<[f64; 2]> = order
        .iter()
        .map(|&i| {
            [
                (samples[i].point[0] - min[0]) / span,
                (samples[i].point[2] - min[1]) / span,
            ]
        })
        .collect();
    let count = points.len();
    points.extend([[-16.0, -8.0], [16.0, -8.0], [0.0, 16.0]]);
    let mut triangles = vec![[count, count + 1, count + 2]];
    for next in 0..count {
        let mut boundary = BTreeMap::<(usize, usize), (usize, usize, usize)>::new();
        triangles.retain(|&[a, b, c]| {
            if in_circle(points[a], points[b], points[c], points[next]) {
                for (a, b) in [(a, b), (b, c), (c, a)] {
                    let edge = boundary.entry((a.min(b), a.max(b))).or_insert((a, b, 0));
                    edge.2 += 1;
                }
                false
            } else {
                true
            }
        });
        for (_, (a, b, occurrences)) in boundary {
            if occurrences != 1 {
                continue;
            }
            let area = orientation(points[a], points[b], points[next]);
            if area > 0.0 {
                triangles.push([a, b, next]);
            } else if area < 0.0 {
                triangles.push([b, a, next]);
            }
        }
    }
    Ok(triangles
        .into_iter()
        .filter(|triangle| triangle.iter().all(|&i| i < count))
        .map(|triangle| triangle.map(|i| order[i]))
        .collect())
}

fn orientation(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn in_circle(a: [f64; 2], b: [f64; 2], c: [f64; 2], p: [f64; 2]) -> bool {
    let a = [a[0] - p[0], a[1] - p[1]];
    let b = [b[0] - p[0], b[1] - p[1]];
    let c = [c[0] - p[0], c[1] - p[1]];
    let determinant = (a[0] * a[0] + a[1] * a[1]) * (b[0] * c[1] - b[1] * c[0])
        - (b[0] * b[0] + b[1] * b[1]) * (a[0] * c[1] - a[1] * c[0])
        + (c[0] * c[0] + c[1] * c[1]) * (a[0] * b[1] - a[1] * b[0]);
    determinant > 0.0
}

#[cfg(test)]
#[path = "plots_3d_tests.rs"]
mod tests;
