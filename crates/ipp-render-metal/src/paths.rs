use std::{
    collections::HashMap,
    rc::Rc,
    sync::{Arc, Weak},
};

use ipp_core::{
    services::asset_management::{
        AssetKey,
        formats::{
            drawing::{DrawingLayer, FillRule},
            quadratic::{QuadraticContour, QuadraticSegment},
        },
    },
    systems::plot::{PlotPath, PlotPathGeometry},
};

use crate::device::{Device, Object};

pub(super) struct Path {
    pub(super) buffer: Rc<Object>,
    pub(super) start: u32,
    pub(super) bounds: [f32; 4],
    // Nonzero contour, even-odd contour, triangle union or stroke union.
    pub(super) mode: f32,
    pub(super) radius: f32,
}

const PATH_PAGE_RECORDS: usize = 64 * 1024;

/// Append to unused ranges only. Paths and submitted Metal commands retain
/// old pages independently of the current allocation cursor.
#[derive(Default)]
struct PathPages {
    current: Option<Rc<Object>>,
    next_record: usize,
}

impl PathPages {
    fn upload(&mut self, device: &Device, data: &[[f32; 4]]) -> Option<(Rc<Object>, u32)> {
        if data.len() > PATH_PAGE_RECORDS {
            return Some((Rc::new(device.path(data)?), 0));
        }
        if self.current.is_none() || self.next_record + data.len() > PATH_PAGE_RECORDS {
            self.current = Some(Rc::new(device.path_page(PATH_PAGE_RECORDS)?));
            self.next_record = 0;
        }
        let page = self.current.as_ref()?;
        let start = self.next_record;
        if !device.write_path(page, start, data) {
            return None;
        }
        self.next_record += data.len();
        Some((page.clone(), start as u32))
    }
}

struct Cached<T> {
    path: Option<T>,
    last_frame: u64,
}

#[derive(Default)]
pub(super) struct PathCache {
    assets: HashMap<(AssetKey, u32), Cached<Path>>,
    plots: HashMap<usize, (Weak<PlotPath>, Cached<Path>)>,
    frame: u64,
    asset_pages: PathPages,
    plot_pages: PathPages,
}

impl PathCache {
    pub(super) fn next_frame(&mut self) {
        self.frame = self.frame.saturating_add(1);
        if self.frame.is_multiple_of(60) {
            self.assets
                .retain(|_, item| self.frame - item.last_frame <= 120);
            self.plots.retain(|_, (owner, item)| {
                owner.strong_count() > 0 && self.frame - item.last_frame <= 120
            });
        }
    }

    pub(super) fn glyph(
        &mut self,
        device: &Device,
        key: AssetKey,
        glyph: u32,
        contours: &[QuadraticContour],
    ) -> Option<&Path> {
        self.asset(device, (key, glyph), contours, FillRule::NonZero)
    }

    pub(super) fn drawing(
        &mut self,
        device: &Device,
        key: AssetKey,
        layer: u32,
        drawing: &DrawingLayer,
    ) -> Option<&Path> {
        self.asset(device, (key, layer), &drawing.contours, drawing.fill_rule)
    }

    fn asset(
        &mut self,
        device: &Device,
        key: (AssetKey, u32),
        contours: &[QuadraticContour],
        fill_rule: FillRule,
    ) -> Option<&Path> {
        let item = self.assets.entry(key).or_insert_with(|| Cached {
            path: Path::contours(&mut self.asset_pages, device, contours, fill_rule),
            last_frame: self.frame,
        });
        item.last_frame = self.frame;
        item.path.as_ref()
    }

    pub(super) fn plot(&mut self, device: &Device, plot: &Arc<PlotPath>) -> Option<&Path> {
        let (_, item) = self
            .plots
            .entry(Arc::as_ptr(plot) as usize)
            .or_insert_with(|| {
                (
                    Arc::downgrade(plot),
                    Cached {
                        path: Path::plot(&mut self.plot_pages, device, plot),
                        last_frame: self.frame,
                    },
                )
            });
        item.last_frame = self.frame;
        item.path.as_ref()
    }
}

impl Path {
    fn contours(
        pages: &mut PathPages,
        device: &Device,
        contours: &[QuadraticContour],
        rule: FillRule,
    ) -> Option<Self> {
        let mut points = Vec::new();
        for contour in contours {
            let mut current = contour.start;
            for segment in &contour.segments {
                let (control, to) = match *segment {
                    QuadraticSegment::Line {
                        to,
                    } => (midpoint(current, to), to),
                    QuadraticSegment::Quadratic {
                        control,
                        to,
                    } => (control, to),
                };
                monotonic_edges(&mut points, current, control, to);
                current = to;
            }
            if current != contour.start {
                monotonic_edges(
                    &mut points,
                    current,
                    midpoint(current, contour.start),
                    contour.start,
                );
            }
        }
        let mode = match rule {
            FillRule::NonZero => 3.0,
            FillRule::EvenOdd => 4.0,
        };
        Self::upload(pages, device, &points, mode, 0.0)
    }

    fn plot(pages: &mut PathPages, device: &Device, plot: &PlotPath) -> Option<Self> {
        #[cfg(feature = "instrumentation")]
        let _trace = ipp_core::profiling::CpuTraceScope::new("RenderService.path-upload");
        match &plot.geometry {
            PlotPathGeometry::Contours(contours) => {
                Self::contours(pages, device, contours, plot.fill_rule)
            }
            PlotPathGeometry::Fill(triangles) => Self::upload(pages, device, triangles, 5.0, 0.0),
            PlotPathGeometry::Stroke {
                segments,
                radius,
            } => {
                let points: Vec<_> = segments
                    .iter()
                    .map(|&[ax, ay, bx, by]| [[ax, ay], [bx, by], [ax, ay]])
                    .collect();
                Self::upload(pages, device, &points, 6.0, *radius)
            }
        }
    }

    fn upload(
        pages: &mut PathPages,
        device: &Device,
        points: &[[[f32; 2]; 3]],
        mode: f32,
        radius: f32,
    ) -> Option<Self> {
        if points.is_empty() || points.len() > 1_000_000 {
            return None;
        }
        let bounds = points.iter().flatten().fold(
            [
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ],
            |[l, t, r, b], &[x, y]| {
                [
                    l.min(x - radius),
                    t.min(y - radius),
                    r.max(x + radius),
                    b.max(y + radius),
                ]
            },
        );
        if bounds[0] >= bounds[2] || bounds[1] >= bounds[3] {
            return None;
        }

        let mut data = path_index(points, bounds, mode, radius);
        for &[[ax, ay], [bx, by], [cx, cy]] in points {
            let quadratic = ay - 2.0 * by + cy;
            let linear = 2.0 * (by - ay);
            data.push([ax, ay, bx, by]);
            data.push([cx, cy, quadratic, linear]);
        }

        let (buffer, start) = pages.upload(device, &data)?;
        Some(Self {
            buffer,
            start,
            bounds,
            mode,
            radius,
        })
    }
}

fn path_index(points: &[[[f32; 2]; 3]], bounds: [f32; 4], mode: f32, radius: f32) -> Vec<[f32; 4]> {
    // A successful fill grid replaces the band index; build only the index
    // this immutable path will actually upload.
    if mode == 5.0
        && let Some(grid) = crate::fill_grid::fill_grid(points, bounds)
    {
        return grid;
    }

    let row_count = points.len().div_ceil(8).clamp(1, 256);
    let scale = row_count as f32 / (bounds[3] - bounds[1]);
    let row = |y: f32| (((y - bounds[1]) * scale).floor() as usize).min(row_count - 1);

    // Scan bands retain only records which can change coverage in a sample
    // of that band. Bound duplication for paths made of long vertical edges.
    let mut rows = vec![Vec::new(); row_count];
    let mut candidates = 0;
    for (index, &[a, b, c]) in points.iter().enumerate() {
        let min = row(a[1].min(b[1]).min(c[1]) - radius);
        let max = row(a[1].max(b[1]).max(c[1]) + radius);
        candidates += max - min + 1;
        if candidates > 4_000_000 {
            rows = vec![(0..points.len() as u32).collect()];
            break;
        }
        for row in &mut rows[min..=max] {
            row.push(index as u32);
        }
    }

    if matches!(mode, 3.0 | 4.0) {
        let records = 1 + rows.len() + rows.iter().map(Vec::len).sum::<usize>().div_ceil(4);
        if let Some(grid) = winding_grid(points, bounds, records) {
            return grid;
        }
    }

    band_index(&rows, bounds[1], bounds[3])
}

fn band_index(rows: &[Vec<u32>], top: f32, bottom: f32) -> Vec<[f32; 4]> {
    let mut data = vec![[0.0; 4]; 1 + rows.len()];
    let mut offset = 0;
    for (i, row) in rows.iter().enumerate() {
        data[1 + i] = [offset as f32, row.len() as f32, 0.0, 0.0];
        offset += row.len();
    }

    let mut ids = rows.iter().flatten().map(|&id| id as f32);
    for _ in 0..offset.div_ceil(4) {
        data.push(std::array::from_fn(|_| ids.next().unwrap_or(0.0)));
    }

    let scale = rows.len() as f32 / (bottom - top);
    data[0] = [rows.len() as f32, top, scale, data.len() as f32];
    data
}

// Keep small contours in a bounded local grid. A monotone edge that lies to
// the right and spans the full cell height contributes its direction to every
// sample there. Only the remaining edges require fragment intersection tests.
fn winding_grid(
    points: &[[[f32; 2]; 3]],
    [left, top, right, bottom]: [f32; 4],
    band_records: usize,
) -> Option<Vec<[f32; 4]>> {
    if !(6..=192).contains(&points.len()) {
        return None;
    }

    let side = points.len().div_ceil(5).clamp(3, 8);
    let scale_x = side as f32 / (right - left);
    let scale_y = side as f32 / (bottom - top);
    if ![scale_x, scale_y].iter().all(|s| s.is_finite() && *s > 0.0) {
        return None;
    }

    // Allow for rounding in the GPU's multiply and the CPU's inverse
    // mapping of cell boundaries. Ambiguous edges stay in the candidate list.
    let pad_x = 16.0 * f32::EPSILON * (1.0 + left.abs().max(right.abs()));
    let pad_y = 16.0 * f32::EPSILON * (1.0 + top.abs().max(bottom.abs()));
    let mut index = vec![[0.0; 4]; 2 + side * side];
    let mut ids = Vec::new();
    let edge_records = 2 * points.len();
    let budget = 3 * (band_records + edge_records) - edge_records;
    for y in 0..side {
        let cell_top = top + y as f32 / scale_y - pad_y;
        let cell_bottom = top + (y + 1) as f32 / scale_y + pad_y;
        for x in 0..side {
            let cell_left = left + x as f32 / scale_x - pad_x;
            let cell_right = left + (x + 1) as f32 / scale_x + pad_x;
            let start = ids.len();
            let mut winding = 0i32;
            for (id, &[a, control, c]) in points.iter().enumerate() {
                let ymin = a[1].min(c[1]);
                let ymax = a[1].max(c[1]);
                if ymin == ymax
                    || ymin > cell_bottom
                    || ymax <= cell_top
                    || a[0].max(control[0]).max(c[0]) < cell_left
                {
                    continue;
                }

                if ymin <= cell_top
                    && ymax > cell_bottom
                    && a[0].min(control[0]).min(c[0]) > cell_right
                {
                    winding += if c[1] > a[1] {
                        1
                    } else {
                        -1
                    };
                } else {
                    ids.push(id as f32);
                }
            }

            index[2 + y * side + x] = [
                start as f32,
                (ids.len() - start) as f32,
                winding as f32,
                0.0,
            ];
        }

        // Cap the entire indexed path at three times the original size.
        if index.len() + ids.len().div_ceil(4) > budget {
            return None;
        }
    }

    for chunk in ids.chunks(4) {
        index.push(std::array::from_fn(|i| {
            chunk.get(i).copied().unwrap_or(0.0)
        }));
    }
    index[0] = [-(side as f32), top, scale_y, index.len() as f32];
    index[1] = [left, scale_x, right, bottom];
    Some(index)
}

fn midpoint(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    lerp(a, b, 0.5)
}

fn lerp(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
    std::array::from_fn(|i| a[i] * (1.0 - t) + b[i] * t)
}

fn monotonic_edges(
    output: &mut Vec<[[f32; 2]; 3]>,
    from: [f32; 2],
    control: [f32; 2],
    end: [f32; 2],
) {
    let pivot = (from[1] - control[1]) / (from[1] - 2.0 * control[1] + end[1]);
    if pivot > 0.0 && pivot < 1.0 {
        let left = lerp(from, control, pivot);
        let right = lerp(control, end, pivot);
        let shared = lerp(left, right, pivot);
        output.extend([[from, left, shared], [shared, right, end]]);
    } else if from[1] != end[1] {
        output.push([from, control, end]);
    }
}

#[cfg(test)]
#[path = "paths_tests.rs"]
mod tests;
