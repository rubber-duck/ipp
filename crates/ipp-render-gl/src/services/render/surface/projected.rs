//! Required content images and sampled Surface geometry, independent of temporal caching.
//!
//! Images use the ordinary Canvas rasterizer. Layer cutouts share one flattened
//! color image and a derived ownership attachment. Geometry and
//! placement are absent from image stamps. A stable requested quality keeps the
//! selected image size while the provider's half-texel approximation still fits.
//! Otherwise image dimensions halve until the bounded mesh fits, never changing
//! the provider mapping. Canvas and SurfaceCamera color capacities share the optional
//! cache's context-wide budget; root presentation targets remain Host-owned.
//! sampled mesh storage additionally stays below 16 MiB. Exhaustion is unavailable.

use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

use super::super::{
    assets::loaders::GlMeshData,
    canvas::scene::{CanvasScene, OutputContentStamp},
    frame::{
        draw_order::RenderMaterialKey,
        scratch::{RenderDraw, RenderDrawIndex},
    },
    outputs::scene::SceneOutputSurface,
    statistics::RenderFrameWork,
};
use super::{
    mesh::{SurfaceMeshPatch, select_quality},
    texture_cache::{
        SURFACE_CACHE_MAX_DIMENSION, SurfaceCacheDiagnostic, SurfaceCachePresentation,
    },
};
use crate::{RenderDevice, RenderError, RenderService};
use ipp_core::{HostRuntime, OutputKind, OutputRef, WorldViewport};

const MESH_BUDGET: usize = 16 << 20;

struct ProjectedGroup<D: RenderDevice> {
    plane: Option<u32>,
    offset: f64,
    painter_order: usize,
    mesh: GlMeshData<D>,
    patches: Vec<SurfaceMeshPatch>,
    bytes: usize,
}

pub(in crate::services::render) struct ProjectedSurface<D: RenderDevice> {
    device: Rc<RefCell<D>>,
    target: Option<D::SurfaceCacheTarget>,
    ownership: bool,
    damage: Option<super::damage::CanvasImageDamage>,
    surface: SceneOutputSurface,
    groups: Vec<ProjectedGroup<D>>,
    /// Ordered image membership, independent of positions and refresh timing.
    requested: [u32; 2],
    budget_limited: bool,
    quality_demand: [f64; 2],
    quality_view: ([f32; 16], WorldViewport),
    size: [u32; 2],
    capacity: [u32; 2],
    band: u8,
    stamp: Option<OutputContentStamp>,
    painted_outputs: BTreeSet<OutputRef>,
    represented_images: BTreeSet<OutputRef>,
    current_image: bool,
    /// Whether the usable image included current child publications when painted.
    painted_current: bool,
    /// Missing primitive submissions remain diagnostic while their image is reused.
    missing_draws: u32,
    raster_dependencies: Vec<(OutputRef, u64)>,
    current_publication: bool,
    painted_at: f64,
    used_at: f64,
    repaints: u32,
    reuses: u32,
    allocations: u32,
    presentation: SurfaceCachePresentation,
    repaint_this_frame: bool,
    paint_context: super::super::canvas::presentation::PaintContextStamp,
}

impl<D: RenderDevice> Drop for ProjectedSurface<D> {
    fn drop(&mut self) {
        if let Some(target) = self.target.take() {
            self.device.borrow_mut().delete_surface_cache_target(target);
        }
    }
}

impl<D: RenderDevice> ProjectedSurface<D> {
    fn image_bytes(&self) -> usize {
        usize::from(self.target.is_some())
            * (1 + usize::from(self.ownership))
            * self.capacity[0] as usize
            * self.capacity[1] as usize
            * 4
    }

    fn mesh_bytes(&self) -> usize {
        self.groups.iter().map(|g| g.bytes).sum()
    }

    fn diagnostic(&self) -> SurfaceCacheDiagnostic {
        SurfaceCacheDiagnostic {
            entity: self.surface.entity.entity,
            presentation: self.presentation,
            band: self.band,
            size: if self.groups.is_empty()
                || (self.surface.selection.kind() == OutputKind::Canvas && self.image_bytes() == 0)
            {
                [0; 2]
            } else {
                self.size
            },
            capacity: if self.groups.is_empty()
                || (self.surface.selection.kind() == OutputKind::Canvas && self.image_bytes() == 0)
            {
                [0; 2]
            } else {
                self.capacity
            },
            repaints: self.repaints,
            reuses: self.reuses,
            painted_at: self.painted_at,
            resident_bytes: if self.surface.selection.kind() == OutputKind::Camera
                && !self.groups.is_empty()
            {
                (self.size[0] as usize * self.size[1] as usize * 4).min(u32::MAX as usize) as u32
            } else {
                self.image_bytes().min(u32::MAX as usize) as u32
            },
        }
    }
}

impl<D: RenderDevice> RenderService<D> {
    pub(super) fn invalidate_projected_images(&mut self) {
        for state in self.projected_surfaces.values_mut() {
            state.stamp = None;
            state.current_image = false;
            state.painted_current = false;
        }
        self.camera_completed.clear();
    }

    pub(in crate::services::render) fn begin_projected_frame(
        &mut self,
        time: f64,
        visible: &BTreeSet<OutputRef>,
    ) {
        self.projected_visible.clone_from(visible);
        self.camera_used
            .retain(|output, _| self.camera_targets.contains_key(output));
        let idle: Vec<_> = self
            .camera_targets
            .keys()
            .copied()
            .filter(|output| {
                !visible.contains(output)
                    && time - self.camera_used.get(output).copied().unwrap_or(time)
                        >= super::texture_cache::SURFACE_CACHE_IDLE_SECONDS
            })
            .collect();
        for output in idle {
            if let Some((target, _, _)) = self.camera_targets.remove(&output) {
                self.device.borrow_mut().delete_surface_cache_target(target);
            }
            self.camera_used.remove(&output);
            self.camera_completed.remove(&output);
        }
        for (selection, state) in &mut self.projected_surfaces {
            if !visible.contains(selection)
                && time - state.used_at >= super::texture_cache::SURFACE_CACHE_IDLE_SECONDS
            {
                if let Some(target) = state.target.take() {
                    self.device.borrow_mut().delete_surface_cache_target(target);
                }
                state.stamp = None;
                state.current_image = false;
            }
            state.repaint_this_frame = false;
            state.allocations = 0;
            state.presentation = SurfaceCachePresentation::Culled;
        }
    }

    fn resident_output_bytes(&self, output: OutputRef) -> usize {
        if output.kind() == OutputKind::Camera {
            self.camera_targets
                .get(&output)
                .map_or(0, |(_, _, c)| c[0] as usize * c[1] as usize * 4)
        } else {
            self.projected_surfaces
                .get(&output)
                .map_or(0, ProjectedSurface::image_bytes)
        }
    }

    /// Each output may use its ranked reservation plus unreserved headroom.
    /// The resident/remaining-demand check still prevents sharing that headroom twice.
    pub(in crate::services::render) fn required_image_limit(&self, output: OutputRef) -> usize {
        let reserved: usize = self.required_image_demand.values().sum();
        self.required_image_demand
            .get(&output)
            .copied()
            .unwrap_or(0)
            + self.surface_cache.budget().saturating_sub(reserved)
    }

    /// Run before any output is painted, so dependencies never see an image
    /// released halfway through child-first composition. Retain mesh/publication
    /// identity; image contents will be reconstructed at the newly admitted tier.
    pub(in crate::services::render) fn reclaim_required_allocations(&mut self) {
        let outputs: Vec<_> = self.required_image_demand.keys().copied().collect();
        for output in outputs {
            let limit = self.required_image_limit(output);
            if output.kind() == OutputKind::Camera {
                if self
                    .camera_targets
                    .get(&output)
                    .is_some_and(|(_, size, _)| size[0] as usize * size[1] as usize * 4 > limit)
                {
                    let (target, _, _) = self
                        .camera_targets
                        .remove(&output)
                        .expect("selected target");
                    self.device.borrow_mut().delete_surface_cache_target(target);
                    self.camera_completed.remove(&output);
                }
            } else if let Some(state) = self.projected_surfaces.get_mut(&output)
                && (1 + usize::from(state.ownership))
                    * state.size[0] as usize
                    * state.size[1] as usize
                    * 4
                    > limit
            {
                if let Some(target) = state.target.take() {
                    self.device.borrow_mut().delete_surface_cache_target(target);
                }
                state.stamp = None;
                state.current_image = false;
                state.budget_limited = true;
            }
        }
    }

    /// Padding may use only bytes left after every other visible image's active
    /// demand, including images that have not reached allocation in this frame.
    pub(in crate::services::render) fn remaining_required_bytes(
        &self,
        current: OutputRef,
    ) -> usize {
        self.required_image_demand
            .iter()
            .filter(|(output, _)| **output != current)
            .map(|(output, bytes)| bytes.saturating_sub(self.resident_output_bytes(*output)))
            .sum()
    }

    pub(in crate::services::render) fn reclaim_required_padding(
        &mut self,
    ) -> Result<(), RenderError> {
        let shortfall: usize = self
            .required_image_demand
            .iter()
            .map(|(output, bytes)| bytes.saturating_sub(self.resident_output_bytes(*output)))
            .sum();
        if shortfall + self.projected_image_bytes() + self.surface_cache.resident().1 as usize
            <= self.surface_cache.budget()
        {
            return Ok(());
        }
        for (output, state) in &mut self.projected_surfaces {
            if self.projected_visible.contains(output) && state.capacity != state.size {
                {
                    if let Some(mut target) = state.target.take() {
                        let resized = self.device.borrow_mut().resize_surface_cache_target(
                            &mut target,
                            state.size[0],
                            state.size[1],
                        );
                        match resized {
                            Ok(()) => {
                                state.target = Some(target);
                                state.allocations = state.allocations.saturating_add(1);
                            }
                            Err(error) => {
                                self.device.borrow_mut().delete_surface_cache_target(target);
                                state.stamp = None;
                                state.current_image = false;
                                if error == RenderError::ContextLost {
                                    if let Some(target) = state.target.take() {
                                        self.device
                                            .borrow_mut()
                                            .delete_surface_cache_target(target);
                                    }
                                    return Err(error);
                                }
                            }
                        }
                    }
                }
                state.capacity = state.size;
                state.stamp = None;
                state.current_image = false;
            }
        }
        let trim: Vec<_> = self
            .camera_targets
            .iter()
            .filter(|(output, (_, active, capacity))| {
                self.projected_visible.contains(output) && capacity != active
            })
            .map(|(output, _)| *output)
            .collect();
        for output in trim {
            let (mut target, active, _) = self
                .camera_targets
                .remove(&output)
                .expect("selected Camera target");
            self.camera_completed.remove(&output);
            let resized = self.device.borrow_mut().resize_surface_cache_target(
                &mut target,
                active[0],
                active[1],
            );
            match resized {
                Ok(()) => {
                    self.camera_targets.insert(output, (target, active, active));
                }
                Err(error) => {
                    self.device.borrow_mut().delete_surface_cache_target(target);
                    self.camera_used.remove(&output);
                    if error == RenderError::ContextLost {
                        return Err(error);
                    }
                }
            }
        }
        Ok(())
    }

    /// Reclaim only outputs absent from this frame's prepared graph. Keeping
    /// visible targets alive protects descendant images referenced later in it.
    pub(in crate::services::render) fn reclaim_inactive_images(&mut self) {
        let shortfall: usize = self
            .required_image_demand
            .iter()
            .map(|(output, bytes)| bytes.saturating_sub(self.resident_output_bytes(*output)))
            .sum();
        if shortfall + self.projected_image_bytes() + self.surface_cache.resident().1 as usize
            <= self.surface_cache.budget()
        {
            return;
        }
        for (output, state) in &mut self.projected_surfaces {
            if !self.projected_visible.contains(output) {
                if let Some(target) = state.target.take() {
                    self.device.borrow_mut().delete_surface_cache_target(target);
                }
                state.stamp = None;
                state.current_image = false;
            }
        }
        let inactive: Vec<_> = self
            .camera_targets
            .keys()
            .copied()
            .filter(|output| !self.projected_visible.contains(output))
            .collect();
        for output in inactive {
            if let Some((target, _, _)) = self.camera_targets.remove(&output) {
                self.device.borrow_mut().delete_surface_cache_target(target);
            }
            self.camera_used.remove(&output);
            self.camera_completed.remove(&output);
        }
    }

    /// Plan nested images with the same bounds-based quality selection used for
    /// the parent. Mesh tessellation belongs only to draw preparation.
    pub(in crate::services::render) fn projected_raster_size(
        &self,
        host: &HostRuntime,
        surface: &SceneOutputSurface,
        mvp: [f32; 16],
        viewport: WorldViewport,
    ) -> Result<[u32; 2], RenderError> {
        let mut offsets = Vec::new();
        if surface.selection.kind() == OutputKind::Canvas && surface.layer_spacing != 0.0 {
            let scene = CanvasScene::new(host, surface.selection, surface.publication)?;
            let mut layers = BTreeSet::new();
            for entry in scene.canvas.entries.iter() {
                if layers.insert(entry.layer()) {
                    offsets.push(
                        scene
                            .canvas
                            .layer_offset(entry.layer())
                            .ok_or(RenderError::UnavailableOutput)?
                            * f64::from(surface.layer_spacing),
                    );
                }
            }
        }
        if offsets.is_empty() {
            offsets.push(0.0);
        }
        let previous = self
            .projected_surfaces
            .get(&surface.selection)
            .filter(|state| {
                state.surface.geometry == surface.geometry
                    && state.surface.token == surface.token
                    && state.surface.provider_incarnation == surface.provider_incarnation
                    && state
                        .groups
                        .iter()
                        .map(|g| g.offset)
                        .eq(offsets.iter().copied())
            });
        if let Some(state) = previous.filter(|state| {
            state.quality_view == (mvp, viewport)
                && state.surface.cache_policy == surface.cache_policy
                && state.size.iter().all(|v| {
                    *v <= self
                        .device
                        .borrow()
                        .surface_cache_limit()
                        .min(SURFACE_CACHE_MAX_DIMENSION)
                })
        }) {
            return Ok(std::array::from_fn(|i| {
                state.requested[i].max(state.size[i])
            }));
        }
        let limit = self
            .device
            .borrow()
            .surface_cache_limit()
            .min(SURFACE_CACHE_MAX_DIMENSION);
        let limit = surface
            .cache_policy
            .map_or(limit, |policy| policy.texture_limit(limit));
        let scale = surface.cache_policy.map_or(1.0, |p| p.resolution_scale);
        let demand = super::quality::surface_demand(&*surface.geometry, &offsets, mvp, viewport)?;
        super::quality::image_size(demand, scale, limit, previous.map(|s| s.requested))
            .ok_or(RenderError::UnavailableOutput)
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::services::render) fn prepare_projected_surface(
        &mut self,
        host: &HostRuntime,
        surface: SceneOutputSurface,
        mvp: [f32; 16],
        viewport: WorldViewport,
        time: f64,
        interaction: bool,
        stats: &mut RenderFrameWork,
    ) -> Result<(), RenderError> {
        let selection = surface.selection;
        let mut previous = self.projected_surfaces.remove(&selection);
        if previous.as_ref().is_some_and(|s| {
            s.surface.entity != surface.entity
                || s.surface.token != surface.token
                || s.surface.provider_incarnation != surface.provider_incarnation
                || s.surface.geometry.component() != surface.geometry.component()
        }) {
            self.release_projected(previous.take().expect("previous identity"));
        }
        let band = surface.cache_policy.map_or(0, |policy| {
            // The caller's viewport selects current raster quality in the near
            // band. Distance is measured in the containing camera domain.
            let distance = surface.cache_distance;
            previous.as_ref().map_or_else(
                || policy.initial_band(distance),
                |state| policy.band(distance, state.band),
            )
        });
        let limit = self
            .device
            .borrow()
            .surface_cache_limit()
            .min(SURFACE_CACHE_MAX_DIMENSION);
        let limit = surface
            .cache_policy
            .map_or(limit, |policy| policy.texture_limit(limit));
        if limit == 0 {
            if let Some(state) = previous {
                self.release_projected(state);
            }
            return Err(RenderError::UnavailableOutput);
        }
        let mut planes = Vec::new();
        if selection.kind() == OutputKind::Canvas && surface.layer_spacing != 0.0 {
            let scene = CanvasScene::new(host, selection, surface.publication)?;
            let mut used = BTreeSet::new();
            for (order, entry) in scene.canvas.entries.iter().enumerate() {
                let id = entry.layer();
                if used.insert(id) {
                    let coordinate = scene
                        .canvas
                        .layer_offset(id)
                        .ok_or(RenderError::UnavailableOutput)?;
                    planes.push((
                        Some(id),
                        coordinate * f64::from(surface.layer_spacing),
                        order,
                    ));
                }
            }
        }
        // Root/zero-spacing presentations composite the complete logical entry
        // sequence. Grouping nonadjacent entries by plane would reorder paint.
        // Empty outputs also need a transparent image for frame inclusion.
        if planes.is_empty() {
            planes.push((None, 0.0, 0));
        }
        let image_count = if selection.kind() == OutputKind::Canvas {
            if surface.layer_spacing != 0.0 {
                2
            } else {
                1
            }
        } else {
            0
        };
        let offsets: Vec<_> = planes.iter().map(|p| p.1).collect();
        let view = (mvp, viewport);
        let demand = if let Some(state) = previous.as_ref().filter(|state| {
            state.surface.geometry == surface.geometry
                && state
                    .groups
                    .iter()
                    .map(|g| g.offset)
                    .eq(offsets.iter().copied())
                && !state.groups.is_empty()
        }) {
            if state.quality_view == view {
                state.quality_demand
            } else {
                super::quality::surface_demand(&*surface.geometry, &offsets, mvp, viewport)?
            }
        } else {
            super::quality::surface_demand(&*surface.geometry, &offsets, mvp, viewport)?
        };
        let requested = super::quality::image_size(
            demand,
            surface.cache_policy.map_or(1.0, |p| p.resolution_scale),
            limit,
            previous.as_ref().map(|state| state.requested),
        )
        .ok_or(RenderError::UnavailableOutput)?;
        let stamp = OutputContentStamp::read(host, selection, surface.publication)?;
        let paint_context = self.paint_context_stamp(&stamp);
        let raster_dependencies = self.canvas_image_generations(&stamp, selection);
        let mut state = previous.unwrap_or_else(|| ProjectedSurface {
            device: self.device.clone(),
            target: None,
            ownership: false,
            damage: None,
            surface: surface.clone(),
            groups: Vec::new(),
            requested,
            budget_limited: false,
            quality_demand: demand,
            quality_view: view,
            size: [0; 2],
            capacity: [0; 2],
            band,
            stamp: None,
            painted_outputs: BTreeSet::new(),
            represented_images: BTreeSet::new(),
            current_image: false,
            painted_current: false,
            missing_draws: 0,
            raster_dependencies: Vec::new(),
            current_publication: false,
            painted_at: 0.0,
            used_at: time,
            repaints: 0,
            reuses: 0,
            allocations: 0,
            presentation: SurfaceCachePresentation::Unavailable,
            repaint_this_frame: false,
            paint_context: Vec::new(),
        });
        let result = (|| {
            let geometry_changed = state.surface.geometry != surface.geometry
                || state.surface.layer_spacing != surface.layer_spacing
                || state
                    .groups
                    .iter()
                    .map(|g| g.offset)
                    .ne(planes.iter().map(|p| p.1));
            // Membership edits damage the shared image, but do not change its
            // presentation meshes while the occupied planes stay the same.
            let groups_changed = state
                .groups
                .iter()
                .map(|g| g.plane)
                .ne(planes.iter().map(|p| p.0));
            let quality_changed =
                state.requested != requested || state.capacity.iter().any(|value| *value > limit);
            // Inactive allocations are expendable before a visible image loses quality.
            let active_bytes = if selection.kind() == OutputKind::Canvas {
                image_count * requested[0] as usize * requested[1] as usize * 4
            } else {
                0
            };
            if active_bytes
                + self.projected_image_bytes()
                + self.surface_cache.resident().1 as usize
                > self.surface_cache.budget()
            {
                for (other, inactive) in &mut self.projected_surfaces {
                    if !self.projected_visible.contains(other) {
                        if let Some(target) = inactive.target.take() {
                            self.device.borrow_mut().delete_surface_cache_target(target);
                        }
                        inactive.stamp = None;
                        inactive.current_image = false;
                    }
                }
            }
            let available = self.required_image_limit(selection).min(
                self.surface_cache.budget().saturating_sub(
                    self.projected_image_bytes()
                        + self.surface_cache.resident().1 as usize
                        + self.remaining_required_bytes(selection),
                ),
            );
            let next_size =
                std::array::from_fn::<_, 2, _>(|i| (state.size[i] * 2).min(requested[i]));
            let recover_budget = state.budget_limited
                && next_size != state.size
                && image_count * next_size[0] as usize * next_size[1] as usize * 4 <= available;
            if geometry_changed
                || groups_changed
                || quality_changed
                || recover_budget
                || state.groups.is_empty()
                || (selection.kind() == OutputKind::Canvas && state.target.is_none())
                || state.image_bytes()
                    + self.projected_image_bytes()
                    + self.surface_cache.resident().1 as usize
                    > self.surface_cache.budget()
            {
                let candidate = if !quality_changed && !recover_budget && state.size != [0; 2] {
                    state.size
                } else {
                    requested
                };
                let offsets: Vec<_> = planes.iter().map(|p| p.1).collect();
                let mut candidate = candidate;
                let mut image_budget_limited = false;
                let (size, capacity, meshes) = loop {
                    let (size, meshes) = select_quality(&*surface.geometry, &offsets, candidate)?;
                    let other_meshes: usize = self
                        .projected_surfaces
                        .values()
                        .map(ProjectedSurface::mesh_bytes)
                        .sum();
                    let (_, optional_bytes) = self.surface_cache.resident();
                    let other_images = self.projected_image_bytes();
                    let mut capacity = if super::quality::fits(size, state.capacity)
                        && state.capacity.iter().all(|value| *value <= limit)
                    {
                        state.capacity
                    } else {
                        super::quality::image_capacity(size, limit)
                    };
                    let available = self.required_image_limit(selection).min(
                        self.surface_cache.budget().saturating_sub(
                            other_images
                                + optional_bytes as usize
                                + self.remaining_required_bytes(selection),
                        ),
                    );
                    let capacity_bytes =
                        image_count * capacity[0] as usize * capacity[1] as usize * 4;
                    if capacity_bytes > available {
                        capacity = size;
                    }
                    let needed = image_count * capacity[0] as usize * capacity[1] as usize * 4;
                    if other_meshes + meshes.iter().map(|m| m.bytes).sum::<usize>() <= MESH_BUDGET
                        && needed <= available
                    {
                        break (size, capacity, meshes);
                    }
                    if size == [1, 1] {
                        return Err(RenderError::UnavailableOutput);
                    }
                    image_budget_limited |= needed > available;
                    candidate = size.map(|v| (v / 2).max(1));
                };
                let content_same = state.size == size && !groups_changed;
                let reuse_images = state.capacity == capacity;
                if !reuse_images && let Some(target) = state.target.take() {
                    self.device.borrow_mut().delete_surface_cache_target(target);
                }
                let mut groups = Vec::new();
                // Allocate geometry first so a failed upload cannot pair a new
                // image with a stale mesh. All handles remain context-owned.
                for ((plane, offset, painter_order), data) in planes.iter().zip(meshes) {
                    let mesh = GlMeshData::private_mesh(self.device.clone(), data.asset)?;
                    stats.uploaded(data.bytes);
                    groups.push(ProjectedGroup {
                        plane: *plane,
                        offset: *offset,
                        painter_order: *painter_order,
                        mesh,
                        patches: data.patches,
                        bytes: data.bytes,
                    });
                }
                state.groups = groups;
                if !content_same || !reuse_images {
                    state.stamp = None;
                    state.current_image = false;
                }
                state.budget_limited = image_budget_limited;
                state.size = size;
                state.capacity = capacity;
            }
            for (group, (_, _, order)) in state.groups.iter_mut().zip(&planes) {
                group.painter_order = *order;
            }
            state.requested = requested;
            state.quality_demand = demand;
            state.quality_view = view;
            state.band = band;
            state.surface = surface.clone();
            state.used_at = time;
            if selection.kind() == OutputKind::Camera {
                state.painted_at = time;
                return Ok(());
            }
            let dependencies_changed = state
                .stamp
                .as_ref()
                .is_none_or(|old| !old.same_dependencies(&stamp));
            let dirty = state.stamp.as_ref() != Some(&stamp)
                || state.raster_dependencies != raster_dependencies;
            let ownership_changed = if dirty && surface.layer_spacing != 0.0 {
                let scene = CanvasScene::new(host, selection, surface.publication)?;
                state
                    .damage
                    .as_ref()
                    .is_some_and(|painted| painted.ownership_changed(&scene))
            } else {
                false
            };
            let interval = surface
                .cache_policy
                .map_or(0.0, |p| p.refresh_interval_at(band));
            let repaint = state.stamp.is_none()
                || !state.current_image
                || dependencies_changed
                || ownership_changed
                || state.paint_context != paint_context
                || (dirty && (interaction || time - state.painted_at >= interval));
            if repaint {
                let scene = CanvasScene::new(host, selection, surface.publication)?;
                let next_damage = super::damage::CanvasImageDamage::capture::<D>(
                    &scene,
                    state.size,
                    scene.root_clip(),
                    1.0,
                    state.damage.as_ref(),
                );
                let partial = state.current_image
                    && !dependencies_changed
                    && state.paint_context == paint_context
                    && state.raster_dependencies == raster_dependencies
                    && stamp.outputs.len() == 1
                    && state.missing_draws == 0;
                let region = next_damage.region(state.damage.as_ref().filter(|_| partial));
                let parent = std::mem::take(&mut self.inclusions.active);
                self.inclusions.active.collect_image = true;
                self.projected_repainting = Some(selection);
                #[cfg(feature = "instrumentation")]
                let _gpu_repaint = self.gpu_scope(
                    super::super::gpu_profiling::RenderGpuScope::CacheRepaint,
                    Some(selection.world()),
                    None,
                );
                let drawn = (|| {
                    state.missing_draws = 0;
                    state.painted_current = true;
                    {
                        if state.target.is_none() {
                            state.target =
                                Some(self.device.borrow_mut().create_surface_cache_target(
                                    state.capacity[0],
                                    state.capacity[1],
                                )?);
                            state.allocations += 1;
                        }
                        self.device
                            .borrow_mut()
                            .set_surface_cache_target_active_size(
                                state.target.as_mut().expect("target"),
                                state.size[0],
                                state.size[1],
                            )?;
                        self.device.borrow_mut().set_surface_cache_ownership(
                            state.target.as_mut().expect("target"),
                            image_count == 2,
                        )?;
                        state.ownership = image_count == 2;
                        self.device.borrow_mut().set_surface_cutout(0)?;
                        self.device.borrow_mut().set_surface_double_sided(true)?;
                        self.device.borrow_mut().begin_surface_cache_region(
                            state.target.as_ref().expect("target"),
                            region,
                        )?;
                        let extent = scene.canvas.logical_extent.map(f64::from);
                        let mvp = super::super::canvas::layering::plane_matrix(
                            [-1.0; 2],
                            [2.0 / extent[0], 2.0 / extent[1]],
                        )?;
                        let before = stats.summary.failed_draw_calls;
                        let result = self.draw_canvas_region(
                            &scene,
                            mvp,
                            scene.root_clip(),
                            1.0,
                            super::super::canvas::layering::CanvasLayering::FLAT,
                            WorldViewport {
                                width: state.size[0],
                                height: state.size[1],
                                device_pixel_ratio: 1.0,
                            },
                            stats,
                            None,
                            region.map(|rect| next_damage.logical_region(rect)),
                        );
                        let end = self.device.borrow_mut().end_surface_cache_target();
                        result.and(end)?;
                        state.missing_draws += self.surface_missing_draws;
                        state.painted_current &= self.surface_missing.is_empty();
                        // Nonresident primitives skip only their own paint, just
                        // as direct Canvas presentation does. Device/raster
                        // failures still make required presentation unavailable.
                        if stats.summary.failed_draw_calls - before != self.surface_missing_draws
                            || self.surface_gui_unretained
                        {
                            return Err(RenderError::UnavailableOutput);
                        }
                    }
                    Ok(())
                })();
                let restored = self.device.borrow_mut().set_surface_double_sided(false);
                let drawn = drawn.and(restored);
                self.projected_repainting = None;
                state.current_image = drawn.is_ok();
                state.painted_current &= drawn.is_ok() && !self.inclusions.active.stale_image;
                state.painted_outputs = std::mem::take(&mut self.inclusions.active.image_outputs);
                state.represented_images =
                    std::mem::take(&mut self.inclusions.active.represented_images);
                self.inclusions.active = parent;
                drawn?;
                stats.image_repaint(state.size, region);
                state.damage = Some(next_damage);
                state.stamp = Some(stamp.clone());
                state.paint_context = paint_context.clone();
                state.raster_dependencies = raster_dependencies.clone();
                state.painted_at = time;
                state.repaints = state.repaints.saturating_add(1);
                state.repaint_this_frame = true;
                state.presentation = SurfaceCachePresentation::Repainted;
            } else {
                // Keep completeness separate from publication equivalence.
                // A temporally stale image must remain stale until it repaints;
                // its source stamp cannot acknowledge a later changed output.
                state.presentation = SurfaceCachePresentation::Reused;
                state.reuses = state.reuses.saturating_add(1);
                stats.summary.failed_draw_calls += state.missing_draws;
            }
            state.current_publication = state.current_image
                && state.painted_current
                && state.stamp.as_ref() == Some(&stamp)
                && state.raster_dependencies == raster_dependencies
                && state.paint_context == paint_context;
            Ok(())
        })();
        if result.is_err() {
            if let Some(target) = state.target.take() {
                self.device.borrow_mut().delete_surface_cache_target(target);
            }
            state.groups.clear();
            state.presentation = SurfaceCachePresentation::Unavailable;
            state.stamp = None;
        }
        self.projected_surfaces.insert(selection, state);
        result
    }

    pub(in crate::services::render) fn projected_size(
        &self,
        output: OutputRef,
    ) -> Option<[u32; 2]> {
        self.projected_surfaces
            .get(&output)
            .filter(|s| !s.groups.is_empty())
            .map(|s| s.size)
    }

    pub(in crate::services::render) fn projected_camera_painted(&mut self, output: OutputRef) {
        if let Some(state) = self.projected_surfaces.get_mut(&output) {
            state.repaints = state.repaints.saturating_add(1);
            state.presentation = SurfaceCachePresentation::Repainted;
            state.repaint_this_frame = true;
        }
    }

    pub(in crate::services::render) fn append_projected_draws(
        &self,
        draws: &mut Vec<RenderDraw>,
        surfaces: &[SceneOutputSurface],
        vp: &[f32; 16],
    ) {
        for (index, surface) in surfaces
            .iter()
            .enumerate()
            .filter(|(_, s)| s.image_presentation())
        {
            let Some(state) = self.projected_surfaces.get(&surface.selection) else {
                continue;
            };
            let mvp = ipp_core::math::multiply(*vp, surface.model);
            for group in &state.groups {
                for (patch, shape) in group.patches.iter().enumerate() {
                    let p = shape.centre;
                    draws.push(RenderDraw {
                        index: RenderDrawIndex::SurfacePatch(
                            index,
                            group.plane,
                            group.painter_order,
                            patch,
                        ),
                        key: (surface.entity, 2),
                        material: RenderMaterialKey::default(),
                        phase: 2,
                        depth: f64::from(mvp[2]) * p[0]
                            + f64::from(mvp[6]) * p[1]
                            + f64::from(mvp[10]) * p[2]
                            + f64::from(mvp[14]),
                    });
                }
            }
        }
        draws.sort_unstable_by(RenderDraw::compare);
    }

    pub(in crate::services::render) fn draw_projected_patches(
        &mut self,
        host: &HostRuntime,
        surface: &SceneOutputSurface,
        rank: Option<u32>,
        patches: &[usize],
        vp: [f32; 16],
        stats: &mut RenderFrameWork,
    ) -> Result<(), RenderError> {
        #[cfg(feature = "instrumentation")]
        let _gpu_composite = self.gpu_scope(
            super::super::gpu_profiling::RenderGpuScope::Composite,
            Some(surface.entity.world),
            Some(surface.entity.entity),
        );
        if self.surface_image_program.is_none() {
            self.surface_image_program = Some(self.device.borrow_mut().create_program(
                crate::services::render::embedded_shader!("shaders/surface_image.vert"),
                crate::services::render::embedded_shader!("shaders/surface_cache.frag"),
            )?);
        }
        let Some(state) = self.projected_surfaces.get(&surface.selection) else {
            return Ok(());
        };
        let Some(group) = state.groups.iter().find(|g| g.plane == rank) else {
            return Ok(());
        };
        let mut ranges: Vec<[u32; 2]> = Vec::with_capacity(patches.len());
        for &patch in patches {
            let shape = group
                .patches
                .get(patch)
                .ok_or(RenderError::UnavailableOutput)?;
            let count = shape.indices.end - shape.indices.start;
            if let Some(previous) = ranges
                .last_mut()
                .filter(|r| r[0] + r[1] == shape.indices.start)
            {
                previous[1] += count;
            } else {
                ranges.push([shape.indices.start, count]);
            }
        }
        let camera = surface.selection.kind() == OutputKind::Camera;
        let target = if camera {
            if !self.camera_completed.contains(&surface.selection) {
                stats.failed_draw();
                return Ok(());
            }
            self.camera_targets
                .get(&surface.selection)
                .map(|(target, _, _)| target)
        } else {
            state.target.as_ref()
        };
        let Some(target) = target else {
            stats.failed_draw();
            return Ok(());
        };
        let Some(mesh) = group.mesh.gpu()? else {
            stats.failed_draw();
            return Ok(());
        };
        self.device
            .borrow_mut()
            .set_surface_cutout(rank.map_or(0, |id| id + 1))?;
        self.device.borrow_mut().set_surface_double_sided(true)?;
        let result = self.device.borrow_mut().draw_surface_image_mesh(
            self.surface_image_program.as_ref().expect("program"),
            target,
            &mesh,
            &ipp_core::math::multiply(vp, surface.model),
            &surface.extent,
            &[0.0, 0.0, surface.extent[0], surface.extent[1]],
            1.0,
            camera,
            &ranges,
        );
        let reset = self.device.borrow_mut().set_surface_cutout(0);
        let restored = self.device.borrow_mut().set_surface_double_sided(false);
        let draws = result?;
        reset.and(restored)?;
        drop(mesh);
        stats.summary.draw_calls += draws;
        stats.summary.triangles += ranges.iter().map(|range| range[1] / 3).sum::<u32>();
        if camera {
            self.inclusions.composite(surface.selection);
        } else {
            self.inclusions.active.stale_image |= !state.current_publication;
            if state.current_publication && self.inclusions.observing() {
                let stale_parent = std::mem::take(&mut self.inclusions.active.stale_image);
                for (output, publication) in super::super::canvas::scene::output_order(
                    host,
                    surface.selection,
                    surface.publication,
                )? {
                    if state.painted_outputs.contains(&output) {
                        self.inclusions.record(output, publication);
                    }
                }
                self.inclusions.active.stale_image = stale_parent;
            }
            let represented_images = state.represented_images.clone();
            self.present_canvas_image_dependencies(&represented_images);
        }
        Ok(())
    }

    fn release_projected(&mut self, state: ProjectedSurface<D>) {
        drop(state);
    }

    pub(in crate::services::render) fn retain_projected_outputs(
        &mut self,
        outputs: &BTreeSet<OutputRef>,
    ) {
        let removed: Vec<_> = self
            .projected_surfaces
            .keys()
            .filter(|s| !outputs.contains(s))
            .copied()
            .collect();
        for output in removed {
            let state = self.projected_surfaces.remove(&output).expect("retired");
            self.release_projected(state);
        }
    }

    pub(in crate::services::render) fn forget_projected_world(&mut self, world: ipp_core::WorldId) {
        let removed: Vec<_> = self
            .projected_surfaces
            .iter()
            .filter(|(s, state)| {
                s.world().id() == world
                    || state.surface.entity.world.id() == world
                    || state
                        .stamp
                        .as_ref()
                        .is_some_and(|s| s.contains_world(world))
            })
            .map(|(s, _)| *s)
            .collect();
        for output in removed {
            let state = self.projected_surfaces.remove(&output).expect("retired");
            self.release_projected(state);
        }
    }

    pub(in crate::services::render) fn clear_projected_surfaces(&mut self) {
        for (_, state) in std::mem::take(&mut self.projected_surfaces) {
            self.release_projected(state);
        }
        if let Some(program) = self.surface_image_program.take() {
            self.device.borrow_mut().delete_program(program);
        }
        self.projected_repainting = None;
    }

    pub(super) fn projected_diagnostics(
        &self,
        world: ipp_core::WorldId,
        out: &mut Vec<SurfaceCacheDiagnostic>,
    ) {
        out.extend(
            self.projected_surfaces
                .values()
                .filter(|s| s.surface.entity.world.id() == world)
                .map(|state| {
                    let mut diagnostic = state.diagnostic();
                    if state.surface.selection.kind() == OutputKind::Camera
                        && let Some((_, _, capacity)) =
                            self.camera_targets.get(&state.surface.selection)
                    {
                        diagnostic.size = self.camera_targets[&state.surface.selection].1;
                        diagnostic.capacity = *capacity;
                        diagnostic.resident_bytes = capacity[0] * capacity[1] * 4;
                    }
                    if state.surface.selection.kind() == OutputKind::Camera
                        && !self.camera_completed.contains(&state.surface.selection)
                    {
                        diagnostic.presentation = SurfaceCachePresentation::Unavailable;
                        diagnostic.size = [0; 2];
                        diagnostic.capacity = [0; 2];
                        diagnostic.resident_bytes = 0;
                    }
                    diagnostic
                }),
        );
        out.sort_by_key(|d| d.entity);
    }

    pub(in crate::services::render) fn projected_image_bytes(&self) -> usize {
        self.projected_surfaces
            .values()
            .map(ProjectedSurface::image_bytes)
            .sum::<usize>()
            + self
                .camera_targets
                .values()
                .map(|(_, _, c)| c[0] as usize * c[1] as usize * 4)
                .sum::<usize>()
    }

    pub(in crate::services::render) fn projected_statistics(
        &self,
        statistics: &mut crate::RenderStatistics,
    ) {
        for state in self.projected_surfaces.values() {
            if state.surface.selection.kind() == OutputKind::Canvas {
                let images = u32::from(state.target.is_some());
                if state.presentation == SurfaceCachePresentation::Repainted {
                    statistics.surface_cache_repaints =
                        statistics.surface_cache_repaints.saturating_add(images);
                }
                if state.presentation == SurfaceCachePresentation::Reused {
                    statistics.surface_cache_reuses =
                        statistics.surface_cache_reuses.saturating_add(images);
                }
                statistics.surface_cache_allocations = statistics
                    .surface_cache_allocations
                    .saturating_add(state.allocations);
                statistics.surface_cache_entries =
                    statistics.surface_cache_entries.saturating_add(images);
                statistics.surface_cache_resident_bytes = statistics
                    .surface_cache_resident_bytes
                    .saturating_add(state.image_bytes() as u32);
            }
        }
    }
}
