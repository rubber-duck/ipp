//! Pure 2D chart preparation over completed binding columns.
//!
//! Samples retain source order and identity. Invalid outputs split line runs and
//! omit bars/sectors; preparation never interpolates a missing source sample.
//! Closed contours are in Canvas top-left/+Y-down coordinates. Stroke and arc
//! expansion feed the existing analytic quadratic renderer, not a new renderer.

use std::{
    collections::{BTreeMap, BTreeSet},
    f32::consts::TAU,
    sync::Arc,
};

use super::*;
use crate::{
    DynamicPropertyKind, ErrorReason,
    components::rows::Rows,
    services::asset_management::{
        drawing::FillRule,
        quadratic::{QuadraticContour, QuadraticSegment},
    },
    systems::gui::presentation::looks::{GuiSkinTokenValue, gui_skin_tokens},
};

const CURVE_ERROR: f32 = 0.2;

#[derive(Clone, Copy, Debug)]
struct Sample {
    row: PlotRowIdentity,
    point: [f64; 2],
    color: [f32; 4],
}

struct Series {
    samples: Vec<Option<Sample>>,
}

#[derive(Clone, Copy)]
struct Anchor {
    point: [f32; 2],
    radius: f32,
}

/// Straight and monotone smooth lines from the same source samples.
/// Repeated or reversing X coordinates use straight segments, preserving order.
pub fn prepare_line(
    chart: &PlotLine2d,
    frame: &PlotFrame2d,
    input: &PlotPreparedInput<'_>,
) -> Result<PlotPreparedGeometry, ErrorReason> {
    let series = samples(&chart.series, input, false)?;
    let (min, max) = ranges(frame, &series, false);
    let mapping = PlotFrameMapping2d::new(frame, min, max)?;
    let mut geometry = CartesianPaint::new(frame, mapping, min, max).prepare();
    let mut anchors = BTreeMap::new();

    for series in &series {
        for run in series.samples.split(Option::is_none) {
            let run: Vec<_> = run.iter().filter_map(|sample| *sample).collect();
            if run.is_empty() {
                continue;
            }
            let points: Vec<_> = run.iter().map(|sample| mapping.map(sample.point)).collect();
            if points.iter().flatten().any(|v| !v.is_finite()) {
                return Err(ErrorReason::InvalidValue);
            }
            let tangents = monotone_tangents(&points);
            let mut contours = Vec::new();
            let mut color = run[0].color;
            for index in 0..points.len().saturating_sub(1) {
                if run[index].color != color {
                    emit_path(&mut geometry, std::mem::take(&mut contours), color);
                    color = run[index].color;
                }
                let a = points[index];
                let b = points[index + 1];
                if chart.interpolation == 1
                    && let (Some(ma), Some(mb)) = (tangents[index], tangents[index + 1])
                    && b[0] > a[0]
                {
                    let dx = (b[0] - a[0]) / 3.0;
                    let cubic = [
                        a,
                        [a[0] + dx, a[1] + ma * dx],
                        [b[0] - dx, b[1] - mb * dx],
                        b,
                    ];
                    smooth_stroke(cubic, chart.line_width, mapping.rect, &mut contours, 0);
                } else {
                    stroke_segment(a, b, chart.line_width, mapping.rect, &mut contours);
                }
            }
            emit_path(&mut geometry, contours, color);

            for (sample, point) in run.iter().zip(points) {
                if !inside(point, mapping.rect) {
                    continue;
                }
                let radius = chart.marker_size / 2.0;
                if radius > 0.0 {
                    emit_path(&mut geometry, vec![circle(point, radius)], sample.color);
                }
                geometry.hits.push(PlotHit {
                    row: sample.row,
                    shape: PlotHitShape::Circle {
                        center: point,
                        radius: radius.max(chart.line_width / 2.0).max(3.0),
                    },
                });
                anchors.insert(
                    row_key(sample.row),
                    Anchor {
                        point,
                        radius: radius.max(chart.line_width),
                    },
                );
            }
        }
    }
    labels(&mut geometry, frame, &chart.labels, &anchors)?;
    Ok(geometry)
}

/// Numeric X is the centre of a supplied bar/bin. The minimum positive spacing
/// determines group width; visible series occupy deterministic neighbouring slots.
/// Values are used directly: this function performs no histogram aggregation.
pub fn prepare_bars(
    chart: &PlotBars2d,
    frame: &PlotFrame2d,
    input: &PlotPreparedInput<'_>,
) -> Result<PlotPreparedGeometry, ErrorReason> {
    let series = samples(&chart.series, input, false)?;
    let highlighted = highlighted_rows(&chart.labels)?;
    let spacing = bar_spacing(&series);
    let (min, max) = bar_ranges(frame, &series, spacing);
    let mapping = PlotFrameMapping2d::new(frame, min, max)?;
    let mut geometry = CartesianPaint::new(frame, mapping, min, max).prepare();
    let mut anchors = BTreeMap::new();
    let count = series.len().max(1) as f64;
    let width = spacing * f64::from(1.0 - chart.gap) / count;

    for (slot, series) in series.iter().enumerate() {
        let offset = (slot as f64 - (count - 1.0) / 2.0) * width;
        for sample in series.samples.iter().flatten() {
            let [x, y] = sample.point;
            let a = mapping.map([x + offset - width / 2.0, 0.0]);
            let b = mapping.map([x + offset + width / 2.0, y]);
            if a.iter().chain(b.iter()).any(|v| !v.is_finite()) {
                return Err(ErrorReason::InvalidValue);
            }
            let rect = [
                a[0].max(mapping.rect[0]),
                a[1].min(b[1]).max(mapping.rect[1]),
                b[0].min(mapping.rect[2]),
                a[1].max(b[1]).min(mapping.rect[3]),
            ];
            if rect[0] >= rect[2] || rect[1] >= rect[3] {
                continue;
            }
            push_box(
                &mut geometry,
                [rect[0], rect[1]],
                [rect[2] - rect[0], rect[3] - rect[1]],
                sample.color,
            );
            if highlighted.contains(&row_key(sample.row)) {
                let mut contours = Vec::new();
                let corners = [
                    [rect[0], rect[1]],
                    [rect[2], rect[1]],
                    [rect[2], rect[3]],
                    [rect[0], rect[3]],
                ];
                for index in 0..4 {
                    stroke_segment(
                        corners[index],
                        corners[(index + 1) % 4],
                        frame.line_width.max(2.0),
                        mapping.rect,
                        &mut contours,
                    );
                }
                emit_path(&mut geometry, contours, token_color("error"));
            }
            geometry.hits.push(PlotHit {
                row: sample.row,
                shape: PlotHitShape::Rect(rect),
            });
            anchors.insert(
                row_key(sample.row),
                Anchor {
                    point: [
                        (rect[0] + rect[2]) / 2.0,
                        if y >= 0.0 {
                            rect[1]
                        } else {
                            rect[3]
                        },
                    ],
                    radius: 4.0,
                },
            );
        }
    }
    labels(&mut geometry, frame, &chart.labels, &anchors)?;
    Ok(geometry)
}

/// All positive finite values of visible series share one radial total. Zero,
/// negative and invalid values produce no sector, label anchor or hit.
pub fn prepare_pie(
    chart: &PlotPie2d,
    frame: &PlotFrame2d,
    input: &PlotPreparedInput<'_>,
) -> Result<PlotPreparedGeometry, ErrorReason> {
    let series = samples(&chart.series, input, true)?;
    let highlighted = highlighted_rows(&chart.labels)?;
    let left = frame.padding_left;
    let top = frame.padding_top;
    let right = frame.width - frame.padding_right;
    let bottom = frame.height - frame.padding_bottom;
    if left < 0.0 || top < 0.0 || right <= left || bottom <= top {
        return Err(ErrorReason::InvalidValue);
    }
    let center = [(left + right) / 2.0, (top + bottom) / 2.0];
    let radius = (right - left).min(bottom - top) / 2.0;
    let inner = radius * chart.inner_radius;
    let mut geometry = PlotPreparedGeometry {
        bounds: Some([[0.0, 0.0, 0.0], [frame.width, frame.height, 0.0]]),
        ..Default::default()
    };
    let positive: Vec<_> = series
        .iter()
        .flat_map(|series| series.samples.iter().flatten())
        .filter(|sample| sample.point[1] > 0.0)
        .collect();
    let total: f64 = positive.iter().map(|sample| sample.point[1]).sum();
    let mut anchors = BTreeMap::new();
    let mut start = chart.start_angle.rem_euclid(TAU);

    for (index, sample) in positive.iter().enumerate() {
        let sweep = if index + 1 == positive.len() {
            (chart.start_angle.rem_euclid(TAU) + TAU - start).max(0.0)
        } else {
            (sample.point[1] / total * f64::from(TAU)) as f32
        };
        let contour = sector(center, inner, radius, start, sweep);
        emit_path(&mut geometry, vec![contour], sample.color);
        if highlighted.contains(&row_key(sample.row)) {
            let half_width = frame.line_width.max(2.0) / 2.0;
            let mut contours = vec![sector(
                center,
                (radius - half_width).max(0.0),
                radius + half_width,
                start,
                sweep,
            )];
            if inner > 0.0 {
                contours.push(sector(
                    center,
                    (inner - half_width).max(0.0),
                    inner + half_width,
                    start,
                    sweep,
                ));
            }
            if sweep < TAU - 1e-5 {
                for angle in [start, start + sweep] {
                    stroke_segment(
                        polar(center, inner, angle),
                        polar(center, radius, angle),
                        half_width * 2.0,
                        [0.0, 0.0, frame.width, frame.height],
                        &mut contours,
                    );
                }
            }
            emit_path(&mut geometry, contours, token_color("error"));
        }
        geometry.hits.push(PlotHit {
            row: sample.row,
            shape: PlotHitShape::Sector {
                center,
                inner_radius: inner,
                radius,
                start,
                sweep,
            },
        });
        let point = polar(center, radius * 0.82, start + sweep / 2.0);
        anchors.insert(
            row_key(sample.row),
            Anchor {
                point,
                radius: 0.0,
            },
        );
        start += sweep;
    }
    labels(&mut geometry, frame, &chart.labels, &anchors)?;
    Ok(geometry)
}

fn samples(
    rows: &Rows<PlotSeriesRow>,
    input: &PlotPreparedInput<'_>,
    radial: bool,
) -> Result<Vec<Series>, ErrorReason> {
    let mut series = Vec::new();
    for (slot, row) in rows.iter().filter(|(_, row)| row.visible) {
        let x = if radial {
            None
        } else {
            Some(numeric(input, &row.x)?)
        };
        let y = numeric(
            input,
            if radial {
                &row.value
            } else {
                &row.y
            },
        )?;
        let color = if row.color_column.is_empty() {
            None
        } else {
            let column = input.bind(&row.color_column)?;
            if !matches!(
                column.kind,
                DynamicPropertyKind::Vec3 | DynamicPropertyKind::Vec4
            ) {
                return Err(ErrorReason::InvalidValue);
            }
            Some(column)
        };
        let mut samples = Vec::with_capacity(input.row_ids.len());
        for (index, row_id) in input.row_ids.iter().enumerate() {
            let x = match x {
                Some(column) => column.number(index)?,
                None => Some(0.0),
            };
            let y = y.number(index)?;
            let color = match color {
                Some(column) => column.color(index)?,
                None => Some(row.color),
            };
            samples.push(match (x, y, color) {
                (Some(x), Some(y), Some(color)) => Some(Sample {
                    row: PlotRowIdentity {
                        series: slot,
                        row_id: *row_id,
                    },
                    point: [x, y],
                    color,
                }),
                _ => None,
            });
        }
        series.push(Series {
            samples,
        });
    }
    Ok(series)
}

fn numeric<'a>(
    input: &'a PlotPreparedInput<'_>,
    name: &str,
) -> Result<PlotPreparedColumn<'a>, ErrorReason> {
    let column = input.bind(name)?;
    if matches!(
        column.kind,
        DynamicPropertyKind::F32 | DynamicPropertyKind::I32 | DynamicPropertyKind::U32
    ) {
        Ok(column)
    } else {
        Err(ErrorReason::InvalidValue)
    }
}

/// Fit the same Cartesian bounds used by paint and picking without preparing
/// geometry. Percentage interpolation borrows the pre-step displayed input here.
pub(super) fn interpolation_ranges(
    frame: &PlotFrame2d,
    rows: &Rows<PlotSeriesRow>,
    input: &PlotPreparedInput<'_>,
    bars: bool,
) -> Result<([f64; 2], [f64; 2]), ErrorReason> {
    let series = samples(rows, input, false)?;
    Ok(if bars {
        bar_ranges(frame, &series, bar_spacing(&series))
    } else {
        ranges(frame, &series, false)
    })
}

fn ranges(frame: &PlotFrame2d, series: &[Series], baseline: bool) -> ([f64; 2], [f64; 2]) {
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for sample in series
        .iter()
        .flat_map(|series| series.samples.iter().flatten())
    {
        for axis in 0..2 {
            min[axis] = min[axis].min(sample.point[axis]);
            max[axis] = max[axis].max(sample.point[axis]);
        }
    }
    if baseline && min[1].is_finite() {
        min[1] = min[1].min(0.0);
        max[1] = max[1].max(0.0);
    }
    for (axis, automatic) in [frame.automatic_x, frame.automatic_y]
        .into_iter()
        .enumerate()
    {
        if !automatic || !min[axis].is_finite() {
            min[axis] = f64::from(frame.min()[axis]);
            max[axis] = f64::from(frame.max()[axis]);
        } else if min[axis] == max[axis] {
            let pad = min[axis].abs().max(1.0) * 0.05;
            min[axis] -= pad;
            max[axis] += pad;
        }
    }
    (min, max)
}

fn bar_ranges(frame: &PlotFrame2d, series: &[Series], spacing: f64) -> ([f64; 2], [f64; 2]) {
    let (mut min, mut max) = ranges(frame, series, true);
    if frame.automatic_x {
        min[0] -= spacing / 2.0;
        max[0] += spacing / 2.0;
    }
    (min, max)
}

fn bar_spacing(series: &[Series]) -> f64 {
    let mut x: Vec<_> = series
        .iter()
        .flat_map(|series| {
            series
                .samples
                .iter()
                .flatten()
                .map(|sample| sample.point[0])
        })
        .collect();
    x.sort_by(f64::total_cmp);
    x.dedup();
    x.windows(2)
        .map(|pair| pair[1] - pair[0])
        .filter(|distance| *distance > 0.0)
        .min_by(f64::total_cmp)
        .unwrap_or(1.0)
}

struct CartesianPaint<'a> {
    frame: &'a PlotFrame2d,
    mapping: PlotFrameMapping2d,
    min: [f64; 2],
    max: [f64; 2],
}

impl<'a> CartesianPaint<'a> {
    fn new(
        frame: &'a PlotFrame2d,
        mapping: PlotFrameMapping2d,
        min: [f64; 2],
        max: [f64; 2],
    ) -> Self {
        Self {
            frame,
            mapping,
            min,
            max,
        }
    }

    fn prepare(self) -> PlotPreparedGeometry {
        let frame = self.frame;
        let [left, top, right, bottom] = self.mapping.rect;
        let mut geometry = PlotPreparedGeometry {
            bounds: Some([[0.0, 0.0, 0.0], [frame.width, frame.height, 0.0]]),
            ..Default::default()
        };
        for tick in 0..=frame.ticks {
            let ratio = f64::from(tick) / f64::from(frame.ticks);
            let x = self.min[0] + (self.max[0] - self.min[0]) * ratio;
            let y = self.min[1] + (self.max[1] - self.min[1]) * ratio;
            let point = self.mapping.map([x, y]);
            push_box(
                &mut geometry,
                [point[0] - frame.line_width / 2.0, top],
                [frame.line_width, bottom - top],
                frame.grid_color(),
            );
            push_box(
                &mut geometry,
                [left, point[1] - frame.line_width / 2.0],
                [right - left, frame.line_width],
                frame.grid_color(),
            );
            let text = tick_text(x);
            let offset = text.chars().count() as f32 * frame.font_size * 0.3;
            push_text(
                &mut geometry,
                [point[0] - offset, bottom + 8.0],
                text,
                frame.font_size,
                frame.color(),
            );
            let text = tick_text(y);
            let offset = text.chars().count() as f32 * frame.font_size * 0.6;
            push_text(
                &mut geometry,
                [left - offset - 8.0, point[1] - frame.font_size / 2.0],
                text,
                frame.font_size,
                frame.color(),
            );
        }
        for (a, b) in [
            ([left, top], [left, bottom]),
            ([left, bottom], [right, bottom]),
        ] {
            let mut contours = Vec::new();
            stroke_segment(
                a,
                b,
                frame.line_width,
                [0.0, 0.0, frame.width, frame.height],
                &mut contours,
            );
            emit_path(&mut geometry, contours, frame.color());
        }
        if !frame.x_title.is_empty() {
            let offset = frame.x_title.chars().count() as f32 * frame.font_size * 0.3;
            push_text(
                &mut geometry,
                [
                    (left + right) / 2.0 - offset,
                    frame.height - frame.font_size - 2.0,
                ],
                frame.x_title.clone(),
                frame.font_size,
                frame.color(),
            );
        }
        if !frame.y_title.is_empty() {
            push_text(
                &mut geometry,
                [left, (top - frame.font_size - 4.0).max(0.0)],
                frame.y_title.clone(),
                frame.font_size,
                frame.color(),
            );
        }
        geometry
    }
}

fn tick_text(value: f64) -> String {
    let value = if value.abs() < 1e-10 {
        0.0
    } else {
        value
    };
    if value != 0.0 && (value.abs() >= 1e6 || value.abs() < 1e-3) {
        format!("{value:.2e}")
    } else {
        let text = format!("{value:.3}");
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

fn labels(
    geometry: &mut PlotPreparedGeometry,
    frame: &PlotFrame2d,
    labels: &Rows<PlotLabelRow>,
    anchors: &BTreeMap<(u32, u64), Anchor>,
) -> Result<(), ErrorReason> {
    // The shared magenta hue marks authored chart selection, as in the reference.
    let highlight = token_color("error");
    let page = token_color("page");
    for (_, label) in labels.iter() {
        let row = PlotRowIdentity {
            series: label.series,
            row_id: label.source_row()?,
        };
        let Some(anchor) = anchors.get(&row_key(row)) else {
            continue;
        };
        let color = if label.highlighted {
            highlight
        } else {
            frame.color()
        };
        if label.highlighted && anchor.radius > 0.0 {
            // A ring makes the selected sample clear without covering its value colour.
            emit_path(
                geometry,
                vec![sector(
                    anchor.point,
                    anchor.radius + 2.0,
                    anchor.radius + 4.0,
                    0.0,
                    TAU,
                )],
                highlight,
            );
        }
        if label.text.is_empty() {
            continue;
        }
        let width = label.text.chars().count() as f32 * frame.font_size * 0.62 + 16.0;
        let height = frame.font_size + 12.0;
        let origin = [
            anchor.point[0] + label.offset[0],
            anchor.point[1] + label.offset[1],
        ];
        if label.connector && label.offset != [0.0; 2] {
            let end = [
                origin[0]
                    + if label.offset[0] < 0.0 {
                        width
                    } else {
                        0.0
                    },
                origin[1] + height / 2.0,
            ];
            let elbow = [
                end[0]
                    + if label.offset[0] < 0.0 {
                        8.0
                    } else {
                        -8.0
                    },
                end[1],
            ];
            let mut contours = Vec::new();
            let rect = [0.0, 0.0, frame.width, frame.height];
            stroke_segment(anchor.point, elbow, frame.line_width, rect, &mut contours);
            stroke_segment(elbow, end, frame.line_width, rect, &mut contours);
            emit_path(geometry, contours, color);
        }
        // Paired 45-degree cuts repeat the native technical control language.
        let outer = cut_box(origin, [width, height], 4.0);
        emit_path(geometry, vec![outer], color);
        let inset = frame.line_width.max(1.0);
        emit_path(
            geometry,
            vec![cut_box(
                [origin[0] + inset, origin[1] + inset],
                [width - inset * 2.0, height - inset * 2.0],
                3.0,
            )],
            page,
        );
        push_text(
            geometry,
            [origin[0] + 8.0, origin[1] + 6.0],
            label.text.clone(),
            frame.font_size,
            color,
        );
    }
    Ok(())
}

fn row_key(row: PlotRowIdentity) -> (u32, u64) {
    (row.series, row.row_id.0)
}

fn highlighted_rows(labels: &Rows<PlotLabelRow>) -> Result<BTreeSet<(u32, u64)>, ErrorReason> {
    labels
        .iter()
        .filter(|(_, label)| label.highlighted)
        .map(|(_, label)| Ok((label.series, label.source_row()?.0)))
        .collect()
}

fn token_color(name: &str) -> [f32; 4] {
    gui_skin_tokens()
        .iter()
        .find_map(|token| match token.value {
            GuiSkinTokenValue::Color(color) if token.name == name => Some(color),
            _ => None,
        })
        .expect("maintained GUI palette colour")
}

fn cut_box(origin: [f32; 2], size: [f32; 2], cut: f32) -> QuadraticContour {
    let [x, y] = origin;
    let [w, h] = size;
    polygon(&[
        [x + cut, y],
        [x + w, y],
        [x + w, y + h - cut],
        [x + w - cut, y + h],
        [x, y + h],
        [x, y + cut],
    ])
}

fn push_box(
    geometry: &mut PlotPreparedGeometry,
    position: [f32; 2],
    size: [f32; 2],
    color: [f32; 4],
) {
    geometry.canvas.push(PlotPrimitive::box_fill(
        geometry.canvas.len() as u32,
        position,
        size,
        color,
    ));
}

fn push_text(
    geometry: &mut PlotPreparedGeometry,
    position: [f32; 2],
    text: impl Into<Arc<str>>,
    font_size: f32,
    color: [f32; 4],
) {
    geometry.canvas.push(PlotPrimitive::text(
        geometry.canvas.len() as u32,
        position,
        text,
        font_size,
        color,
    ));
}

fn emit_path(
    geometry: &mut PlotPreparedGeometry,
    contours: Vec<QuadraticContour>,
    color: [f32; 4],
) {
    if contours.is_empty() {
        return;
    }
    let mut bounds = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    let mut enclose = |point: [f32; 2]| {
        bounds[0] = bounds[0].min(point[0]);
        bounds[1] = bounds[1].min(point[1]);
        bounds[2] = bounds[2].max(point[0]);
        bounds[3] = bounds[3].max(point[1]);
    };
    for contour in &contours {
        enclose(contour.start);
        for segment in &contour.segments {
            match segment {
                QuadraticSegment::Line {
                    to,
                } => enclose(*to),
                QuadraticSegment::Quadratic {
                    control,
                    to,
                } => {
                    enclose(*control);
                    enclose(*to);
                }
            }
        }
    }
    let path = PlotPath {
        bounds,
        contours: contours.into(),
        fill_rule: FillRule::NonZero,
    };
    geometry.canvas.push(PlotPrimitive::path(
        geometry.canvas.len() as u32,
        path,
        color,
    ));
}

fn polygon(points: &[[f32; 2]]) -> QuadraticContour {
    QuadraticContour {
        start: points[0],
        segments: points[1..]
            .iter()
            .map(|point| QuadraticSegment::Line {
                to: *point,
            })
            .collect(),
    }
}

fn inside(point: [f32; 2], rect: [f32; 4]) -> bool {
    point[0] >= rect[0] && point[0] <= rect[2] && point[1] >= rect[1] && point[1] <= rect[3]
}

/// Liang-Barsky clipping limits fixed-bound strokes before contour expansion.
fn clip(a: [f32; 2], b: [f32; 2], rect: [f32; 4]) -> Option<[[f32; 2]; 2]> {
    let delta = [
        f64::from(b[0]) - f64::from(a[0]),
        f64::from(b[1]) - f64::from(a[1]),
    ];
    let mut lo: f64 = 0.0;
    let mut hi: f64 = 1.0;
    for axis in 0..2 {
        for (p, q) in [
            (-delta[axis], f64::from(a[axis]) - f64::from(rect[axis])),
            (delta[axis], f64::from(rect[axis + 2]) - f64::from(a[axis])),
        ] {
            if p == 0.0 {
                if q < 0.0 {
                    return None;
                }
            } else if p < 0.0 {
                lo = lo.max(q / p);
            } else {
                hi = hi.min(q / p);
            }
        }
    }
    (lo <= hi).then(|| {
        [
            std::array::from_fn(|axis| (f64::from(a[axis]) + delta[axis] * lo) as f32),
            std::array::from_fn(|axis| (f64::from(a[axis]) + delta[axis] * hi) as f32),
        ]
    })
}

fn stroke_segment(
    a: [f32; 2],
    b: [f32; 2],
    width: f32,
    rect: [f32; 4],
    contours: &mut Vec<QuadraticContour>,
) {
    let Some([a, b]) = clip(a, b, rect) else {
        return;
    };
    let normal = normal(a, b, width / 2.0);
    if normal == [0.0; 2] {
        return;
    }
    contours.push(polygon(&[
        add(a, normal),
        add(b, normal),
        sub(b, normal),
        sub(a, normal),
    ]));
    // All contours wind alike, so intersections are a union in one analytic draw.
    contours.push(circle(a, width / 2.0));
    contours.push(circle(b, width / 2.0));
}

fn normal(a: [f32; 2], b: [f32; 2], radius: f32) -> [f32; 2] {
    let delta = sub(b, a);
    let length = delta[0].hypot(delta[1]);
    if length <= f32::EPSILON {
        [0.0; 2]
    } else {
        [delta[1] / length * radius, -delta[0] / length * radius]
    }
}

fn add(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn mid(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
}

fn polar(center: [f32; 2], radius: f32, angle: f32) -> [f32; 2] {
    [
        center[0] + radius * angle.sin(),
        center[1] - radius * angle.cos(),
    ]
}

fn arc(
    center: [f32; 2],
    radius: f32,
    start: f32,
    sweep: f32,
    segments: &mut Vec<QuadraticSegment>,
) {
    // Tangent intersection quadratics, bounded in angle and radial error. The
    // midpoint radial error is r*(1-cos(h/2))^2/(2*cos(h/2)).
    let step = (8.0 * CURVE_ERROR / radius.max(CURVE_ERROR))
        .powf(0.25)
        .min(std::f32::consts::FRAC_PI_4);
    let count = (sweep.abs() / step).ceil().max(1.0) as usize;
    let delta = sweep / count as f32;
    for index in 0..count {
        let angle = start + delta * index as f32;
        segments.push(QuadraticSegment::Quadratic {
            control: polar(center, radius / (delta / 2.0).cos(), angle + delta / 2.0),
            to: polar(center, radius, angle + delta),
        });
    }
}

fn circle(center: [f32; 2], radius: f32) -> QuadraticContour {
    let mut contour = QuadraticContour {
        start: polar(center, radius, 0.0),
        segments: Vec::new(),
    };
    arc(center, radius, 0.0, TAU, &mut contour.segments);
    contour
}

fn sector(center: [f32; 2], inner: f32, radius: f32, start: f32, sweep: f32) -> QuadraticContour {
    let mut contour = QuadraticContour {
        start: polar(center, radius, start),
        segments: Vec::new(),
    };
    arc(center, radius, start, sweep, &mut contour.segments);
    if inner > 0.0 {
        contour.segments.push(QuadraticSegment::Line {
            to: polar(center, inner, start + sweep),
        });
        arc(center, inner, start + sweep, -sweep, &mut contour.segments);
    } else {
        contour.segments.push(QuadraticSegment::Line {
            to: center,
        });
    }
    contour
}

/// Fritsch-Carlson harmonic tangents suppress overshoot in each strictly
/// increasing-X run. Discontinuities in X deliberately disable nearby smoothing.
fn monotone_tangents(points: &[[f32; 2]]) -> Vec<Option<f32>> {
    if points.len() < 2 {
        return vec![None; points.len()];
    }
    let slopes: Vec<_> = points
        .windows(2)
        .map(|pair| {
            let dx = pair[1][0] - pair[0][0];
            (dx > 0.0).then(|| (pair[1][1] - pair[0][1]) / dx)
        })
        .collect();
    (0..points.len())
        .map(|index| {
            if index == 0 {
                return slopes[0];
            }
            if index + 1 == points.len() {
                return slopes[index - 1];
            }
            let (a, b) = (slopes[index - 1]?, slopes[index]?);
            if a * b <= 0.0 {
                return Some(0.0);
            }
            let previous = points[index][0] - points[index - 1][0];
            let next = points[index + 1][0] - points[index][0];
            let w1 = 2.0 * next + previous;
            let w2 = next + 2.0 * previous;
            Some((w1 + w2) / (w1 / a + w2 / b))
        })
        .collect()
}

fn split_cubic(p: [[f32; 2]; 4]) -> [[[f32; 2]; 4]; 2] {
    let a = mid(p[0], p[1]);
    let b = mid(p[1], p[2]);
    let c = mid(p[2], p[3]);
    let d = mid(a, b);
    let e = mid(b, c);
    let f = mid(d, e);
    [[p[0], a, d, f], [f, e, c, p[3]]]
}

fn chord_distance(point: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let delta = sub(b, a);
    let length = delta[0].hypot(delta[1]);
    if length <= f32::EPSILON {
        sub(point, a)[0].hypot(sub(point, a)[1])
    } else {
        ((point[0] - a[0]) * (delta[1] / length) - (point[1] - a[1]) * (delta[0] / length)).abs()
    }
}

fn smooth_stroke(
    p: [[f32; 2]; 4],
    width: f32,
    rect: [f32; 4],
    contours: &mut Vec<QuadraticContour>,
    depth: u32,
) {
    // Convex-hull culling prevents expensive subdivision far outside fixed axes.
    if (0..2).any(|axis| {
        p.iter().all(|point| point[axis] < rect[axis])
            || p.iter().all(|point| point[axis] > rect[axis + 2])
    }) {
        return;
    }

    // The control hull bounds the entire cubic's distance from its chord.
    // Subdivision preserves every source endpoint and expands, rather than
    // reduces, the curve. Line strips avoid ill-conditioned near-collinear
    // quadratic side edges in analytic coverage; round joins/caps still use
    // closed quadratic arcs through that same renderer.
    let error = chord_distance(p[1], p[0], p[3]).max(chord_distance(p[2], p[0], p[3]));
    if error <= CURVE_ERROR || depth == 16 {
        stroke_segment(p[0], p[3], width, rect, contours);
        return;
    }

    for half in split_cubic(p) {
        smooth_stroke(half, width, rect, contours, depth + 1);
    }
}

#[cfg(test)]
#[path = "plots_2d_tests.rs"]
mod tests;
