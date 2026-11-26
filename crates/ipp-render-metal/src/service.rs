use std::{cell::RefCell, collections::BTreeMap, fmt, rc::Rc};

use ipp_core::{
    HostRuntime, OutputKind, OutputPublicationObservation, OutputRef, PublishedSceneTraversal,
    PublishedWorldAttachment, RenderItem, WorldAttachmentMode, WorldPublicationId, WorldViewport,
    services::{
        asset_management::AssetKey,
        web_content::{WebContentImageId, WebContentRequestId},
    },
    systems::{
        camera::CameraPublication,
        canvas::{CanvasPaintEntry, CanvasPrimitive},
        geometry::{GeometryBounds, GeometryPublication, GeometrySystem, frustum_planes},
        plot::{PlotPlaneLayout, PlotPlanePlacement, PlotPublication, PlotSystem},
        render::{
            RenderPublication, RenderSystem,
            plot_presentation::{
                RenderEntity, ScenePlotPlane, label_layout::PlotLabelLayoutState, plane_facing,
                view_placement,
            },
        },
        surface::SurfaceCachePolicy,
    },
};

use crate::{
    analytic_surface::AnalyticSurface,
    assets::{self, MetalMesh, MetalTexture},
    canvas::{
        CanvasAssets, CanvasPass, CanvasPlacement, CanvasProjection, CanvasScene, PaintSources,
        camera_slots,
    },
    canvas_batches::{CanvasBatches, Lookup, PRIMITIVES_PER_RUN, RunKey},
    canvas_damage::CanvasDamage,
    canvas_images::{self, CachedCanvas, CanvasContent},
    custom_mesh::CustomDraw,
    derived_meshes::DerivedMeshes,
    device::{
        Blend, CanvasBinding, Device, Frame, Material, MeshBuffers, MeshStreams, Object, Target,
    },
    lighting::{LightCatalog, LightSelections, Lighting, SHADOW_TILE},
    particles,
    paths::PathCache,
    shaders,
    web_images::{self, MetalWebImage, WebImages},
};

const BACKGROUND: [f32; 4] = [0.003095975, 0.004400849, 0.007194409, 1.0];
const IDENTITY: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

/// A scoped failure to configure or present a Metal output.
#[derive(Debug)]
pub enum MetalRenderError {
    /// The exact completed Canvas publication is unavailable or invalid.
    InvalidCanvas,
    /// The exact completed camera publication is unavailable or invalid.
    InvalidCamera,
    /// The requested extent cannot be presented as configured.
    InvalidViewport,
    /// A Metal allocation, encoding or command buffer failed.
    Device(String),
}

impl fmt::Display for MetalRenderError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCanvas => out.write_str("selected canvas publication is invalid"),
            Self::InvalidCamera => out.write_str("selected camera publication is invalid"),
            Self::InvalidViewport => out.write_str("invalid Metal viewport"),
            Self::Device(reason) => write!(out, "Metal: {reason}"),
        }
    }
}

impl std::error::Error for MetalRenderError {}

/// Completed draw statistics.
#[derive(Default)]
pub struct MetalFrameSummary {
    /// Submitted draw calls.
    pub draw_calls: u32,
    /// Submitted triangles.
    pub triangles: u32,
    /// Published draws whose resources or material path were unavailable.
    pub failed_draw_calls: u32,
    /// GPU execution duration of a measured draw without readback.
    #[cfg(feature = "instrumentation")]
    pub gpu_elapsed_ns: Option<u64>,
}

impl MetalFrameSummary {
    pub(super) fn fail(&mut self) {
        self.failed_draw_calls = self.failed_draw_calls.saturating_add(1);
    }
}

/// Exact Host-selected source, dimensions and clock for one native presentation.
#[derive(Clone, Copy)]
pub struct MetalPresentation {
    /// Root Camera or Canvas output.
    pub output: OutputRef,
    /// Most recent completed World publication selected by the Host.
    pub publication: WorldPublicationId,
    /// Physical presentation viewport.
    pub viewport: WorldViewport,
    /// Host-owned presentation time in seconds.
    pub time: f64,
}

struct SurfaceView<'a> {
    projection: [f32; 16],
    camera: [f32; 16],
    viewport: [u32; 2],
    lighting: &'a Lighting,
}

#[derive(Clone, Copy)]
struct OutputSource {
    output: OutputRef,
    publication: WorldPublicationId,
}

struct DrawResources<'a> {
    frame: &'a mut Frame,
    summary: &'a mut MetalFrameSummary,
    images: BTreeMap<ImageKey, PreparedImage>,
    fullscreen: Option<(OutputSource, [f64; 2])>,
    included: BTreeMap<OutputRef, WorldPublicationId>,
    viewport: [u32; 2],
    target_pixels: u64,
    time: f64,
}

type ImageKey = (OutputRef, [u64; 2]);

struct PreparedImage {
    target: Rc<Target>,
    included: BTreeMap<OutputRef, WorldPublicationId>,
}

struct RetainedImage {
    target: Rc<Target>,
    size: [u32; 2],
    frame: u64,
}

impl DrawResources<'_> {
    fn image_key(output: OutputRef, physical: [f64; 2]) -> ImageKey {
        (output, physical.map(f64::to_bits))
    }
}

struct SurfaceVisit<'a> {
    entity: RenderEntity,
    edge: &'a PublishedWorldAttachment,
    child: OutputSource,
    model: [f32; 16],
    analytic: Option<AnalyticSurface<'a>>,
    column_start: Option<u32>,
}

/// Metal device and resource loaders for one Host's offscreen presentation.
pub struct MetalRenderService {
    device: Rc<Device>,
    traversal: PublishedSceneTraversal,
    viewport: Option<(u32, u32)>,
    instances: Vec<Material>,
    particle_quad: MeshBuffers,
    paths: RefCell<PathCache>,
    generated: DerivedMeshes,
    plot_layouts: BTreeMap<OutputRef, PlotLabelLayoutState>,
    light_selections: LightSelections,
    targets: BTreeMap<ImageKey, RetainedImage>,
    cached_canvases: BTreeMap<OutputRef, CachedCanvas>,
    canvas_batches: RefCell<CanvasBatches>,
    frame_number: u64,
    shadows: BTreeMap<OutputRef, (u32, u64, Rc<Object>)>,
    web_images: WebImages,
    #[cfg(feature = "instrumentation")]
    gpu_timing: bool,
}

// Bound transient column storage to 2 MiB per camera. Groups beyond the
// budget continue to use the analytic fragment intersection.
fn prepare_canvas_columns(
    surfaces: &mut [SurfaceVisit<'_>],
    view: [f32; 16],
    viewport: [u32; 2],
    frame: &mut Frame,
) {
    let stride = viewport[0] + 2;
    let capacity = 16.min(32_768 / stride as usize);
    let mut mappings = Vec::new();

    for visit in surfaces.iter_mut() {
        if visit.child.output.kind() != OutputKind::Canvas
            || visit.edge.layer_spacing != 0.0
            || visit.edge.surface_cache_policy.is_some()
        {
            continue;
        }
        let Some(geometry) = &visit.edge.surface_geometry else {
            continue;
        };
        if let Some(analytic) =
            AnalyticSurface::prepare(geometry, view, visit.model, [1.0; 2], viewport)
        {
            if mappings.len() < capacity
                && let Some(mapping) = analytic.column_mapping()
            {
                visit.column_start = Some(mappings.len() as u32 * stride);
                mappings.push(mapping);
            }
            visit.analytic = Some(analytic);
        }
    }

    if mappings.is_empty() {
        return;
    }

    #[cfg(feature = "instrumentation")]
    let _trace =
        ipp_core::profiling::CpuTraceScope::new("MetalRenderService.prepare_canvas_columns");
    if !frame.canvas_columns(&mappings, viewport) {
        for visit in surfaces {
            visit.column_start = None;
        }
    }
}

impl MetalRenderService {
    /// Initial Host limit, bounding offscreen attachments and readback allocations.
    pub const MAX_VIEWPORT: u32 = 4096;

    /// Create Metal objects on the Host thread and register the typed GPU loaders.
    pub fn install(host: &mut HostRuntime) -> Result<Self, String> {
        let device = Rc::new(Device::new()?);
        let mesh_device = device.clone();
        host.asset_resources_mut()
            .register_graphics_loader(ipp_core::MESH_TYPE, move || {
                assets::mesh_loader(mesh_device.clone())
            })?;
        let texture_device = device.clone();
        host.asset_resources_mut()
            .register_graphics_loader(ipp_core::TEXTURE_TYPE, move || {
                assets::texture_loader(texture_device.clone())
            })?;

        let shader_device = device.clone();
        host.asset_resources_mut().register_graphics_loader(
            ipp_core::services::asset_management::formats::shader::SHADER_TYPE,
            move || shaders::loader(shader_device.clone()),
        )?;
        let particle_quad = particles::quad(&device)
            .ok_or_else(|| "could not upload the Metal particle quad".to_string())?;
        Ok(Self {
            device,
            particle_quad,
            traversal: PublishedSceneTraversal::default(),
            viewport: None,
            instances: Vec::with_capacity(Material::MAX_INSTANCES),
            paths: RefCell::default(),
            generated: DerivedMeshes::default(),
            plot_layouts: BTreeMap::new(),
            light_selections: LightSelections::default(),
            targets: BTreeMap::new(),
            cached_canvases: BTreeMap::new(),
            canvas_batches: RefCell::default(),
            frame_number: 0,
            shadows: BTreeMap::new(),
            web_images: WebImages::default(),
            #[cfg(feature = "instrumentation")]
            gpu_timing: false,
        })
    }

    /// Replace failed command storage while preserving the Host and its asset identities.
    pub fn recover(&mut self, host: &mut HostRuntime) -> Result<(), MetalRenderError> {
        let device = Device::new().map_err(MetalRenderError::Device)?;
        let quad = particles::quad(&device).ok_or_else(|| {
            MetalRenderError::Device("could not restore particle geometry".into())
        })?;
        if let Some((width, height)) = self.viewport
            && !device.resize(width, height)
        {
            return Err(MetalRenderError::Device(
                "could not restore frame attachments".into(),
            ));
        }

        self.targets.clear();
        self.cached_canvases.clear();
        *self.canvas_batches.get_mut() = CanvasBatches::default();
        self.shadows.clear();
        host.web_content_mut().renderer_invalidated();
        self.web_images = WebImages::default();
        *self.paths.get_mut() = PathCache::default();
        self.generated = DerivedMeshes::default();
        self.plot_layouts.clear();
        self.light_selections = LightSelections::default();
        self.particle_quad = quad;
        self.device.replace(device);

        let resources = host.asset_resources_mut();
        let graphics: Vec<_> = resources
            .iter()
            .filter(|resource| {
                [
                    ipp_core::MESH_TYPE,
                    ipp_core::TEXTURE_TYPE,
                    ipp_core::services::asset_management::formats::shader::SHADER_TYPE,
                ]
                .contains(&resource.source().kind)
            })
            .map(|resource| resource.key())
            .collect();
        for key in graphics {
            resources.invalidate_graphics(key);
        }
        Ok(())
    }

    /// Retire storage after the last exact World publication releases each image.
    pub fn prune_web_images(&mut self, host: &HostRuntime) {
        self.web_images.prune(host);
    }

    /// Reserve exact image authority before asking a producer for accelerated paint.
    pub fn reserve_web_image(
        &self,
        host: &mut HostRuntime,
        request: WebContentRequestId,
        size: [u32; 2],
        outstanding_bytes: usize,
        outstanding_images: usize,
    ) -> Result<WebContentImageId, String> {
        if size.contains(&0) || size.iter().any(|&value| value > Self::MAX_VIEWPORT) {
            return Err("accelerated web image dimensions exceed the device bounds".into());
        }
        if !self.web_images.can_reserve(
            outstanding_bytes.saturating_add(web_images::private_bytes(size)),
            outstanding_images.saturating_add(1),
        ) {
            return Err("accelerated web image allocation budget exhausted".into());
        }
        web_images::reserve(host, request, size)
    }

    /// Import callback-borrowed BGRA IOSurface data into a private texture through Metal.
    ///
    /// # Safety
    ///
    /// The producer must supply a live, immutable IOSurface for the entire call.
    /// Metal reads finish before the call returns. The renderer never retains the source.
    pub unsafe fn import_web_image(
        &self,
        surface: *mut std::ffi::c_void,
        size: [u32; 2],
        byte_budget: usize,
    ) -> Result<MetalWebImage, String> {
        // SAFETY: The producer promises the IOSurface stays live and immutable
        // until synchronous GPU conversion completes.
        unsafe { MetalWebImage::import(&self.device, surface, size, byte_budget) }
    }

    /// Publish a private texture only for the exact reserved and still-live request.
    pub fn complete_web_image(
        &mut self,
        host: &mut HostRuntime,
        request: WebContentRequestId,
        image: WebContentImageId,
        texture: MetalWebImage,
    ) -> Result<(), String> {
        self.web_images.complete(host, request, image, texture)
    }

    /// Actual GPU identity reported by Metal.
    pub fn device_name(&self) -> String {
        self.device.name()
    }

    /// Read command-buffer GPU duration for future completed frames.
    #[cfg(feature = "instrumentation")]
    pub fn set_gpu_timing(&mut self, requested: bool) -> bool {
        self.gpu_timing = requested && Device::gpu_timing_available();
        self.gpu_timing
    }

    /// Allocate or retain the exact offscreen extent before a surface rebind.
    pub fn configure(&mut self, viewport: WorldViewport) -> Result<(), MetalRenderError> {
        if viewport.width == 0
            || viewport.height == 0
            || !viewport.device_pixel_ratio.is_finite()
            || viewport.device_pixel_ratio <= 0.0
            || viewport.width > Self::MAX_VIEWPORT
            || viewport.height > Self::MAX_VIEWPORT
        {
            return Err(MetalRenderError::InvalidViewport);
        }
        if !self.device.resize(viewport.width, viewport.height) {
            return Err(MetalRenderError::Device(
                "could not allocate frame attachments".into(),
            ));
        }
        self.viewport = Some((viewport.width, viewport.height));
        Ok(())
    }

    /// Present an exact output publication and optionally read back top-left RGBA8.
    pub fn draw(
        &mut self,
        host: &HostRuntime,
        view: MetalPresentation,
        capture: Option<&mut [u8]>,
        observations: &mut [OutputPublicationObservation],
    ) -> Result<MetalFrameSummary, MetalRenderError> {
        let result = self.draw_frame(host, view, capture, observations);
        if result.is_err() {
            self.cached_canvases.clear();
            *self.canvas_batches.get_mut() = CanvasBatches::default();
        }
        result
    }

    fn draw_frame(
        &mut self,
        host: &HostRuntime,
        view: MetalPresentation,
        capture: Option<&mut [u8]>,
        observations: &mut [OutputPublicationObservation],
    ) -> Result<MetalFrameSummary, MetalRenderError> {
        let MetalPresentation {
            output,
            publication,
            viewport,
            time,
        } = view;
        #[cfg(feature = "instrumentation")]
        let _cpu_trace = ipp_core::profiling::CpuTraceScope::new("MetalRenderService.draw");

        if self.viewport != Some((viewport.width, viewport.height)) {
            return Err(MetalRenderError::InvalidViewport);
        }
        self.paths.get_mut().next_frame();
        self.frame_number = self.frame_number.saturating_add(1);
        self.canvas_batches.get_mut().begin_frame(self.frame_number);
        self.generated.retain(self.frame_number);
        self.light_selections.begin_frame(host, self.frame_number);
        self.plot_layouts.retain(|&output, _| {
            host.latest_publication(output.world().id())
                .and_then(|publication| host.output(publication, output))
                .is_some()
        });
        self.cached_canvases
            .retain(|_, canvas| self.frame_number - canvas.frame <= 1);
        self.targets
            .retain(|_, image| self.frame_number - image.frame <= 1);
        self.shadows
            .retain(|_, (_, frame, _)| self.frame_number - *frame <= 1);
        let root_canvas = if output.kind() == OutputKind::Canvas {
            Some(
                CanvasScene::new(host, output, publication)
                    .ok_or(MetalRenderError::InvalidCanvas)?,
            )
        } else {
            None
        };
        let mut frame = self.device.frame().ok_or_else(|| {
            MetalRenderError::Device("could not allocate a frame command buffer".into())
        })?;
        let mut summary = MetalFrameSummary::default();
        let mut draw = DrawResources {
            frame: &mut frame,
            summary: &mut summary,
            images: BTreeMap::new(),
            fullscreen: root_canvas.and_then(|scene| fullscreen_camera(host, scene)),
            included: BTreeMap::new(),
            viewport: [viewport.width, viewport.height],
            target_pixels: 0,
            time,
        };
        let source = OutputSource {
            output,
            publication,
        };
        let extent = draw.viewport.map(f64::from);
        self.prepare_children(host, source, &mut draw, 0)?;
        if let Some(scene) = root_canvas {
            if let Some((background, extent)) = draw.fullscreen {
                self.camera_pass(host, background, extent, None, &mut draw)?;
            } else if !draw.frame.pass(None, BACKGROUND, None, None) {
                return Err(MetalRenderError::Device(
                    "could not encode Canvas pass".into(),
                ));
            }
            self.draw_canvas(
                host,
                scene,
                &CanvasPlacement::root(scene.canvas.logical_extent),
                &CanvasProjection::viewport(scene.canvas.logical_extent),
                None,
                &mut draw,
                0,
                None,
            );
        } else {
            self.camera_pass(host, source, extent, None, &mut draw)?;
        }
        let included = draw.included;

        #[cfg(feature = "instrumentation")]
        let mut seconds = 0.0;
        #[cfg(feature = "instrumentation")]
        let gpu_seconds = self.gpu_timing.then_some(&mut seconds);
        #[cfg(not(feature = "instrumentation"))]
        let gpu_seconds = None;
        let merged = frame
            .finish(capture, gpu_seconds)
            .map_err(MetalRenderError::Device)?;
        summary.draw_calls = summary.draw_calls.saturating_sub(merged);
        #[cfg(feature = "instrumentation")]
        if seconds.is_finite() && seconds > 0.0 {
            summary.gpu_elapsed_ns = Some((seconds * 1e9).round() as u64);
        }

        if summary.failed_draw_calls == 0 {
            for observation in observations {
                observation.publication = included.get(&observation.output).copied();
            }
        }
        Ok(summary)
    }

    fn prepare_children(
        &mut self,
        host: &HostRuntime,
        source: OutputSource,
        draw: &mut DrawResources<'_>,
        depth: u32,
    ) -> Result<(), MetalRenderError> {
        if depth >= 64 {
            draw.summary.fail();
            return Ok(());
        }
        match source.output.kind() {
            OutputKind::Canvas => {
                let scene = CanvasScene::new(host, source.output, source.publication)
                    .ok_or(MetalRenderError::InvalidCanvas)?;
                for slot in camera_slots(scene) {
                    if let Some((output, publication)) = scene.child(host, slot) {
                        let child = OutputSource {
                            output,
                            publication,
                        };
                        if output.kind() == OutputKind::Camera
                            && !(depth == 0
                                && draw
                                    .fullscreen
                                    .is_some_and(|(direct, _)| direct.output == output))
                        {
                            self.prepare_image(host, child, slot.physical_extent, draw, depth + 1)?;
                        } else {
                            self.prepare_children(host, child, draw, depth + 1)?;
                        }
                    }
                }
            }
            OutputKind::Camera => {
                for visit in self.surfaces(host, source)? {
                    if visit.child.output.kind() == OutputKind::Camera {
                        let extent = visit
                            .edge
                            .surface_extent
                            .ok_or(MetalRenderError::InvalidCamera)?;
                        self.prepare_image(host, visit.child, extent, draw, depth + 1)?;
                    } else {
                        self.prepare_children(host, visit.child, draw, depth + 1)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn prepare_image(
        &mut self,
        host: &HostRuntime,
        source: OutputSource,
        physical: [f64; 2],
        draw: &mut DrawResources<'_>,
        depth: u32,
    ) -> Result<(), MetalRenderError> {
        let key = DrawResources::image_key(source.output, physical);
        if draw.images.contains_key(&key) {
            return Ok(());
        }
        if depth >= 64
            || physical
                .iter()
                .any(|value| !value.is_finite() || *value <= 0.0)
        {
            draw.summary.fail();
            return Ok(());
        }

        let [w, h] = draw.viewport.map(f64::from);
        let scale = (w / physical[0]).min(h / physical[1]);
        let size = physical.map(|extent| (extent * scale).round().max(1.0) as u32);
        let pixels = u64::from(size[0]) * u64::from(size[1]);
        if draw.target_pixels + pixels > 16 * 1024 * 1024 {
            draw.summary.fail();
            return Ok(());
        }

        let target = self
            .targets
            .get_mut(&key)
            .filter(|image| image.size == size)
            .map(|image| {
                image.frame = self.frame_number;
                image.target.clone()
            });
        let target = match target {
            Some(target) => target,
            None => {
                let Some(target) = self.device.target(size, false).map(Rc::new) else {
                    draw.summary.fail();
                    return Ok(());
                };
                self.targets.insert(
                    key,
                    RetainedImage {
                        target: target.clone(),
                        size,
                        frame: self.frame_number,
                    },
                );
                target
            }
        };
        draw.target_pixels += pixels;
        self.prepare_children(host, source, draw, depth)?;
        let siblings = std::mem::take(&mut draw.included);
        let result = self.camera_pass(host, source, physical, Some(&target), draw);
        let included = std::mem::replace(&mut draw.included, siblings);
        result?;
        draw.images.insert(
            key,
            PreparedImage {
                target,
                included,
            },
        );
        Ok(())
    }

    fn surfaces<'a>(
        &mut self,
        host: &'a HostRuntime,
        source: OutputSource,
    ) -> Result<Vec<SurfaceVisit<'a>>, MetalRenderError> {
        self.traversal
            .reset(host, source.publication, Some(source.output))
            .map_err(|_| MetalRenderError::InvalidCamera)?;
        let mut visits = Vec::new();
        while let Some(visit) = self.traversal.next(host) {
            let parent = visit
                .placement
                .render_matrix()
                .map_err(|_| MetalRenderError::InvalidCamera)?;
            for edge in &visit.publication.attachments {
                if edge.mode == WorldAttachmentMode::Spatial
                    || edge
                        .placement_output
                        .is_some_and(|owner| owner != source.output)
                {
                    continue;
                }
                if let Some((output, publication)) =
                    edge.output.zip(host.attached_publication(edge))
                {
                    let local = edge.placement.map(|v| v as f32);
                    let model = ipp_core::math::multiply(parent, local);
                    visits.push(SurfaceVisit {
                        entity: RenderEntity {
                            world: visit.publication.world,
                            entity: edge.anchor,
                            incarnation: edge.surface_incarnation.unwrap_or(0),
                        },
                        edge,
                        child: OutputSource {
                            output,
                            publication: publication.id,
                        },
                        model,
                        analytic: None,
                        column_start: None,
                    });
                }
            }
        }
        Ok(visits)
    }

    fn camera_pass(
        &mut self,
        host: &HostRuntime,
        source: OutputSource,
        extent: [f64; 2],
        target: Option<&Target>,
        draw: &mut DrawResources<'_>,
    ) -> Result<(), MetalRenderError> {
        let OutputSource {
            output,
            publication,
        } = source;
        let camera = host
            .output(publication, output)
            .and_then(|chunk| chunk.data::<CameraPublication>())
            .ok_or(MetalRenderError::InvalidCamera)?;
        let lights = LightCatalog::prepare(host, &mut self.traversal, output, publication, camera)?;
        let lighting = lights.root();
        let camera_pose = camera
            .pose
            .render_matrix()
            .map_err(|_| MetalRenderError::InvalidCamera)?;
        let camera = camera
            .prepare_for_extent(extent)
            .map_err(|_| MetalRenderError::InvalidCamera)?;
        let mut surfaces = self.surfaces(host, source)?;
        surfaces.sort_by(|a, b| {
            surface_depth(camera.view_projection, b.model)
                .total_cmp(&surface_depth(camera.view_projection, a.model))
                .then_with(|| a.entity.cmp(&b.entity))
        });
        let viewport = target.map_or(draw.viewport, Target::size);
        for surface in &surfaces {
            let selected =
                if lights.over_capacity() && surface.child.output.kind() == OutputKind::Canvas {
                    Some(surface_lights(
                        &lights,
                        output,
                        surface,
                        &mut self.light_selections,
                    ))
                } else {
                    None
                };
            self.prepare_canvas_image(
                host,
                surface,
                SurfaceView {
                    projection: camera.view_projection,
                    camera: camera_pose,
                    viewport,
                    lighting: selected.as_ref().unwrap_or(&lighting),
                },
                draw,
            )?;
        }
        let shadow = self.shadow_pass(host, source, &lights, draw)?;
        prepare_canvas_columns(&mut surfaces, camera.view_projection, viewport, draw.frame);
        self.traversal
            .reset(host, publication, Some(output))
            .map_err(|_| MetalRenderError::InvalidCamera)?;

        if !draw
            .frame
            .pass(target, BACKGROUND, Some(&lighting), shadow.as_deref())
        {
            return Err(MetalRenderError::Device(
                "could not encode Camera pass".into(),
            ));
        }

        let mut pending: Option<MeshBindings<'_>> = None;
        let mut transparency = Vec::new();
        let frustum = frustum_planes(camera.view_projection);
        let mut plots = Vec::new();
        let mut debug = Vec::new();
        self.instances.clear();
        let state = host
            .publication(publication)
            .and_then(|source| source.chunk(RenderSystem::ID))
            .and_then(|chunk| chunk.data::<RenderPublication>())
            .map(|render| render.state)
            .unwrap_or_default();
        while let Some(visit) = self.traversal.next(host) {
            let placement = visit
                .placement
                .render_matrix()
                .map_err(|_| MetalRenderError::InvalidCamera)?;
            if let Some(plot) = visit
                .publication
                .chunk(PlotSystem::ID)
                .and_then(|chunk| chunk.data::<PlotPublication>())
            {
                plots.push((visit.publication, placement, plot));
            }
            let Some(scene) = visit
                .publication
                .chunk(RenderSystem::ID)
                .and_then(|chunk| chunk.data::<RenderPublication>())
            else {
                continue;
            };
            debug.extend(
                scene
                    .debug
                    .iter()
                    .filter(|item| state.show_all_debug_geometries || item.is_rendered)
                    .map(|item| (placement, item)),
            );
            let geometry = visit
                .publication
                .chunk(GeometrySystem::ID)
                .and_then(|chunk| chunk.data::<GeometryPublication>());
            let local_frustum = frustum.map(|plane| visit.placement.local_plane(&plane));
            for item in &scene.items {
                if item.item.particle.is_none()
                    && item
                        .custom
                        .as_ref()
                        .is_none_or(|material| material.conservative_bounds)
                    && let Some(geometry) = geometry
                    && let Ok(index) = geometry
                        .entities
                        .binary_search_by_key(&item.item.entity, |entry| entry.entity)
                    && let Some(shape) = &geometry.entities[index].culling
                    && !shape.intersects_frustum(&local_frustum)
                {
                    continue;
                }
                let entity = RenderEntity {
                    world: visit.publication.world,
                    entity: item.item.entity,
                    incarnation: item.incarnation,
                };
                if let Some(particle) = item.item.particle.filter(|particle| particle.sprite) {
                    let Some((bindings, material)) = sprite_draw(
                        host,
                        visit.publication.id,
                        camera.view_projection,
                        camera_pose,
                        placement,
                        &item.item,
                        &self.particle_quad,
                    ) else {
                        draw.summary.fail();
                        continue;
                    };
                    let depth = if particle.additive {
                        f32::INFINITY
                    } else {
                        material.clip_from_mesh[14]
                    };
                    transparency.push((depth, entity, particle.id, bindings, material));
                    continue;
                }
                let Some((mut bindings, mut material)) = mesh_draw(
                    host,
                    visit.publication.id,
                    camera.view_projection,
                    placement,
                    &item.item,
                    item.palette.as_deref(),
                ) else {
                    draw.summary.fail();
                    continue;
                };
                if let Some(custom) = &item.custom
                    && let Some(mesh) =
                        asset::<MetalMesh>(host, visit.publication.id, item.item.mesh.asset)
                    && let Some(custom) = CustomDraw::resolve(
                        host,
                        visit.publication.id,
                        custom,
                        mesh.metadata.attributes(),
                    )
                {
                    custom.configure(&mut material);
                    material.surface[3] = f32::from(item.item.normals);
                    bindings.blend = custom.blend;
                    bindings.custom = Some(custom);
                }
                if lights.over_capacity()
                    && (item.item.pbr.is_some()
                        || item
                            .custom
                            .as_ref()
                            .is_some_and(|material| material.receives_light))
                {
                    let model = material.world_from_mesh;
                    let bounds =
                        asset::<MetalMesh>(host, visit.publication.id, item.item.mesh.asset)
                            .map(|mesh| mesh.metadata.bounds());
                    let ([x, y, z], radius) = light_receiver(model, bounds);
                    let selected = lights.select(
                        output,
                        entity,
                        [x, y, z],
                        radius,
                        &mut self.light_selections,
                    );
                    bindings.selected_lights = Some(Box::new(selected));
                }
                if bindings.blend != Blend::Opaque {
                    transparency.push((material.clip_from_mesh[14], entity, 0, bindings, material));
                    continue;
                }
                if let Some(previous) = &pending
                    && (!previous.matches(&bindings)
                        || self.instances.len() == Material::MAX_INSTANCES)
                {
                    previous.submit(draw.frame, &self.instances, draw.summary, Some(&lighting));
                    self.instances.clear();
                }
                pending = Some(bindings);
                self.instances.push(material);
            }
        }
        if let Some(bindings) = pending {
            bindings.submit(draw.frame, &self.instances, draw.summary, Some(&lighting));
        }

        for (placement, item) in debug {
            let Some(mesh) = self
                .generated
                .debug(&self.device, &item.geometry, self.frame_number)
            else {
                draw.summary.fail();
                continue;
            };
            let model = ipp_core::math::multiply(placement, item.model);
            let [r, g, b] = item.color_override.unwrap_or(state.debug_geometry_color);
            submit_generated(
                draw,
                mesh,
                model,
                camera.view_projection,
                [r, g, b, 1.0],
                None,
            );
        }

        for (publication, placement, plots) in &plots {
            for chart in &plots.charts {
                for plot in &chart.meshes {
                    let source = &plot.geometry.meshes[plot.mesh_index];
                    let custom = source
                        .material
                        .as_ref()
                        .zip(plot.render.custom.as_ref())
                        .and_then(|(streams, material)| {
                            let attributes = u32::from(!streams.colors.is_empty())
                                | (u32::from(!streams.uvs.is_empty()) << 1)
                                | (u32::from(!streams.normals.is_empty()) << 2);
                            CustomDraw::resolve(host, publication.id, material, attributes)
                        });
                    let Some(mesh) = self.generated.plot(&self.device, plot, custom.is_some())
                    else {
                        draw.summary.fail();
                        continue;
                    };
                    let color = plot.geometry.meshes[plot.mesh_index].color;
                    let model = ipp_core::math::multiply(*placement, plot.render.item.model);
                    submit_generated(draw, mesh, model, camera.view_projection, color, custom);
                }
            }
        }

        if !plots.is_empty() {
            let mut planes = Vec::new();
            for (publication, placement, plots) in plots {
                for chart in &plots.charts {
                    let chart_model = ipp_core::math::multiply(placement, chart.model);
                    for plane in chart.planes.iter() {
                        let model = ipp_core::math::multiply(chart_model, plane.model);
                        let model =
                            view_placement::model(model, chart_model, camera_pose, plane.placement)
                                .and_then(|model| {
                                    plane_facing::model(model, plane.facing, camera_pose)
                                })
                                .map_err(|_| MetalRenderError::InvalidCamera)?;
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
                .plot_layouts
                .entry(output)
                .or_default()
                .frame(
                    &planes,
                    camera_pose,
                    camera.view_projection,
                    WorldViewport {
                        width: viewport[0],
                        height: viewport[1],
                        device_pixel_ratio: 1.0,
                    },
                    draw.time,
                )
                .map_err(|_| MetalRenderError::InvalidCamera)?;
            for plane in frame.iter() {
                let model = plane.model;
                let projection = CanvasProjection::Plane {
                    matrix: ipp_core::math::multiply(camera.view_projection, model),
                    world: Some(model),
                };
                let sources = PaintSources::Plot {
                    publication: plane.publication,
                    paints: [plane.paint, None],
                };
                let placement = CanvasPlacement::rectangle(plane.plane.clip);
                let mut pass = CanvasPass {
                    frame: draw.frame,
                    summary: draw.summary,
                    projection: &projection,
                    owner: 0,
                };
                for primitive in plane.plane.primitives.iter() {
                    pass.primitive(
                        &sources,
                        &placement,
                        None,
                        primitive,
                        CanvasAssets {
                            host,
                            device: &self.device,
                            paths: &mut self.paths.borrow_mut(),
                            web_images: &self.web_images,
                        },
                    );
                }
            }
        }

        let mut order: Vec<_> = (0..transparency.len()).collect();
        order.sort_unstable_by(|&left, &right| {
            let a = &transparency[left];
            let b = &transparency[right];
            b.0.total_cmp(&a.0)
                .then_with(|| (a.1, a.2).cmp(&(b.1, b.2)))
                .then(left.cmp(&right))
        });
        let mut surfaces = surfaces.into_iter().peekable();
        let mut meshes = order
            .into_iter()
            .map(|index| &transparency[index])
            .peekable();
        self.instances.clear();
        let mut pending: Option<&MeshBindings<'_>> = None;
        while surfaces.peek().is_some() || meshes.peek().is_some() {
            if let Some(visit) = surfaces.next_if(|surface| {
                meshes.peek().is_none_or(|mesh| {
                    let depth = surface_depth(camera.view_projection, surface.model);
                    depth
                        .total_cmp(&mesh.0)
                        .reverse()
                        .then_with(|| surface.entity.cmp(&mesh.1))
                        .is_le()
                })
            }) {
                if let Some(bindings) = pending.take() {
                    bindings.submit(draw.frame, &self.instances, draw.summary, Some(&lighting));
                    self.instances.clear();
                }
                let selected =
                    if lights.over_capacity() && visit.child.output.kind() == OutputKind::Canvas {
                        Some(surface_lights(
                            &lights,
                            output,
                            &visit,
                            &mut self.light_selections,
                        ))
                    } else {
                        None
                    };
                if let Some(selected) = &selected {
                    draw.frame.lighting(selected);
                }
                self.draw_surface(host, visit, camera.view_projection, draw)?;
                if selected.is_some() {
                    draw.frame.lighting(&lighting);
                }
                continue;
            }

            let (_, _, _, bindings, material) = meshes.next().expect("pending blended mesh");
            if let Some(previous) = pending
                && (!previous.matches(bindings) || self.instances.len() == Material::MAX_INSTANCES)
            {
                previous.submit(draw.frame, &self.instances, draw.summary, Some(&lighting));
                self.instances.clear();
            }
            pending = Some(bindings);
            self.instances.push(*material);
        }
        if let Some(bindings) = pending {
            bindings.submit(draw.frame, &self.instances, draw.summary, Some(&lighting));
        }
        draw.included.insert(output, publication);
        Ok(())
    }

    fn shadow_pass(
        &mut self,
        host: &HostRuntime,
        source: OutputSource,
        lights: &LightCatalog,
        draw: &mut DrawResources<'_>,
    ) -> Result<Option<Rc<Object>>, MetalRenderError> {
        let grid = lights.shadow_grid();
        if grid == 0 {
            return Ok(None);
        }
        let size = grid * SHADOW_TILE;
        let atlas = self
            .shadows
            .get_mut(&source.output)
            .filter(|(extent, _, _)| *extent == size)
            .map(|(_, frame, atlas)| {
                *frame = self.frame_number;
                atlas.clone()
            });
        let atlas = match atlas {
            Some(atlas) => atlas,
            None => {
                let atlas = Rc::new(self.device.shadow_texture(size).ok_or_else(|| {
                    MetalRenderError::Device("could not allocate spotlight shadow atlas".into())
                })?);
                self.shadows
                    .insert(source.output, (size, self.frame_number, atlas.clone()));
                atlas
            }
        };

        if !draw.frame.shadow(&atlas) {
            return Err(MetalRenderError::Device(
                "could not encode shadow atlas".into(),
            ));
        }
        let mut has_custom_inputs = false;
        for (tile, view) in lights.shadow_views() {
            draw.frame.shadow_tile(
                tile % grid * SHADOW_TILE,
                tile / grid * SHADOW_TILE,
                SHADOW_TILE,
            );
            self.traversal
                .reset(host, source.publication, Some(source.output))
                .map_err(|_| MetalRenderError::InvalidCamera)?;
            while let Some(visit) = self.traversal.next(host) {
                let Some(scene) = visit
                    .publication
                    .chunk(RenderSystem::ID)
                    .and_then(|chunk| chunk.data::<RenderPublication>())
                else {
                    continue;
                };
                let placement = visit
                    .placement
                    .render_matrix()
                    .map_err(|_| MetalRenderError::InvalidCamera)?;
                for item in &scene.items {
                    if item.item.particle.is_some_and(|particle| particle.sprite) {
                        continue;
                    }
                    let custom = item.custom.as_ref().and_then(|material| {
                        let mesh =
                            asset::<MetalMesh>(host, visit.publication.id, item.item.mesh.asset)?;
                        CustomDraw::resolve(
                            host,
                            visit.publication.id,
                            material,
                            mesh.metadata.attributes(),
                        )
                    });
                    let casts = custom.as_ref().map_or_else(
                        || item.item.pbr.is_some_and(|pbr| pbr.cast_shadows),
                        |custom| custom.casts_shadows,
                    );
                    if !casts {
                        continue;
                    }
                    let Some((bindings, mut material)) = mesh_draw(
                        host,
                        visit.publication.id,
                        view,
                        placement,
                        &item.item,
                        item.palette.as_deref(),
                    ) else {
                        continue;
                    };
                    if let Some(custom) = &custom {
                        custom.configure(&mut material);
                        if !has_custom_inputs {
                            draw.frame.lighting(&Lighting::default());
                            has_custom_inputs = true;
                        }
                    }
                    if !draw.frame.draw(
                        MeshStreams {
                            mesh: bindings.mesh,
                            pose: bindings.pose,
                            joints: bindings.palette,
                        },
                        None,
                        &[material],
                        Blend::Opaque,
                        custom.as_ref().map(CustomDraw::parameters),
                    ) {
                        draw.summary.fail();
                    }
                }
            }
        }
        Ok(Some(atlas))
    }

    fn prepare_canvas_image(
        &mut self,
        host: &HostRuntime,
        visit: &SurfaceVisit<'_>,
        view: SurfaceView<'_>,
        draw: &mut DrawResources<'_>,
    ) -> Result<(), MetalRenderError> {
        let SurfaceView {
            projection: view,
            camera,
            viewport,
            lighting,
        } = view;
        if visit.child.output.kind() != OutputKind::Canvas {
            return Ok(());
        }
        let Some(geometry) = &visit.edge.surface_geometry else {
            return Ok(());
        };
        let scene = CanvasScene::new(host, visit.child.output, visit.child.publication)
            .ok_or(MetalRenderError::InvalidCanvas)?;
        let affine = geometry.exact_affine(0.0).is_some();
        let layered = visit.edge.layer_spacing != 0.0 && scene.canvas.layers.len() > 1;
        let policy = visit.edge.surface_cache_policy;
        let current_image = !affine && (layered || policy.is_some());
        if !current_image && policy.is_none() {
            return Ok(());
        }
        let content = CanvasContent::leaf(host, scene);
        let automatic = SurfaceCachePolicy {
            direct_distance: 100_000.0,
            resolution_scale: 1.0,
            allocation_priority: 1.0,
            max_refresh_hz: 240.0,
            max_texture_size: 0,
            force_cache: false,
        };
        let policy = policy.unwrap_or(automatic);
        let physical = geometry.physical_extent();
        let key = DrawResources::image_key(visit.child.output, physical);
        if draw.images.contains_key(&key) {
            return Ok(());
        }

        let distance = (visit.model[12] - camera[12])
            .hypot(visit.model[13] - camera[13])
            .hypot(visit.model[14] - camera[14]);
        let cached = self.cached_canvases.get(&visit.child.output);
        let band = cached.map_or_else(
            || policy.initial_band(distance),
            |cached| policy.band(distance, cached.band),
        );
        if (affine && band == 0) || (affine && content.is_none() && !layered) {
            return Ok(());
        }

        let matrix = ipp_core::math::multiply(view, visit.model);
        let Some(size) = canvas_images::projected_size(geometry, matrix, viewport, policy) else {
            return Ok(());
        };
        let pixels = u64::from(size[0])
            * u64::from(size[1])
            * if layered {
                2
            } else {
                1
            };
        if draw.target_pixels + pixels > 16 * 1024 * 1024 {
            return Ok(());
        }

        let reusable = cached
            .filter(|cached| cached.target.size() == size && cached.target.layered() == layered);
        let target = reusable
            .map(|cached| cached.target.clone())
            .or_else(|| self.device.target(size, layered).map(Rc::new));
        let Some(target) = target else {
            return Ok(());
        };

        let unchanged =
            content.is_some() && reusable.is_some_and(|cached| cached.content == content);
        let throttled = reusable.is_some_and(|cached| {
            content.is_some()
                && !layered
                && draw.time >= cached.time
                && draw.time - cached.time < policy.refresh_interval_at(band)
        });
        draw.target_pixels += pixels;

        let (publication, content, time, included, damage) =
            if let Some(cached) = reusable.filter(|_| unchanged || throttled) {
                let publication = if unchanged {
                    visit.child.publication
                } else {
                    cached.publication
                };
                (
                    publication,
                    cached.content,
                    cached.time,
                    BTreeMap::from([(visit.child.output, publication)]),
                    cached.damage.clone(),
                )
            } else {
                let failures = draw.summary.failed_draw_calls;
                let previous_damage = reusable.and_then(|image| image.damage.as_ref());
                let damage = (content.is_some() && !layered)
                    .then(|| CanvasDamage::capture(host, scene, size, previous_damage));
                let region = damage
                    .as_ref()
                    .and_then(|next| next.region(previous_damage).map(|region| (next, region)));
                #[cfg(feature = "instrumentation")]
                let _damage_trace = region
                    .map(|_| ipp_core::profiling::CpuTraceScope::new("MetalCanvasDamage.repaint"));
                let prepared = if let Some((_, region)) = region {
                    draw.frame.repaint_canvas(&target, lighting, region)
                } else {
                    draw.frame
                        .pass(Some(&target), [0.0; 4], Some(lighting), None)
                };
                if !prepared {
                    return Err(MetalRenderError::Device(
                        "could not encode cached Canvas pass".into(),
                    ));
                }
                let previous = std::mem::take(&mut draw.included);
                let extent = scene.canvas.logical_extent;
                let CanvasProjection::Plane {
                    matrix: image_matrix,
                    ..
                } = CanvasProjection::viewport(extent)
                else {
                    unreachable!()
                };
                let scale = std::array::from_fn(|i| physical[i] / f64::from(extent[i]));
                let scene_projection = surface_projection(view, visit.model, geometry, scale, 0.0);
                let projection = match scene_projection {
                    CanvasProjection::Plane {
                        world,
                        ..
                    } => CanvasProjection::Plane {
                        matrix: image_matrix,
                        world,
                    },
                    CanvasProjection::Surface {
                        matrix,
                        world,
                        geometry,
                        scale,
                        offset,
                        ..
                    } => CanvasProjection::Surface {
                        matrix,
                        world,
                        geometry,
                        scale,
                        offset,
                        image_projection: Some(image_matrix),
                    },
                    CanvasProjection::Analytic(_) => unreachable!(),
                };
                let placement = CanvasPlacement::root(extent);
                if let Some((damage, region)) = region {
                    self.draw_canvas_primitives(
                        host,
                        scene,
                        &placement,
                        &projection,
                        draw,
                        damage.entries(region),
                    );
                    draw.included.insert(scene.canvas.selection, scene.world.id);
                } else {
                    self.draw_canvas(host, scene, &placement, &projection, None, draw, 0, None);
                }
                let included = std::mem::replace(&mut draw.included, previous);
                if failures != draw.summary.failed_draw_calls {
                    self.cached_canvases.remove(&visit.child.output);
                    return Ok(());
                }
                (
                    visit.child.publication,
                    content,
                    draw.time,
                    included,
                    damage,
                )
            };
        self.cached_canvases.insert(
            visit.child.output,
            CachedCanvas {
                target: target.clone(),
                content,
                damage,
                publication,
                frame: self.frame_number,
                time,
                band,
            },
        );
        draw.images.insert(
            key,
            PreparedImage {
                target,
                included,
            },
        );
        Ok(())
    }

    fn draw_surface(
        &self,
        host: &HostRuntime,
        visit: SurfaceVisit<'_>,
        view_projection: [f32; 16],
        draw: &mut DrawResources<'_>,
    ) -> Result<(), MetalRenderError> {
        let Some(geometry) = &visit.edge.surface_geometry else {
            return Ok(());
        };
        let physical = geometry.physical_extent();
        let model = visit.model;
        let key = DrawResources::image_key(visit.child.output, physical);
        if let Some(image) = draw.images.get(&key) {
            let extent = physical.map(|v| v as f32);
            let placement = CanvasPlacement::root(extent);
            let before = draw.summary.draw_calls;
            if image.target.layered() {
                let scene = CanvasScene::new(host, visit.child.output, visit.child.publication)
                    .ok_or(MetalRenderError::InvalidCanvas)?;
                for (id, offset) in image_layer_order(
                    scene,
                    geometry,
                    view_projection,
                    model,
                    visit.edge.layer_spacing,
                ) {
                    let projection =
                        surface_projection(view_projection, model, geometry, [1.0; 2], offset);
                    let mut pass = CanvasPass {
                        frame: draw.frame,
                        summary: draw.summary,
                        projection: &projection,
                        owner: 0,
                    };
                    pass.draw_image(
                        &placement,
                        CanvasBinding::Layer {
                            target: &image.target,
                            id,
                        },
                        extent,
                    );
                }
            } else {
                let projection =
                    surface_projection(view_projection, model, geometry, [1.0; 2], 0.0);
                let mut pass = CanvasPass {
                    frame: draw.frame,
                    summary: draw.summary,
                    projection: &projection,
                    owner: 0,
                };
                pass.draw_image(&placement, CanvasBinding::Target(&image.target), extent);
            }
            if draw.summary.draw_calls > before {
                draw.included.extend(
                    image
                        .included
                        .iter()
                        .map(|(&output, &publication)| (output, publication)),
                );
            }
            return Ok(());
        }

        match visit.child.output.kind() {
            OutputKind::Canvas => {
                let scene = CanvasScene::new(host, visit.child.output, visit.child.publication)
                    .ok_or(MetalRenderError::InvalidCanvas)?;
                let extent = scene.canvas.logical_extent;
                let scale = std::array::from_fn(|i| physical[i] / f64::from(extent[i]));
                let placement = CanvasPlacement::root(extent);
                let spacing = f64::from(visit.edge.layer_spacing);
                let projection =
                    |offset| surface_projection(view_projection, model, geometry, scale, offset);
                if let Some(mut analytic) = visit.analytic {
                    analytic.set_scale(scale);
                    let roots = if analytic.needs_near_root() {
                        &[false, true][..]
                    } else {
                        &[false][..]
                    };
                    for &near in roots {
                        analytic.select_root(near);
                        if let Some(start) = visit.column_start {
                            analytic.use_columns(start);
                        }
                        self.draw_canvas(
                            host,
                            scene,
                            &placement,
                            &CanvasProjection::Analytic(&analytic),
                            None,
                            draw,
                            0,
                            None,
                        );
                    }
                    return Ok(());
                }
                if spacing == 0.0 {
                    self.draw_canvas(
                        host,
                        scene,
                        &placement,
                        &projection(0.0),
                        None,
                        draw,
                        0,
                        None,
                    );
                } else {
                    for (layer, offset) in image_layer_order(
                        scene,
                        geometry,
                        view_projection,
                        model,
                        visit.edge.layer_spacing,
                    ) {
                        self.draw_canvas(
                            host,
                            scene,
                            &placement,
                            &projection(offset),
                            Some(layer),
                            draw,
                            0,
                            None,
                        );
                    }
                }
            }
            OutputKind::Camera => {}
        }
        Ok(())
    }

    fn draw_retained_canvas(
        &self,
        host: &HostRuntime,
        scene: CanvasScene<'_>,
        placement: &CanvasPlacement,
        projection: &CanvasProjection<'_>,
        draw: &mut DrawResources<'_>,
    ) -> bool {
        let entries = &scene.canvas.entries;
        if entries.len() < 8
            || entries.iter().any(|entry| {
                !matches!(entry.as_ref(), CanvasPaintEntry::Primitive { primitive, .. }
                if !matches!(primitive, CanvasPrimitive::Html { .. }))
            })
        {
            return false;
        }

        let Some(key) = draw
            .frame
            .presentation_extent()
            .and_then(|size| RunKey::new(projection, size, scene.canvas))
        else {
            return false;
        };

        let mut batches = self.canvas_batches.borrow_mut();
        for (index, run) in entries.chunks(PRIMITIVES_PER_RUN).enumerate() {
            let start = index * PRIMITIVES_PER_RUN;
            let lookup = batches.lookup(
                scene.canvas,
                key,
                start..start + run.len(),
                draw.frame,
                draw.summary,
            );
            if matches!(lookup, Lookup::Replayed) {
                continue;
            }

            let before = (
                draw.summary.draw_calls,
                draw.summary.triangles,
                draw.summary.failed_draw_calls,
            );
            self.draw_canvas_primitives(
                host,
                scene,
                placement,
                projection,
                draw,
                run.iter().map(AsRef::as_ref),
            );
            if let Lookup::Record(id) = lookup {
                let counts = (before.2 == draw.summary.failed_draw_calls).then(|| {
                    (
                        draw.summary.draw_calls - before.0,
                        draw.summary.triangles - before.1,
                    )
                });
                batches.finish(id, draw.frame, counts);
            }
        }

        true
    }

    fn draw_canvas_primitives<'a>(
        &self,
        host: &HostRuntime,
        scene: CanvasScene<'_>,
        placement: &CanvasPlacement,
        projection: &CanvasProjection<'_>,
        draw: &mut DrawResources<'_>,
        entries: impl Iterator<Item = &'a CanvasPaintEntry>,
    ) {
        let mut paths = self.paths.borrow_mut();
        for entry in entries {
            let CanvasPaintEntry::Primitive {
                space,
                primitive,
                ..
            } = entry
            else {
                unreachable!("only leaf Canvas entries use this path");
            };
            let mut pass = CanvasPass {
                frame: draw.frame,
                summary: draw.summary,
                projection,
                owner: entry.layer().saturating_add(1),
            };
            pass.primitive(
                &PaintSources::Canvas(scene),
                placement,
                space.as_deref(),
                primitive,
                CanvasAssets {
                    host,
                    device: &self.device,
                    paths: &mut paths,
                    web_images: &self.web_images,
                },
            );
        }
    }

    #[allow(clippy::too_many_arguments, clippy::only_used_in_recursion)]
    fn draw_canvas(
        &self,
        host: &HostRuntime,
        scene: CanvasScene<'_>,
        placement: &CanvasPlacement,
        projection: &CanvasProjection<'_>,
        layer: Option<u32>,
        draw: &mut DrawResources<'_>,
        depth: u32,
        inherited_owner: Option<u32>,
    ) {
        if depth >= 64 {
            draw.summary.fail();
            return;
        }
        if depth == 0
            && layer.is_none()
            && inherited_owner.is_none()
            && self.draw_retained_canvas(host, scene, placement, projection, draw)
        {
            draw.included.insert(scene.canvas.selection, scene.world.id);
            return;
        }

        for entry in scene.canvas.entries.iter() {
            if layer.is_some_and(|layer| entry.layer() != layer) {
                continue;
            }
            let owner = inherited_owner.unwrap_or_else(|| entry.layer());
            let owner_id = owner.saturating_add(1);
            match entry.as_ref() {
                CanvasPaintEntry::Primitive {
                    space,
                    primitive,
                    ..
                } => {
                    let mut pass = CanvasPass {
                        frame: draw.frame,
                        summary: draw.summary,
                        projection,
                        owner: owner_id,
                    };
                    pass.primitive(
                        &PaintSources::Canvas(scene),
                        placement,
                        space.as_deref(),
                        primitive,
                        CanvasAssets {
                            host,
                            device: &self.device,
                            paths: &mut self.paths.borrow_mut(),
                            web_images: &self.web_images,
                        },
                    );
                }
                CanvasPaintEntry::Attachment(slot) => {
                    let Some((output, publication)) = scene.child(host, slot) else {
                        continue;
                    };
                    match output.kind() {
                        OutputKind::Canvas => {
                            if let Some(child) = CanvasScene::new(host, output, publication)
                                && let Some(placement) =
                                    placement.slot(slot, child.canvas.logical_extent)
                            {
                                self.draw_canvas(
                                    host,
                                    child,
                                    &placement,
                                    projection,
                                    None,
                                    draw,
                                    depth + 1,
                                    Some(owner),
                                );
                            }
                        }
                        OutputKind::Camera => {
                            let extent = slot.physical_extent.map(|v| v as f32);
                            if let Some(placement) = placement.slot(slot, extent)
                                && let Some(image) = draw
                                    .images
                                    .get(&DrawResources::image_key(output, slot.physical_extent))
                            {
                                let mut pass = CanvasPass {
                                    frame: draw.frame,
                                    summary: draw.summary,
                                    projection,
                                    owner: owner_id,
                                };
                                let before = pass.summary.draw_calls;
                                pass.draw_image(
                                    &placement,
                                    CanvasBinding::Target(&image.target),
                                    extent,
                                );
                                if pass.summary.draw_calls > before {
                                    draw.included.extend(
                                        image
                                            .included
                                            .iter()
                                            .map(|(&output, &publication)| (output, publication)),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        draw.included.insert(scene.canvas.selection, scene.world.id);
    }
}

fn image_layer_order(
    scene: CanvasScene<'_>,
    geometry: &ipp_core::systems::surface::SurfaceGeometry,
    view: [f32; 16],
    model: [f32; 16],
    spacing: f32,
) -> impl Iterator<Item = (u32, f64)> {
    let clip = ipp_core::math::multiply(view, model);
    let center = geometry.physical_extent().map(|value| value * 0.5);
    let mut layers: Vec<_> = scene
        .canvas
        .layers
        .iter()
        .filter_map(|layer| {
            let offset = layer.offset * f64::from(spacing);
            let sample = geometry.sample(center, offset).ok()?;
            let z = f64::from(clip[14])
                + (0..3)
                    .map(|i| f64::from(clip[i * 4 + 2]) * sample.position[i])
                    .sum::<f64>();
            Some((layer.id, offset, z))
        })
        .collect();
    layers.sort_by(|a, b| b.2.total_cmp(&a.2));
    layers.into_iter().map(|(layer, offset, _)| (layer, offset))
}

fn submit_generated(
    draw: &mut DrawResources<'_>,
    mesh: &crate::derived_meshes::DerivedMesh,
    model: [f32; 16],
    view: [f32; 16],
    color: [f32; 4],
    custom: Option<CustomDraw<'_>>,
) {
    let mut material = Material {
        clip_from_mesh: ipp_core::math::multiply(view, model),
        world_from_mesh: model,
        color,
        options: [0.0, mesh.vertex_mode, 0.0, 0.0],
        surface: [0.0; 4],
        effects: [0.0; 4],
    };
    let blend = if let Some(custom) = &custom {
        custom.configure(&mut material);
        material.surface[3] = 1.0;
        custom.blend
    } else if color[3] < 1.0 {
        Blend::Alpha
    } else {
        Blend::Opaque
    };
    let bindings = MeshBindings {
        mesh: &mesh.buffers,
        pose: None,
        texture: None,
        palette: std::slice::from_ref(&IDENTITY),
        triangles: mesh.triangles,
        blend,
        custom,
        selected_lights: None,
    };
    bindings.submit(draw.frame, &[material], draw.summary, None);
}

fn fullscreen_camera(
    host: &HostRuntime,
    scene: CanvasScene<'_>,
) -> Option<(OutputSource, [f64; 2])> {
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
    Some((
        OutputSource {
            output,
            publication,
        },
        slot.physical_extent,
    ))
}

fn surface_projection(
    view: [f32; 16],
    world: [f32; 16],
    geometry: &ipp_core::systems::surface::SurfaceGeometry,
    scale: [f64; 2],
    offset: f64,
) -> CanvasProjection<'_> {
    if let Some(local) = geometry.exact_affine(offset) {
        let local = ipp_core::math::multiply(
            local.map(|v| v as f32),
            ipp_core::systems::canvas::CanvasAffine::axis_aligned(
                [0.0; 2],
                scale.map(|v| v as f32),
            )
            .matrix(),
        );
        let pose = ipp_core::math::multiply(world, local);
        CanvasProjection::Plane {
            matrix: ipp_core::math::multiply(view, pose),
            world: Some(pose),
        }
    } else {
        let matrix = ipp_core::math::multiply(view, world);
        CanvasProjection::Surface {
            matrix,
            world,
            geometry,
            scale,
            offset,
            image_projection: None,
        }
    }
}

pub(super) fn asset<T: 'static>(
    host: &HostRuntime,
    publication: WorldPublicationId,
    key: u64,
) -> Option<&T> {
    host.publication_resource(publication, AssetKey::from_u64(key))?
        .data()?
        .as_any()
        .downcast_ref()
}

struct MeshBindings<'a> {
    mesh: &'a MeshBuffers,
    pose: Option<&'a MeshBuffers>,
    texture: Option<&'a Object>,
    palette: &'a [[f32; 16]],
    triangles: u32,
    blend: Blend,
    custom: Option<CustomDraw<'a>>,
    selected_lights: Option<Box<Lighting>>,
}

impl MeshBindings<'_> {
    fn matches(&self, other: &Self) -> bool {
        std::ptr::eq(self.mesh, other.mesh)
            && self.pose.map(std::ptr::from_ref) == other.pose.map(std::ptr::from_ref)
            && self.texture.map(std::ptr::from_ref) == other.texture.map(std::ptr::from_ref)
            && std::ptr::eq(self.palette, other.palette)
            && self.blend == other.blend
            && self.selected_lights == other.selected_lights
            && match (&self.custom, &other.custom) {
                (None, None) => true,
                (Some(left), Some(right)) => left.matches(right),
                _ => false,
            }
    }

    fn submit(
        &self,
        frame: &mut Frame,
        instances: &[Material],
        summary: &mut MetalFrameSummary,
        default_lights: Option<&Lighting>,
    ) {
        if let Some(lighting) = &self.selected_lights {
            frame.lighting(lighting);
        }
        let instance_count = instances.len() as u32;
        if frame.draw(
            MeshStreams {
                mesh: self.mesh,
                pose: self.pose,
                joints: self.palette,
            },
            self.texture,
            instances,
            self.blend,
            self.custom.as_ref().map(CustomDraw::parameters),
        ) {
            summary.draw_calls = summary.draw_calls.saturating_add(1);
            summary.triangles = summary
                .triangles
                .saturating_add(self.triangles.saturating_mul(instance_count));
        } else {
            summary.failed_draw_calls = summary.failed_draw_calls.saturating_add(instance_count);
        }

        if self.selected_lights.is_some()
            && let Some(lighting) = default_lights
        {
            frame.lighting(lighting);
        }
    }
}

fn surface_depth(view: [f32; 16], model: [f32; 16]) -> f32 {
    view[14] + (0..3).map(|i| view[4 * i + 2] * model[12 + i]).sum::<f32>()
}

fn surface_lights(
    lights: &LightCatalog,
    output: OutputRef,
    surface: &SurfaceVisit<'_>,
    history: &mut LightSelections,
) -> Lighting {
    let bounds = surface.edge.surface_geometry.as_ref().and_then(|geometry| {
        match geometry.bounds([0.0; 2]).ok()? {
            ipp_core::systems::geometry::GeometryShape::Box {
                min,
                max,
            } => Some((min.map(|value| value as f32), max.map(|value| value as f32))),
            _ => None,
        }
    });
    let (center, radius) = light_receiver(surface.model, bounds);
    lights.select(output, surface.entity, center, radius, history)
}

fn light_receiver(model: [f32; 16], bounds: Option<([f32; 3], [f32; 3])>) -> ([f32; 3], f32) {
    let (min, max) = bounds.unwrap_or(([0.0; 3], [0.0; 3]));
    let center: [f32; 3] = std::array::from_fn(|i| min[i].midpoint(max[i]));
    let half: [f32; 3] = std::array::from_fn(|i| (max[i] - min[i]) * 0.5);
    let position = std::array::from_fn(|i| {
        model[12 + i] + model[i] * center[0] + model[4 + i] * center[1] + model[8 + i] * center[2]
    });
    let extent: [f32; 3] = std::array::from_fn(|i| {
        model[i].abs() * half[0] + model[4 + i].abs() * half[1] + model[8 + i].abs() * half[2]
    });
    (position, extent[0].hypot(extent[1]).hypot(extent[2]))
}

fn mesh_draw<'a>(
    host: &'a HostRuntime,
    publication: WorldPublicationId,
    view: [f32; 16],
    placement: [f32; 16],
    draw: &RenderItem,
    palette: Option<&'a [[f32; 16]]>,
) -> Option<(MeshBindings<'a>, Material)> {
    let mesh = asset::<MetalMesh>(host, publication, draw.mesh.asset)?;
    let buffers = mesh.buffers.as_ref()?;
    let (target, weight) = if let Some((key, weight)) = draw.pose {
        let target = asset::<MetalMesh>(host, publication, key.asset)?;
        if target.metadata.vertex_count() != mesh.metadata.vertex_count() {
            return None;
        }
        (Some(target.buffers.as_ref()?), weight)
    } else {
        (None, 0.0)
    };
    let palette = if draw.skinned {
        let palette = palette?;
        let maximum = usize::from(mesh.metadata.max_joint_index()?);
        if maximum >= palette.len() {
            return None;
        }
        palette
    } else {
        std::slice::from_ref(&IDENTITY)
    };
    let texture = match draw.texture {
        Some(key) => Some(
            asset::<MetalTexture>(host, publication, key.asset)?
                .texture
                .as_ref()?,
        ),
        None => None,
    };
    let model = ipp_core::math::multiply(placement, draw.model);
    let material = Material {
        clip_from_mesh: ipp_core::math::multiply(view, model),
        world_from_mesh: model,
        color: [draw.material.r, draw.material.g, draw.material.b, 1.0],
        options: [
            weight,
            u8::from(draw.skinned) as f32,
            u8::from(draw.texture_weights) as f32,
            u8::from(draw.solid_fallback) as f32,
        ],
        effects: [
            f32::from(draw.pbr.is_some_and(|pbr| pbr.receive_shadows)),
            0.0,
            0.0,
            f32::from(texture.is_some()),
        ],
        surface: draw.pbr.map_or([0.0; 4], |material| {
            [
                material.metallic,
                material.roughness,
                1.0,
                f32::from(draw.normals),
            ]
        }),
    };
    let bindings = MeshBindings {
        mesh: buffers,
        pose: target,
        texture,
        palette,
        triangles: (mesh.metadata.index_count() / 3) as u32,
        blend: Blend::Opaque,
        custom: None,
        selected_lights: None,
    };
    Some((bindings, material))
}

fn sprite_draw<'a>(
    host: &'a HostRuntime,
    publication: WorldPublicationId,
    view: [f32; 16],
    camera: [f32; 16],
    placement: [f32; 16],
    item: &RenderItem,
    quad: &'a MeshBuffers,
) -> Option<(MeshBindings<'a>, Material)> {
    let particle = item.particle?;
    let model = particles::billboard(
        item,
        camera,
        placement,
        ipp_core::math::multiply(placement, item.model),
    )?;
    let texture = match item.texture {
        Some(texture) => Some(
            asset::<MetalTexture>(host, publication, texture.asset)?
                .texture
                .as_ref()?,
        ),
        None => None,
    };
    let material = Material {
        clip_from_mesh: ipp_core::math::multiply(view, model),
        world_from_mesh: model,
        color: [
            item.material.r,
            item.material.g,
            item.material.b,
            -particle.opacity,
        ],
        options: [0.0; 4],
        surface: [0.0; 4],
        effects: [0.0, 0.0, 0.0, f32::from(texture.is_some())],
    };
    let bindings = MeshBindings {
        mesh: quad,
        pose: None,
        texture,
        palette: std::slice::from_ref(&IDENTITY),
        triangles: 2,
        blend: if particle.additive {
            Blend::Additive
        } else {
            Blend::Alpha
        },
        custom: None,
        selected_lights: None,
    };
    Some((bindings, material))
}
