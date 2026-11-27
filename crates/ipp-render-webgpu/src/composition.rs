//! Compose child outputs in dependency order with separate upload storage for each encoded pass.

use crate::{
    camera::CameraBuffers,
    canvas::{CanvasAssets, CanvasBinding, CanvasPass, Draws},
    canvas_buffers::{CanvasBuffers, CanvasLoad},
    canvas_device::RecordBuffer,
    device::{Graphics, Target},
    paths::PathCache,
    scene::Scene,
    service::{FrameSummary, RenderError},
};
use ipp_core::{
    HostRuntime, OutputKind, OutputRef, WorldPublicationId, systems::canvas::CanvasPaintEntry,
};
use ipp_core::{
    WorldViewport,
    systems::{
        plot::{PlotPlaneLayout, PlotPlanePlacement},
        render::plot_presentation::{RenderEntity, ScenePlotPlane, plane_facing, view_placement},
    },
};
use ipp_render_scene::{
    canvas::{
        CanvasPlacement, CanvasProjection, CanvasScene, PaintSources, image_layer_order,
        surface_projection,
    },
    lighting::Lighting,
    screen_damage::{CanvasSurfaceView, Repaint, ScreenDamage},
};
use std::collections::BTreeMap;

type ImageKey = (OutputRef, [u64; 2]);

pub(super) struct Composition {
    scene: Scene,
    plot_layouts: BTreeMap<
        OutputRef,
        ipp_core::systems::render::plot_presentation::label_layout::PlotLabelLayoutState,
    >,
    paths: PathCache,
    cached: BTreeMap<ImageKey, crate::canvas_images::CachedCanvas>,
    shadows: BTreeMap<OutputRef, (crate::device::ShadowAtlas, u64)>,
    images: BTreeMap<ImageKey, crate::camera_images::CameraImage>,
    canvas_buffers: Vec<CanvasBuffers>,
    cameras: Vec<CameraBuffers>,
    canvas_white: wgpu::BindGroup,
    empty_path: RecordBuffer,
    frame: u64,
    pub(super) damage: Option<ScreenDamage>,
    overlay: Option<Overlay>,
    surface_paints: BTreeMap<(OutputRef, OutputRef), SurfacePaint>,
}

impl Composition {
    /// Frozen publications can lose HTML image authority independently of their revision.
    #[cfg(target_arch = "wasm32")]
    pub fn invalidate_web_images(&mut self) {
        self.cached.clear();
        self.images.clear();
        self.damage = None;
        self.overlay = None;
        self.surface_paints.clear();
    }

    pub fn new(gpu: &Graphics, canvas_white: wgpu::BindGroup) -> Self {
        Self {
            cached: BTreeMap::new(),
            scene: Scene::default(),
            plot_layouts: BTreeMap::new(),
            paths: PathCache::default(),
            images: BTreeMap::new(),
            shadows: BTreeMap::new(),
            canvas_buffers: Vec::new(),
            cameras: Vec::new(),
            canvas_white,
            empty_path: RecordBuffer::new(gpu, 2, 1),
            frame: 0,
            damage: None,
            overlay: None,
            surface_paints: BTreeMap::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        gpu: &Graphics,
        host: &HostRuntime,
        target: &Target,
        encoder: &mut wgpu::CommandEncoder,
        white: &wgpu::BindGroup,
        identity: &wgpu::BindGroup,
        output: OutputRef,
        publication: WorldPublicationId,
        time: f64,
    ) -> Result<FrameSummary, RenderError> {
        self.frame += 1;
        self.paths.next_frame();
        self.surface_paints.retain(|_, p| self.frame - p.frame <= 1);
        self.plot_layouts.retain(|&output, _| {
            host.latest_publication(output.world().id())
                .and_then(|id| host.output(id, output))
                .is_some()
        });
        self.cached.retain(|_, image| self.frame - image.frame <= 1);
        self.shadows.retain(|_, (_, last)| self.frame - *last <= 1);
        self.images.retain(|_, image| self.frame - image.frame <= 1);
        let extent = [target.color.width(), target.color.height()];
        let old_damage = self.damage.take();
        let mut frame = Frame {
            state: self,
            gpu,
            host,
            encoder,
            white,
            identity,
            viewport: extent,
            summary: Default::default(),
            camera: 0,
            canvas: 0,
            pixels: 0,
            time,
            old_damage,
            next_damage: None,
            repaint: None,
            forward: Vec::new(),
        };
        match output.kind() {
            OutputKind::Camera => {
                frame.camera(target, output, publication, extent.map(f64::from), 0)?
            }
            OutputKind::Canvas => {
                let scene = CanvasScene::new(host, output, publication)
                    .ok_or(RenderError::Draw("Invalid root Canvas"))?;
                let logical = scene.canvas.logical_extent;
                let direct = fullscreen_camera(host, scene);
                if direct.is_none() {
                    frame.damage(scene, target, None);
                }
                if let Some((camera, id, size)) = direct {
                    frame.camera(target, camera, id, size, 1)?;
                }
                if scene
                    .canvas
                    .entries
                    .iter()
                    .any(|e| matches!(e.as_ref(), CanvasPaintEntry::Attachment(_)))
                    && frame.overlay(scene, target, direct.is_some())?
                {
                    return frame.finish();
                }
                let mut draws = Draws::default();
                frame.canvas(
                    scene,
                    &CanvasPlacement::root(logical),
                    &CanvasProjection::viewport(logical),
                    None,
                    0,
                    0,
                    &mut draws,
                    0,
                    direct.is_some(),
                )?;
                frame.encode(target, &draws, direct.is_none() && frame.repaint.is_none())?;
            }
        }
        frame.finish()
    }
}

// Only a single analytic root has one scene depth at every covered pixel.
struct SurfacePaint {
    view: CanvasSurfaceView,
    revisions: [u64; 4],
    extent: [f32; 2],
    size: [u32; 2],
    image: Option<Target>,
    frame: u64,
}

struct Overlay {
    flattened: Option<(Draws, CanvasBuffers)>,
    size: [u32; 2],
    camera_images: Vec<wgpu::BindGroup>,
    skip_first: bool,
    sources: Vec<(OutputRef, [u64; 4], [u32; 2])>,
    draws: Draws,
    buffers: CanvasBuffers,
}

struct Frame<'a> {
    state: &'a mut Composition,
    gpu: &'a Graphics,
    host: &'a HostRuntime,
    encoder: &'a mut wgpu::CommandEncoder,
    white: &'a wgpu::BindGroup,
    identity: &'a wgpu::BindGroup,
    viewport: [u32; 2],
    summary: FrameSummary,
    camera: usize,
    canvas: usize,
    pixels: u64,
    time: f64,
    old_damage: Option<ScreenDamage>,
    next_damage: Option<ScreenDamage>,
    repaint: Option<Repaint>,
    forward: Vec<(Target, wgpu::RenderBundle)>,
}

impl Frame<'_> {
    fn take_forward(&mut self, target: &Target) -> Option<wgpu::RenderBundle> {
        let index = self
            .forward
            .iter()
            .position(|(t, _)| t.color == target.color)?;
        Some(self.forward.swap_remove(index).1)
    }

    fn flush_forward(&mut self, target: &Target) -> Result<(), RenderError> {
        if self.forward.iter().any(|(t, _)| t.color == target.color) {
            self.encode_canvas(target, &Draws::default(), CanvasLoad::Root)?;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<FrameSummary, RenderError> {
        while let Some((target, _)) = self.forward.last() {
            self.flush_forward(&target.clone())?;
        }
        self.state.cameras.truncate(self.camera);
        self.state.canvas_buffers.truncate(self.canvas);
        if self.summary.failed == 0 {
            self.state.damage = self.next_damage;
        }
        Ok(self.summary)
    }

    #[allow(clippy::too_many_arguments)]
    fn overlay_sources(
        &mut self,
        scene: CanvasScene<'_>,
        placement: &CanvasPlacement,
        skip: usize,
        sources: &mut Vec<(OutputRef, [u64; 4], [u32; 2])>,
        images: &mut Vec<wgpu::BindGroup>,
        included: &mut BTreeMap<OutputRef, WorldPublicationId>,
        depth: u32,
    ) -> Result<(), RenderError> {
        if depth >= 64 {
            return Err(RenderError::Draw("Canvas attachment depth exceeded"));
        }
        let c = scene.canvas;
        sources.push((
            c.selection,
            [
                c.layout_revision,
                c.paint_revision,
                c.resource_revision,
                c.paints_revision,
            ],
            c.logical_extent.map(f32::to_bits),
        ));
        included.insert(c.selection, scene.world.id);
        for entry in c.entries.iter().skip(skip) {
            if let CanvasPaintEntry::Attachment(slot) = entry.as_ref()
                && let Some((output, id)) = scene.child(self.host, slot)
            {
                if let Some(child) = CanvasScene::new(self.host, output, id) {
                    if let Some(placed) = placement.slot(slot, child.canvas.logical_extent) {
                        self.overlay_sources(
                            child,
                            &placed,
                            0,
                            sources,
                            images,
                            included,
                            depth + 1,
                        )?;
                    }
                } else if output.kind() == OutputKind::Camera
                    && placement
                        .slot(slot, slot.physical_extent.map(|v| v as f32))
                        .is_some()
                {
                    images.push(
                        self.image(output, id, slot.physical_extent, depth + 1)?
                            .image,
                    );
                }
            }
        }
        Ok(())
    }

    fn overlay(
        &mut self,
        scene: CanvasScene<'_>,
        target: &Target,
        skip_first: bool,
    ) -> Result<bool, RenderError> {
        let mut sources = Vec::new();
        let mut included = BTreeMap::new();
        let mut images = Vec::new();
        self.overlay_sources(
            scene,
            &CanvasPlacement::root(scene.canvas.logical_extent),
            usize::from(skip_first),
            &mut sources,
            &mut images,
            &mut included,
            0,
        )?;
        let previous = self.state.overlay.take();
        let reuse = previous.as_ref().is_some_and(|p| {
            p.sources == sources && p.camera_images == images && p.skip_first == skip_first
        });
        let mut overlay = previous.unwrap_or_else(|| Overlay {
            flattened: None,
            size: [0; 2],
            sources: Vec::new(),
            camera_images: Vec::new(),
            skip_first,
            draws: Draws::default(),
            buffers: Default::default(),
        });
        if !reuse {
            overlay.flattened = None;
            overlay.sources = sources;
            overlay.camera_images = images;
            overlay.skip_first = skip_first;
            overlay.draws = Draws::default();
            let extent = scene.canvas.logical_extent;
            self.canvas(
                scene,
                &CanvasPlacement::root(extent),
                &CanvasProjection::viewport(extent),
                None,
                0,
                0,
                &mut overlay.draws,
                0,
                skip_first,
            )?;
        }
        let size = [target.color.width(), target.color.height()];
        if size != overlay.size {
            overlay.size = size;
            overlay.flattened = None;
        }
        let new_flattened = reuse
            && overlay.flattened.is_none()
            && self.flatten_overlay(&mut overlay, scene.canvas.logical_extent)?;
        let (draws, buffers, upload) = if let Some((draws, buffers)) = &mut overlay.flattened {
            (&*draws, buffers, new_flattened)
        } else {
            (&overlay.draws, &mut overlay.buffers, !reuse)
        };
        let forward = self.take_forward(target);
        buffers.encode(
            self.gpu,
            self.encoder,
            target,
            draws,
            if skip_first {
                CanvasLoad::Load
            } else {
                CanvasLoad::Root
            },
            None,
            &self.state.canvas_white,
            &self.state.empty_path,
            &mut self.summary,
            upload,
            forward,
        )?;
        self.summary.included.extend(included);
        if overlay.draws.failed == 0 {
            self.state.overlay = Some(overlay);
        }
        Ok(true)
    }

    fn flatten_overlay(
        &mut self,
        overlay: &mut Overlay,
        extent: [f32; 2],
    ) -> Result<bool, RenderError> {
        // Child Cameras are sampled live between groups of stable GUI paint in painter order.
        let runs = std::mem::take(&mut overlay.draws.runs);
        let is_camera = |run: &crate::canvas::Run| {
            run.image
                .as_ref()
                .is_some_and(|image| overlay.camera_images.contains(image))
        };
        let groups = || runs.chunk_by(|a, b| is_camera(a) == is_camera(b));
        let pixels = u64::from(overlay.size[0]) * u64::from(overlay.size[1]);
        let candidates = groups()
            .filter(|g| !is_camera(&g[0]) && (g.len() > 1 || g[0].end - g[0].start > 30))
            .count() as u64;
        if candidates == 0 || self.pixels + candidates * pixels > 16 * 1024 * 1024 {
            overlay.draws.runs = runs;
            return Ok(false);
        }
        self.pixels += candidates * pixels;
        let mut composite = Draws {
            records: overlay.draws.records.clone(),
            ..Default::default()
        };
        for group in groups() {
            if is_camera(&group[0]) || group.len() == 1 && group[0].end - group[0].start <= 30 {
                for run in group {
                    let mut copy = run.clone();
                    copy.start = composite.vertices.len() as u32;
                    composite.vertices.extend_from_slice(
                        &overlay.draws.vertices[run.start as usize..run.end as usize],
                    );
                    copy.end = composite.vertices.len() as u32;
                    composite.runs.push(copy);
                }
                continue;
            }
            let image = Target::new(self.gpu, overlay.size);
            overlay.draws.runs.extend_from_slice(group);
            self.encode_canvas(&image, &overlay.draws, CanvasLoad::Image)?;
            overlay.draws.runs.clear();

            let mut low = [f32::INFINITY; 2];
            let mut high = [f32::NEG_INFINITY; 2];
            for run in group {
                for vertex in &overlay.draws.vertices[run.start as usize..run.end as usize] {
                    let [x, y, _, w] = vertex.clip;
                    let p = [
                        (1. + x / w) * 0.5 * extent[0],
                        (1. - y / w) * 0.5 * extent[1],
                    ];
                    for i in 0..2 {
                        low[i] = low[i].min(p[i]);
                        high[i] = high[i].max(p[i]);
                    }
                }
            }
            let pad = [
                extent[0] / overlay.size[0] as f32,
                extent[1] / overlay.size[1] as f32,
            ];
            let placement = CanvasPlacement::rectangle([
                low[0] - pad[0],
                low[1] - pad[1],
                high[0] + pad[0],
                high[1] + pad[1],
            ]);
            CanvasPass {
                plot_target: None,
                draws: &mut composite,
                projection: &CanvasProjection::viewport(extent),
                light_offset: 0,
                mapping: 0,
            }
            .draw_image(&placement, CanvasBinding::Bitmap(&image.image), extent);
        }
        overlay.draws.runs = runs;
        overlay.flattened = Some((composite, CanvasBuffers::default()));
        Ok(true)
    }

    fn damage(&mut self, scene: CanvasScene<'_>, target: &Target, view: Option<CanvasSurfaceView>) {
        if let Some((damage, repaint)) = ScreenDamage::prepare(
            self.old_damage.take(),
            self.host,
            scene,
            [target.color.width(), target.color.height()],
            view,
        ) {
            self.next_damage = Some(damage);
            self.repaint = repaint;
        }
    }

    fn encode(&mut self, target: &Target, draws: &Draws, clear: bool) -> Result<(), RenderError> {
        self.encode_canvas(
            target,
            draws,
            if clear {
                CanvasLoad::Root
            } else {
                CanvasLoad::Load
            },
        )
    }

    fn encode_canvas(
        &mut self,
        target: &Target,
        draws: &Draws,
        load: CanvasLoad,
    ) -> Result<(), RenderError> {
        if self.canvas == self.state.canvas_buffers.len() {
            self.state.canvas_buffers.push(Default::default());
        }
        let forward = self.take_forward(target);
        self.state.canvas_buffers[self.canvas].encode(
            self.gpu,
            self.encoder,
            target,
            draws,
            load,
            self.repaint.as_ref().map(|r| r.scissor),
            &self.state.canvas_white,
            &self.state.empty_path,
            &mut self.summary,
            true,
            forward,
        )?;
        self.canvas += 1;
        Ok(())
    }

    fn image(
        &mut self,
        output: OutputRef,
        id: WorldPublicationId,
        physical: [f64; 2],
        depth: u32,
    ) -> Result<Target, RenderError> {
        if depth >= 64 || physical.iter().any(|v| !v.is_finite() || *v <= 0.) {
            return Err(RenderError::Draw(
                "Invalid Camera attachment extent or depth",
            ));
        }
        let scale = (f64::from(self.viewport[0]) / physical[0])
            .min(f64::from(self.viewport[1]) / physical[1]);
        let size = physical.map(|v| (v * scale).round().max(1.) as u32);
        let key = (output, physical.map(f64::to_bits));
        if let Some(image) = self.state.images.get(&key)
            && image.frame == self.state.frame
        {
            self.summary.included.extend(&image.included);
            return Ok(image.target.clone());
        }
        self.pixels += u64::from(size[0]) * u64::from(size[1]);
        if self.pixels > 16 * 1024 * 1024 {
            return Err(RenderError::Draw("Camera attachment pixel budget exceeded"));
        }
        let mut nodes = Vec::new();
        let mut publications = BTreeMap::new();
        let content = crate::camera_images::collect(
            self.host,
            Some(output),
            id,
            &mut nodes,
            &mut publications,
            0,
        )
        .map(|_| nodes);
        let mut image = self
            .state
            .images
            .remove(&key)
            .filter(|i| [i.target.color.width(), i.target.color.height()] == size)
            .unwrap_or_else(|| crate::camera_images::CameraImage {
                target: Target::new(self.gpu, size),
                frame: 0,
                content: None,
                included: Default::default(),
            });
        image.frame = self.state.frame;
        if content.is_some() && image.content == content {
            for (output, id) in &mut image.included {
                *id = publications[output];
            }
        } else {
            let parent = std::mem::take(&mut self.summary.included);
            let failures = self.summary.failed;
            self.camera(&image.target, output, id, physical, depth)?;
            self.flush_forward(&image.target)?;
            image.included = std::mem::replace(&mut self.summary.included, parent);
            image.content = content.filter(|_| self.summary.failed == failures);
        }
        self.summary.included.extend(&image.included);
        let target = image.target.clone();
        self.state.images.insert(key, image);
        Ok(target)
    }

    #[allow(clippy::too_many_arguments)]
    fn cached_canvas(
        &mut self,
        visit: &crate::scene::SurfaceVisit<'_>,
        scene: CanvasScene<'_>,
        view: [f32; 16],
        camera: [f32; 16],
        lighting: &Lighting,
        viewport: [u32; 2],
        depth: u32,
    ) -> Result<Option<Target>, RenderError> {
        use crate::canvas_images::{CachedCanvas, Content, image_projection};
        use ipp_core::systems::surface::SurfaceCachePolicy;
        let Some(geometry) = &visit.edge.surface_geometry else {
            return Ok(None);
        };
        let layered = visit.edge.layer_spacing != 0. && scene.canvas.layers.len() > 1;
        let affine = geometry.exact_affine(0.).is_some();
        let authored = visit.edge.surface_cache_policy;
        if authored.is_none() && (affine || !layered) {
            return Ok(None);
        }
        let policy = authored.unwrap_or(SurfaceCachePolicy {
            direct_distance: 100_000.,
            resolution_scale: 1.,
            allocation_priority: 1.,
            max_refresh_hz: 240.,
            max_texture_size: 0,
            force_cache: false,
        });
        let physical = geometry.physical_extent();
        let key = (visit.output, physical.map(f64::to_bits));
        let previous = self.state.cached.get(&key);
        if let Some(image) = previous
            && image.frame == self.state.frame
        {
            self.summary.included.extend(&image.included);
            return Ok(Some(image.target.clone()));
        }
        let distance = (visit.model[12] - camera[12])
            .hypot(visit.model[13] - camera[13])
            .hypot(visit.model[14] - camera[14]);
        let band = previous.map_or_else(
            || policy.initial_band(distance),
            |c| policy.band(distance, c.band),
        );
        let content = Content::leaf(self.host, scene);
        if affine && (band == 0 || content.is_none() && !layered) {
            return Ok(None);
        }
        let Some(size) = ipp_render_scene::canvas::projected_size(
            geometry,
            ipp_core::math::multiply(view, visit.model),
            viewport,
            policy,
        ) else {
            return Ok(None);
        };
        let pixels = u64::from(size[0])
            * u64::from(size[1])
            * if layered {
                2
            } else {
                1
            };
        if self.pixels + pixels > 16 * 1024 * 1024 {
            return Ok(None);
        }
        self.pixels += pixels;

        let previous = self.state.cached.remove(&key).filter(|old| {
            old.target.color.width() == size[0]
                && old.target.color.height() == size[1]
                && old.target.owner.is_some() == layered
        });
        let reusable = previous.as_ref().is_some_and(|c| {
            content.is_some()
                && (c.content == content
                    || !layered
                        && self.time >= c.time
                        && self.time - c.time < policy.refresh_interval_at(band))
        });
        if reusable {
            let mut image = previous.unwrap();
            if image.content == content {
                image.included.insert(visit.output, visit.publication);
            }
            image.band = band;
            image.frame = self.state.frame;
            self.summary.included.extend(&image.included);
            let target = image.target.clone();
            self.state.cached.insert(key, image);
            return Ok(Some(target));
        }
        let target = previous.map_or_else(
            || Target::canvas_image(self.gpu, size, layered),
            |c| c.target,
        );
        let extent = scene.canvas.logical_extent;
        let scale = std::array::from_fn(|i| physical[i] / f64::from(extent[i]));
        let projection = image_projection(
            surface_projection(view, visit.model, geometry, scale, 0.),
            extent,
        );
        let parent = std::mem::take(&mut self.summary.included);
        let failures = self.summary.failed;
        let mut draws = Draws::default();
        add_lighting(&mut draws, lighting);
        self.canvas(
            scene,
            &CanvasPlacement::root(extent),
            &projection,
            None,
            0,
            0,
            &mut draws,
            depth,
            false,
        )?;
        self.encode_canvas(&target, &draws, CanvasLoad::Image)?;
        if layered {
            self.encode_canvas(&target, &draws, CanvasLoad::Owner)?;
        }
        let included = std::mem::replace(&mut self.summary.included, parent);
        if self.summary.failed != failures {
            return Ok(None);
        }
        self.summary.included.extend(&included);
        self.state.cached.insert(
            key,
            CachedCanvas {
                target: target.clone(),
                content,
                included,
                time: self.time,
                frame: self.state.frame,
                band,
            },
        );
        Ok(Some(target))
    }

    #[allow(clippy::too_many_arguments)]
    fn surface_paint(
        &mut self,
        camera: OutputRef,
        child: CanvasScene<'_>,
        surface: &ipp_render_scene::analytic_surface::AnalyticSurface<'_>,
        view: CanvasSurfaceView,
        size: [u32; 2],
        placement: &CanvasPlacement,
        draws: &mut Draws,
        depth: u32,
    ) -> Result<bool, RenderError> {
        let canvas = child.canvas;
        if surface.needs_near_root()
            || self.repaint.is_some()
            || canvas
                .entries
                .iter()
                .any(|e| matches!(e.as_ref(), CanvasPaintEntry::Attachment(_)))
        {
            return Ok(false);
        }
        let key = (camera, canvas.selection);
        let revisions = [
            canvas.layout_revision,
            canvas.paint_revision,
            canvas.resource_revision,
            canvas.paints_revision,
        ];
        let stable = self.state.surface_paints.get(&key).is_some_and(|p| {
            p.view == view
                && p.size == size
                && p.extent == canvas.logical_extent
                && p.revisions == revisions
        });
        if !stable {
            self.state.surface_paints.insert(
                key,
                SurfacePaint {
                    view,
                    revisions,
                    extent: canvas.logical_extent,
                    size,
                    image: None,
                    frame: self.state.frame,
                },
            );
            return Ok(false);
        }

        let visible = surface.draw(
            &placement.polygon,
            ipp_render_scene::canvas::bounds(&placement.polygon),
            placement.affine.inverse().unwrap(),
        );
        let pixels = u64::from(size[0]) * u64::from(size[1]);
        if visible.is_none() || self.pixels + pixels > 16 * 1024 * 1024 {
            return Ok(false);
        }
        self.pixels += pixels;
        let projection = CanvasProjection::Analytic(surface);
        let image = if let Some(target) = &self.state.surface_paints[&key].image {
            target.clone()
        } else {
            let target = Target::new(self.gpu, size);
            let mut paint = Draws::default();
            add_lighting(&mut paint, &view.lighting);
            let mapping = paint.records.len() as u32;
            paint.records.extend(surface.batch_mapping().0.records());
            let failures = self.summary.failed;
            self.canvas(
                child,
                placement,
                &projection,
                None,
                0,
                mapping,
                &mut paint,
                depth,
                false,
            )?;
            self.encode_canvas(&target, &paint, CanvasLoad::Image)?;
            if self.summary.failed != failures {
                self.state.surface_paints.remove(&key);
                return Ok(false);
            }
            self.state.surface_paints.get_mut(&key).unwrap().image = Some(target.clone());
            target
        };
        self.state.surface_paints.get_mut(&key).unwrap().frame = self.state.frame;
        let mapping = draws.records.len() as u32;
        draws.records.extend(surface.batch_mapping().0.records());
        CanvasPass {
            plot_target: None,
            draws,
            projection: &projection,
            light_offset: 0,
            mapping,
        }
        .draw_image(
            placement,
            CanvasBinding::Screen(&image.image),
            canvas.logical_extent,
        );
        self.summary
            .included
            .insert(canvas.selection, child.world.id);
        Ok(true)
    }

    fn camera(
        &mut self,
        target: &Target,
        output: OutputRef,
        id: WorldPublicationId,
        extent: [f64; 2],
        depth: u32,
    ) -> Result<(), RenderError> {
        let mut scene = self
            .state
            .scene
            .prepare(self.host, self.gpu, output, id, extent)?;
        if depth == 0
            && scene.failed == 0
            && scene.batches.is_empty()
            && scene.plots.is_empty()
            && let [visit] = scene.surfaces.as_slice()
            && visit.edge.layer_spacing == 0.
            && visit.edge.surface_cache_policy.is_none()
            && let Some(child) = CanvasScene::new(self.host, visit.output, visit.publication)
            && let Some(geometry) = &visit.edge.surface_geometry
        {
            let view = CanvasSurfaceView {
                camera: output,
                projection: scene.projection,
                model: visit.model,
                geometry: geometry.clone(),
                lighting: scene.lighting,
            };
            self.damage(child, target, Some(view));
        }
        let mut shadow_bind = &self.gpu.empty_shadow.bind;
        if scene.shadow_grid > 0 {
            let size = ipp_render_scene::lighting::SHADOW_TILE * scene.shadow_grid;
            let entry = self.state.shadows.entry(output).or_insert_with(|| {
                (
                    crate::device::ShadowAtlas::new(
                        &self.gpu.device,
                        &self.gpu.shadow_layout,
                        size,
                    ),
                    0,
                )
            });
            if entry.0.size != size {
                entry.0 = crate::device::ShadowAtlas::new(
                    &self.gpu.device,
                    &self.gpu.shadow_layout,
                    size,
                );
            }
            entry.1 = self.state.frame;
            let atlas = &entry.0;
            for (tile, view) in scene.shadow_views {
                if self.camera == self.state.cameras.len() {
                    self.state.cameras.push(Default::default());
                }
                for batch in &mut scene.casters {
                    batch.camera.projection = view;
                }
                let commands = self.state.cameras[self.camera].prepare(
                    self.gpu,
                    self.white,
                    self.identity,
                    &scene.casters,
                    &scene.caster_instances,
                    &mut self.summary,
                    true,
                    &self.gpu.empty_shadow.bind,
                )?;
                crate::camera::draw_shadow(self.encoder, atlas, tile, scene.shadow_grid, &commands);
                self.camera += 1;
            }
            shadow_bind = &atlas.bind;
        }
        let forward = if self.repaint.is_none() {
            if self.camera == self.state.cameras.len() {
                self.state.cameras.push(Default::default());
            }
            let bundle = self.state.cameras[self.camera].prepare(
                self.gpu,
                self.white,
                self.identity,
                &scene.batches,
                &self.state.scene.instances,
                &mut self.summary,
                false,
                shadow_bind,
            )?;
            self.camera += 1;
            Some(bundle)
        } else {
            None
        };
        self.summary.failed += scene.failed;
        let mut draws = Draws::default();
        if !scene.surfaces.is_empty() || !scene.plots.is_empty() {
            add_lighting(&mut draws, &scene.lighting);
        }
        if !scene.plots.is_empty() {
            let mut planes = Vec::new();
            for (publication, placement, plots) in scene.plots {
                for chart in &plots.charts {
                    let chart_model = ipp_core::math::multiply(placement, chart.model);
                    for plane in chart.planes.iter() {
                        let model = ipp_core::math::multiply(chart_model, plane.model);
                        let model = view_placement::model(
                            model,
                            chart_model,
                            scene.camera_pose,
                            plane.placement,
                        )
                        .and_then(|model| {
                            plane_facing::model(model, plane.facing, scene.camera_pose)
                        })
                        .map_err(|_| RenderError::Draw("Invalid plot projection"))?;
                        let paint = match (plane.layout, plane.placement) {
                            (PlotPlaneLayout::Callout, _) => chart.callout_paint.as_ref(),
                            (
                                PlotPlaneLayout::None,
                                PlotPlanePlacement::Axis {
                                    ..
                                },
                            ) => chart.axis_paint.as_ref(),
                            _ => None,
                        };
                        planes.push(ScenePlotPlane {
                            entity: RenderEntity {
                                world: publication.world,
                                entity: chart.target.entity,
                                incarnation: chart.target.incarnation,
                            },
                            target: chart.target,
                            publication: publication.id,
                            model,
                            chart_model,
                            paint,
                            geometry: &chart.geometry,
                            plane,
                        });
                    }
                }
            }
            let frame = self
                .state
                .plot_layouts
                .entry(output)
                .or_default()
                .frame(
                    &planes,
                    scene.camera_pose,
                    scene.projection,
                    WorldViewport {
                        width: target.color.width(),
                        height: target.color.height(),
                        device_pixel_ratio: 1.0,
                    },
                    self.time,
                )
                .map_err(|_| RenderError::Draw("Invalid plot projection"))?;
            for plane in frame.iter() {
                let model = plane.model;
                let projection = CanvasProjection::Plane {
                    matrix: ipp_core::math::multiply(scene.projection, model),
                    world: Some(model),
                };
                let sources = PaintSources::Plot {
                    publication: plane.publication,
                    paints: [plane.paint, None],
                };
                let placement = CanvasPlacement::rectangle(plane.plane.clip);
                let mut pass = CanvasPass {
                    plot_target: Some([target.color.width(), target.color.height()]),
                    draws: &mut draws,
                    projection: &projection,
                    light_offset: 0,
                    mapping: 0,
                };
                for primitive in plane.plane.primitives.iter() {
                    pass.primitive(
                        &sources,
                        &placement,
                        None,
                        primitive,
                        CanvasAssets {
                            host: self.host,
                            device: self.gpu,
                            paths: &mut self.state.paths,
                        },
                    );
                }
            }
        }

        for visit in scene.surfaces {
            let Some(geometry) = &visit.edge.surface_geometry else {
                continue;
            };
            let physical = geometry.physical_extent();
            match visit.output.kind() {
                OutputKind::Canvas => {
                    let child = CanvasScene::new(self.host, visit.output, visit.publication)
                        .ok_or(RenderError::Draw("Invalid SurfaceCanvas"))?;
                    let logical = child.canvas.logical_extent;
                    let scale = std::array::from_fn(|i| physical[i] / f64::from(logical[i]));
                    let placement = CanvasPlacement::root(logical);
                    let spacing = visit.edge.layer_spacing;
                    if let Some(image) = self.cached_canvas(
                        &visit,
                        child,
                        scene.projection,
                        scene.camera_pose,
                        &scene.lighting,
                        [target.color.width(), target.color.height()],
                        depth + 1,
                    )? {
                        let extent = physical.map(|v| v as f32);
                        let placement = CanvasPlacement::root(extent);
                        if image.owner.is_some() {
                            for (id, offset) in image_layer_order(
                                child,
                                geometry,
                                scene.projection,
                                visit.model,
                                spacing,
                            ) {
                                let projection = surface_projection(
                                    scene.projection,
                                    visit.model,
                                    geometry,
                                    [1.; 2],
                                    offset,
                                );
                                CanvasPass {
                                    plot_target: None,
                                    draws: &mut draws,
                                    projection: &projection,
                                    light_offset: 0,
                                    mapping: 0,
                                }
                                .draw_image(
                                    &placement,
                                    CanvasBinding::Layer(&image.image, id),
                                    extent,
                                );
                            }
                        } else {
                            let projection = surface_projection(
                                scene.projection,
                                visit.model,
                                geometry,
                                [1.; 2],
                                0.,
                            );
                            CanvasPass {
                                plot_target: None,
                                draws: &mut draws,
                                projection: &projection,
                                light_offset: 0,
                                mapping: 0,
                            }
                            .draw_image(
                                &placement,
                                CanvasBinding::Bitmap(&image.image),
                                extent,
                            );
                        }
                    } else if spacing == 0.0
                        && geometry.exact_affine(0.).is_none()
                        && let Some(mut surface) =
                            ipp_render_scene::analytic_surface::AnalyticSurface::prepare(
                                geometry,
                                scene.projection,
                                visit.model,
                                scale,
                                [target.color.width(), target.color.height()],
                            )
                    {
                        // The single-Surface root already retains damaged regions in its own target.
                        if depth > 0 || !scene.batches.is_empty() {
                            let view = CanvasSurfaceView {
                                camera: output,
                                projection: scene.projection,
                                model: visit.model,
                                geometry: geometry.clone(),
                                lighting: scene.lighting,
                            };
                            if self.surface_paint(
                                output,
                                child,
                                &surface,
                                view,
                                [target.color.width(), target.color.height()],
                                &placement,
                                &mut draws,
                                depth + 1,
                            )? {
                                continue;
                            }
                        }
                        let roots = if surface.needs_near_root() {
                            &[false, true][..]
                        } else {
                            &[false][..]
                        };
                        for near in roots {
                            surface.select_root(*near);
                            let offset = draws.records.len() as u32;
                            draws.records.extend(surface.batch_mapping().0.records());
                            self.canvas(
                                child,
                                &placement,
                                &CanvasProjection::Analytic(&surface),
                                None,
                                0,
                                offset,
                                &mut draws,
                                depth + 1,
                                false,
                            )?;
                        }
                    } else if spacing == 0.0 {
                        let projection =
                            surface_projection(scene.projection, visit.model, geometry, scale, 0.);
                        self.canvas(
                            child,
                            &placement,
                            &projection,
                            None,
                            0,
                            0,
                            &mut draws,
                            depth + 1,
                            false,
                        )?;
                    } else {
                        for (layer, offset) in image_layer_order(
                            child,
                            geometry,
                            scene.projection,
                            visit.model,
                            spacing,
                        ) {
                            let projection = surface_projection(
                                scene.projection,
                                visit.model,
                                geometry,
                                scale,
                                offset,
                            );
                            self.canvas(
                                child,
                                &placement,
                                &projection,
                                Some(layer),
                                0,
                                0,
                                &mut draws,
                                depth + 1,
                                false,
                            )?;
                        }
                    }
                }
                OutputKind::Camera => {
                    let image = self.image(visit.output, visit.publication, physical, depth + 1)?;
                    let projection =
                        surface_projection(scene.projection, visit.model, geometry, [1.; 2], 0.);
                    let logical = physical.map(|v| v as f32);
                    CanvasPass {
                        plot_target: None,
                        draws: &mut draws,
                        projection: &projection,
                        light_offset: 0,
                        mapping: 0,
                    }
                    .draw_image(
                        &CanvasPlacement::root(logical),
                        CanvasBinding::Bitmap(&image.image),
                        logical,
                    );
                }
            }
        }
        if let Some(bundle) = forward {
            self.forward.push((target.clone(), bundle));
        }
        if !draws.runs.is_empty() || draws.failed > 0 || self.repaint.is_some() {
            self.encode(target, &draws, false)?;
        }
        self.summary.included.insert(output, id);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn canvas(
        &mut self,
        scene: CanvasScene<'_>,
        placement: &CanvasPlacement,
        projection: &CanvasProjection<'_>,
        layer: Option<u32>,
        light_offset: u32,
        mapping: u32,
        draws: &mut Draws,
        depth: u32,
        skip_first: bool,
    ) -> Result<(), RenderError> {
        if depth >= 64 {
            return Err(RenderError::Draw("Canvas attachment depth exceeded"));
        }
        let repaint_indices = self.repaint.as_ref().map(|r| r.indices.clone());
        let indices = repaint_indices.as_deref();
        let entries = &scene.canvas.entries;
        for index in usize::from(skip_first)..indices.map_or(entries.len(), <[usize]>::len) {
            let entry = &entries[indices.map_or(index, |v| v[index])];
            if layer.is_some_and(|l| l != entry.layer()) {
                continue;
            }
            let inherited_owner = draws.owner;
            draws.owner = Some(inherited_owner.unwrap_or_else(|| entry.layer()));
            match entry.as_ref() {
                CanvasPaintEntry::Primitive {
                    space,
                    primitive,
                    ..
                } => {
                    CanvasPass {
                        plot_target: None,
                        draws,
                        projection,
                        light_offset,
                        mapping,
                    }
                    .primitive(
                        &PaintSources::Canvas(scene),
                        placement,
                        space.as_deref(),
                        primitive,
                        CanvasAssets {
                            host: self.host,
                            device: self.gpu,
                            paths: &mut self.state.paths,
                        },
                    );
                }
                CanvasPaintEntry::Attachment(slot) => {
                    let Some((output, id)) = scene.child(self.host, slot) else {
                        draws.owner = inherited_owner;
                        continue;
                    };
                    match output.kind() {
                        OutputKind::Canvas => {
                            let child = CanvasScene::new(self.host, output, id)
                                .ok_or(RenderError::Draw("Invalid attached Canvas"))?;
                            if let Some(placed) = placement.slot(slot, child.canvas.logical_extent)
                            {
                                self.canvas(
                                    child,
                                    &placed,
                                    projection,
                                    None,
                                    light_offset,
                                    mapping,
                                    draws,
                                    depth + 1,
                                    false,
                                )?;
                            }
                        }
                        OutputKind::Camera => {
                            let logical = slot.physical_extent.map(|v| v as f32);
                            if let Some(placed) = placement.slot(slot, logical) {
                                let target =
                                    self.image(output, id, slot.physical_extent, depth + 1)?;
                                CanvasPass {
                                    plot_target: None,
                                    draws,
                                    projection,
                                    light_offset,
                                    mapping,
                                }
                                .draw_image(
                                    &placed,
                                    CanvasBinding::Bitmap(&target.image),
                                    logical,
                                );
                            }
                        }
                    }
                }
            }
            draws.owner = inherited_owner;
        }
        self.summary
            .included
            .insert(scene.canvas.selection, scene.world.id);
        Ok(())
    }
}

fn add_lighting(draws: &mut Draws, lighting: &Lighting) {
    draws
        .records
        .extend([lighting.camera, lighting.ambient_count]);
    draws.records.extend(lighting.lights.iter().flatten());
}

fn fullscreen_camera(
    host: &HostRuntime,
    scene: CanvasScene<'_>,
) -> Option<(OutputRef, WorldPublicationId, [f64; 2])> {
    let CanvasPaintEntry::Attachment(slot) = scene.canvas.entries.first()?.as_ref() else {
        return None;
    };
    let (output, publication) = scene.child(host, slot)?;
    if output.kind() != OutputKind::Camera || slot.space.is_some() || slot.opacity != 1.0 {
        return None;
    }
    let [w, h] = slot.physical_extent;
    let extent = scene.canvas.logical_extent.map(f64::from);
    let origin = slot.to_drawing([-w * 0.5, h * 0.5]);
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-5 * b.abs().max(1.0);
    if slot.scale[0] <= 0.0
        || !close(slot.scale[0], -slot.scale[1])
        || !close(origin[0], 0.0)
        || !close(origin[1], 0.0)
        || !close(slot.scale[0] * w, extent[0])
        || !close(-slot.scale[1] * h, extent[1])
        || slot.clip[0] > 0.0
        || slot.clip[1] > 0.0
        || f64::from(slot.clip[2]) < extent[0]
        || f64::from(slot.clip[3]) < extent[1]
    {
        return None;
    }
    Some((output, publication, slot.physical_extent))
}
