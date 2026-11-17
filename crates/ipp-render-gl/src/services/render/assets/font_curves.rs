//! Shared curve textures for the glyphs referenced by live Canvas and Plot output.
//! Output leases count glyphs by immutable font identity. Packed offsets are local
//! to one texture generation, including when a second World changes the union.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU64, Ordering};

use ipp_core::services::asset_management::formats::quadratic::{
    QuadraticContour, QuadraticSegment,
};
use ipp_core::services::asset_management::{AssetKey, AssetProvider, formats::font::FontAsset};
use ipp_core::systems::canvas::CanvasPrimitive;

use super::context::RenderAssetLease;
use super::loaders::SharedRenderDevice;
use super::paths::{
    SurfaceBandTexels, SurfaceCurveTexels, SurfacePathTexels, pack_surface_paths_with_limit,
};
use super::surface_resources::GlFontData;
use crate::services::render::device::SurfacePathDescriptor;
use crate::services::render::statistics::RenderUploadCounter;
use crate::{RenderDevice, RenderError};

/// The two minimum-sized textures admit the graphics representation through the
/// normal loader/context barrier before any World has published glyph demand.
pub(super) fn empty_texels() -> SurfacePathTexels {
    SurfacePathTexels {
        curves: SurfaceCurveTexels::Int16(vec![[0; 4]]),
        curve_scale: 1.0,
        bands: SurfaceBandTexels::Uint16(vec![0]),
    }
}

fn flip_contour(contour: &QuadraticContour) -> QuadraticContour {
    let flip = |[x, y]: [f32; 2]| [x, -y];
    QuadraticContour {
        start: flip(contour.start),
        segments: contour
            .segments
            .iter()
            .map(|segment| match *segment {
                QuadraticSegment::Line {
                    to,
                } => QuadraticSegment::Line {
                    to: flip(to),
                },
                QuadraticSegment::Quadratic {
                    control,
                    to,
                } => QuadraticSegment::Quadratic {
                    control: flip(control),
                    to: flip(to),
                },
            })
            .collect(),
    }
}

pub(in crate::services::render) struct FontCurves<D: RenderDevice> {
    font: Weak<FontAsset>,
    dirty: bool,
    device: SharedRenderDevice<D>,
    lease: RenderAssetLease,
    uploads: RenderUploadCounter,
    demand: BTreeMap<u32, usize>,
    ready_path: Option<D::SurfacePath>,
    frame: u64,
    retry_at: Option<u64>,
    pub atlas: Option<Rc<FontCurveAtlas<D>>>,
}

pub(in crate::services::render) struct FontCurveAtlas<D: RenderDevice> {
    device: SharedRenderDevice<D>,
    lease: RenderAssetLease,
    path: Option<D::SurfacePath>,
    pub ranges: BTreeMap<u32, SurfacePathDescriptor>,
    pub generation: u64,
    bytes: usize,
}

impl<D: RenderDevice> FontCurveAtlas<D> {
    pub fn path(&self) -> &D::SurfacePath {
        self.path.as_ref().expect("resident font curve textures")
    }
}

impl<D: RenderDevice> Drop for FontCurveAtlas<D> {
    fn drop(&mut self) {
        if let Some(path) = self.path.take()
            && self.lease.is_current()
        {
            self.device.borrow_mut().delete_surface_path(path);
        }
    }
}

impl<D: RenderDevice> FontCurves<D> {
    pub(super) fn new(
        font: Weak<FontAsset>,
        device: SharedRenderDevice<D>,
        lease: RenderAssetLease,
        uploads: RenderUploadCounter,
        ready_path: Option<D::SurfacePath>,
    ) -> Self {
        Self {
            font,
            dirty: false,
            device,
            lease,
            uploads,
            ready_path,
            frame: 0,
            retry_at: None,
            demand: BTreeMap::new(),
            atlas: None,
        }
    }

    pub fn failed(&self) -> bool {
        self.retry_at.is_some()
    }

    pub(super) fn bytes(&self) -> usize {
        self.atlas.as_ref().map_or(0, |atlas| atlas.bytes)
            + if self.ready_path.is_some() {
                size_of::<[i16; 4]>() + size_of::<u16>()
            } else {
                0
            }
    }

    pub(super) fn invalidate(&mut self) {
        self.atlas = None;
        if let Some(path) = self.ready_path.take()
            && self.lease.is_current()
        {
            self.device.borrow_mut().delete_surface_path(path);
        }
    }

    fn release(&mut self, id: u32) {
        let count = self
            .demand
            .get_mut(&id)
            .expect("registered font curve demand");
        *count -= 1;
        if *count == 0 {
            self.demand.remove(&id);
            self.dirty = true;
            self.retry_at = None;
            if self.demand.is_empty() {
                self.atlas = None;
            }
        }
    }

    fn prepare(&mut self) -> Result<(), RenderError> {
        if self.retry_at.is_some_and(|retry| self.frame < retry)
            || !self.dirty
            || self.demand.is_empty()
        {
            return Ok(());
        }
        if !self.lease.is_current() {
            return Err(RenderError::ContextLost);
        }

        let Some(font) = self.font.upgrade() else {
            return Ok(());
        };
        let glyphs: Vec<_> = self
            .demand
            .keys()
            .filter_map(|&id| {
                let glyph = font.glyph(id)?;
                if glyph.contours.is_empty() {
                    return None;
                }
                let [left, bottom, right, top] = glyph.bounds;
                let contours: Vec<_> = glyph.contours.iter().map(flip_contour).collect();
                Some((id, [left, -top, right, -bottom], contours))
            })
            .collect();
        if glyphs.is_empty() {
            return Ok(());
        }

        // Font outlines keep the ordinary band lookup used by the font loader.
        let packed = pack_surface_paths_with_limit(
            glyphs
                .iter()
                .map(|(_, bounds, contours)| (*bounds, contours.as_slice())),
            0,
        );
        // Allocate the exact compact replacement. A growable path can reserve
        // almost double the actual curve and band payload until the next repack.
        let mut device = self.device.borrow_mut();
        let path = match device.create_surface_path(&packed.texels) {
            Ok(path) => path,
            Err(RenderError::ContextLost) => return Err(RenderError::ContextLost),
            Err(_) => {
                self.retry_at = Some(
                    self.frame
                        .saturating_add(super::super::retained::glyph_atlas::POPULATE_RETRY_FRAMES),
                );
                return Ok(());
            }
        };
        if let Some(empty) = self.ready_path.take() {
            device.delete_surface_path(empty);
        }

        drop(device);
        self.dirty = false;
        self.retry_at = None;
        let bytes = packed.texels.byte_len();
        self.uploads.add(bytes);
        static NEXT_LAYOUT: AtomicU64 = AtomicU64::new(1);
        self.atlas = Some(Rc::new(FontCurveAtlas {
            device: self.device.clone(),
            lease: self.lease.clone(),
            path: Some(path),
            bytes,
            generation: NEXT_LAYOUT
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
                .expect("font curve layout identity exhausted"),
            ranges: glyphs
                .iter()
                .map(|(id, _, _)| *id)
                .zip(packed.descriptors)
                .collect(),
        }));
        Ok(())
    }
}

struct FontGlyphLease<D: RenderDevice> {
    owner: Weak<RefCell<FontCurves<D>>>,
    glyphs: BTreeSet<u32>,
}

impl<D: RenderDevice> FontGlyphLease<D> {
    fn replace(&mut self, next: BTreeSet<u32>, owner: &mut FontCurves<D>) {
        for &id in self.glyphs.difference(&next) {
            owner.release(id);
        }
        for &id in next.difference(&self.glyphs) {
            let count = owner.demand.entry(id).or_default();
            *count += 1;
            if *count == 1 {
                owner.dirty = true;
                owner.retry_at = None;
            }
        }
        self.glyphs = next;
    }
}

impl<D: RenderDevice> Drop for FontGlyphLease<D> {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.upgrade() {
            let mut owner = owner.borrow_mut();
            for &id in &self.glyphs {
                owner.release(id);
            }
            // Compact a shared font after an output disappears, so the remaining
            // Worlds keep their live ranges and reclaim dropped ranges immediately.
            if owner.atlas.is_some() && owner.lease.is_current() && owner.prepare().is_err() {
                owner.invalidate();
            }
        }
    }
}

pub(in crate::services::render) struct FontCurveDemand<D: RenderDevice> {
    fonts: BTreeMap<AssetKey, FontGlyphLease<D>>,
    complete: bool,
}

impl<D: RenderDevice> FontCurveDemand<D> {
    pub fn new() -> Self {
        Self {
            fonts: BTreeMap::new(),
            complete: false,
        }
    }

    pub fn publish<'a>(
        &mut self,
        primitives: impl Iterator<Item = &'a CanvasPrimitive>,
        resource: impl Fn(AssetKey) -> Option<&'a AssetProvider>,
        reusable: bool,
        frame: u64,
    ) -> Result<(), RenderError> {
        let ready_font = |key| {
            resource(key)
                .and_then(|provider| provider.data())
                .and_then(|asset| asset.as_any().downcast_ref::<GlFontData<D>>())
                .filter(|font| font.graphics_prepared)
        };

        // An explicitly unloaded or recovered provider can keep its asset key.
        let same_fonts = |(key, lease): (&AssetKey, &FontGlyphLease<D>)| {
            ready_font(*key).is_some_and(|font| lease.owner.ptr_eq(&Rc::downgrade(&font.curves)))
        };
        if !reusable || !self.complete || !self.fonts.iter().all(same_fonts) {
            let mut wanted = BTreeMap::<AssetKey, BTreeSet<u32>>::new();
            for primitive in primitives {
                if let CanvasPrimitive::Glyphs {
                    font,
                    glyphs,
                    ..
                } = primitive
                {
                    wanted
                        .entry(*font)
                        .or_default()
                        .extend(glyphs.iter().map(|glyph| glyph.glyph_id));
                }
            }
            self.fonts.retain(|key, _| wanted.contains_key(key));
            self.complete = true;
            for (key, mut ids) in wanted {
                let Some(font) = ready_font(key) else {
                    self.fonts.remove(&key);
                    self.complete = false;
                    continue;
                };
                ids.retain(|id| {
                    font.font
                        .glyph(*id)
                        .is_some_and(|glyph| !glyph.contours.is_empty())
                });
                let owner = Rc::downgrade(&font.curves);
                if self
                    .fonts
                    .get(&key)
                    .is_some_and(|lease| !lease.owner.ptr_eq(&owner))
                {
                    self.fonts.remove(&key);
                }
                let lease = self.fonts.entry(key).or_insert_with(|| FontGlyphLease {
                    owner,
                    glyphs: BTreeSet::new(),
                });
                lease.replace(ids, &mut font.curves.borrow_mut());
            }
        }

        for key in self.fonts.keys() {
            if let Some(font) = ready_font(*key) {
                let mut curves = font.curves.borrow_mut();
                curves.frame = frame;
                curves.prepare()?;
            }
        }

        Ok(())
    }
}
