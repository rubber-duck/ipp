use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

use ipp_core::{
    services::asset_management::{
        AssetKey,
        formats::{
            drawing::{DrawingLayer, FillRule},
            quadratic::QuadraticContour,
        },
    },
    systems::plot::PlotPath,
};

use crate::{canvas_device::RecordBuffer, device::Graphics as Device};

pub(super) struct Path {
    pub(super) buffer: wgpu::BindGroup,
    pub(super) start: u32,
    pub(super) bounds: [f32; 4],
    // Nonzero contour, even-odd contour, triangle union or stroke union.
    pub(super) mode: f32,
    pub(super) radius: f32,
}

const PATH_PAGE_RECORDS: usize = 64 * 1024;

/// Append to unused ranges only. Paths and submitted WebGPU commands retain
/// old pages independently of the current allocation cursor.
#[derive(Default)]
struct PathPages {
    current: Option<RecordBuffer>,
    next_record: usize,
}

impl PathPages {
    fn upload(&mut self, device: &Device, data: &[[f32; 4]]) -> Option<(wgpu::BindGroup, u32)> {
        if data.len() > RecordBuffer::limit(&device.device) {
            return None;
        }
        if data.len() > PATH_PAGE_RECORDS {
            let table = RecordBuffer::new(device, 2, data.len());
            table.write(&device.queue, 0, data);
            return Some((table.bind, 0));
        }
        if self.current.is_none() || self.next_record + data.len() > PATH_PAGE_RECORDS {
            self.current = Some(RecordBuffer::new(device, 2, PATH_PAGE_RECORDS));
            self.next_record = 0;
        }
        let page = self.current.as_ref()?;
        let start = self.next_record;
        page.write(&device.queue, start, data);
        self.next_record += data.len();
        Some((page.bind.clone(), start as u32))
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
            path: PathRecords::contours(contours, fill_rule)
                .and_then(|p| Path::upload(&mut self.asset_pages, device, p)),
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
                        path: PathRecords::plot(plot)
                            .and_then(|p| Path::upload(&mut self.plot_pages, device, p)),
                        last_frame: self.frame,
                    },
                )
            });
        item.last_frame = self.frame;
        item.path.as_ref()
    }
}

use ipp_render_scene::paths::PathRecords;
impl Path {
    fn upload(pages: &mut PathPages, device: &Device, data: PathRecords) -> Option<Self> {
        let (buffer, start) = pages.upload(device, &data.records)?;
        Some(Self {
            buffer,
            start,
            bounds: data.bounds,
            mode: data.mode,
            radius: data.radius,
        })
    }
}
