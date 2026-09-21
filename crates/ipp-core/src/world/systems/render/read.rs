use super::*;

fn mesh_upload(
    upload: MeshUpload,
) -> Result<crate::services::asset_management::AssetUpload, ErrorReason> {
    Ok(crate::services::asset_management::AssetUpload {
        id: upload.id,
        key: crate::services::asset_management::AssetUploadIdentity {
            kind: crate::MESH_TYPE,
            asset: upload.key.asset,
            variant: upload.key.variant,
        },
        bytes: upload.bytes,
    })
}

fn texture_upload(upload: TextureUpload) -> crate::services::asset_management::AssetUpload {
    crate::services::asset_management::AssetUpload {
        id: upload.id,
        key: crate::services::asset_management::AssetUploadIdentity {
            kind: crate::TEXTURE_TYPE,
            asset: upload.key.asset,
            variant: upload.key.variant,
        },
        bytes: upload.bytes,
    }
}

impl crate::WorldContext<'_> {
    /// Borrow the completed frame's effective custom material inputs.
    pub fn custom_material(&self, entity: EntityId) -> Option<&CustomMaterial> {
        self.world.state.entities.get(&entity)?;
        self.world
            .components
            .custom_material(entity.index() as usize)
    }

    /// Resolve a retained source selection in this World's producer namespace.
    pub fn asset_source_key(
        &self,
        kind: crate::services::asset_management::AssetTypeId,
        source: &str,
        variant: u32,
    ) -> Option<crate::services::asset_management::AssetKey> {
        if crate::allocation_optimizations_enabled() {
            return self
                .asset_acquisition
                .find_source(self.world.id, kind, source, variant);
        }
        let selection = crate::services::asset_management::service::AssetDemandSelection::new(
            kind, source, variant,
        );
        self.asset_acquisition.find(
            &crate::services::asset_management::service::AssetManagementService::scoped_selection(
                self.world.id,
                &selection,
            )
            .descriptor(),
        )
    }

    /// Typed convenience adapter; storage and loading remain generic.
    pub fn enqueue_mesh(&mut self, upload: MeshUpload) -> Result<(), ErrorReason> {
        self.enqueue_asset(mesh_upload(upload)?)
    }

    /// Typed convenience adapter; storage and loading remain generic.
    pub fn enqueue_texture(&mut self, upload: TextureUpload) -> Result<(), ErrorReason> {
        self.enqueue_asset(texture_upload(upload))
    }
}

impl<'a> RenderReadAccess<'a> {
    pub(in crate::world) fn new(
        world: &'a WorldSimulationState,
        assets: &'a AssetManagementService,
        render: &'a RenderSystemState,
    ) -> Self {
        Self {
            world,
            assets,
            render,
        }
    }

    /// Evaluated draw inputs retained by RenderSystem for the completed world frame.
    pub fn render_items(&self) -> &'a [RenderItem] {
        &self.render.items
    }

    /// Evaluated debug shape inputs for the completed world frame.
    pub fn debug_render_items(&self) -> &'a [DebugRenderItem] {
        &self.render.debug_items
    }

    /// Evaluated Surface inputs retained independently of mesh submissions.
    #[cfg(feature = "surfaces")]
    pub fn surface_render_items(&self) -> &'a [crate::SurfaceRenderItem] {
        &self.render.surface_items
    }

    /// Data compatibility diagnostics from the completed render preparation pass.
    pub fn render_diagnostics(&self) -> Vec<RenderDiagnostic> {
        self.render.diagnostics.clone()
    }

    pub(in crate::world) fn resolved_mesh(
        &self,
        entity: EntityId,
        source: &str,
        variant: u32,
    ) -> Option<MeshKey> {
        let key = cached_source_key(
            self.assets,
            self.world.id,
            &self.render.mesh_keys,
            entity,
            AssetResourceKind::Mesh,
            source,
            variant,
        )?;
        self.assets.get(key)?.data()?;
        Some(MeshKey {
            asset: key.to_u64(),
            variant,
        })
    }

    pub(in crate::world) fn resolved_texture(
        &self,
        entity: EntityId,
        source: &str,
        variant: u32,
    ) -> Option<TextureKey> {
        let key = cached_source_key(
            self.assets,
            self.world.id,
            &self.render.texture_keys,
            entity,
            AssetResourceKind::Texture,
            source,
            variant,
        )?;
        self.assets.get(key)?.data()?;
        Some(TextureKey {
            asset: key.to_u64(),
            variant,
        })
    }

    fn base_color_texture(&self, index: usize) -> Option<(&str, u32)> {
        self.world
            .components
            .base_color_texture(index)
            .map(|texture| (texture.source.as_str(), texture.variant))
            .or_else(|| {
                self.world
                    .components
                    .unlit_texture(index)
                    .map(|texture| (texture.source.as_str(), texture.variant))
            })
    }

    /// An unavailable declared resource can make an entry absent until loading completes.
    /// Retry membership preparation while that exact input remains pending.
    pub(super) fn render_resources_pending(&self, entity: EntityId) -> bool {
        let index = entity.index() as usize;
        #[cfg(feature = "particles")]
        let sprite = self.world.components.particle_sprite(index);
        #[cfg(feature = "particles")]
        let particle_mesh = self.world.components.particle_mesh(index);
        #[cfg(feature = "particles")]
        if ((sprite.is_some() || particle_mesh.is_some())
            && self.world.components.particle_emitter(index).is_none()
            && self.world.components.particle_playback(index).is_none())
            || (sprite.is_some() && particle_mesh.is_some())
        {
            return false;
        }
        if self.world.components.unlit_material(index).is_none()
            && self.world.components.pbr_material(index).is_none()
            && self.world.components.custom_material(index).is_none()
            && {
                #[cfg(feature = "particles")]
                {
                    sprite.is_none()
                }
                #[cfg(not(feature = "particles"))]
                {
                    true
                }
            }
        {
            return false;
        }
        #[cfg(feature = "particles")]
        let mesh_selection = if sprite.is_some() {
            None
        } else {
            particle_mesh
                .map(|mesh| (mesh.source.as_str(), mesh.variant))
                .or_else(|| {
                    self.world
                        .components
                        .mesh_instance(index)
                        .map(|mesh| (mesh.source.as_str(), mesh.variant))
                })
        };
        #[cfg(not(feature = "particles"))]
        let mesh_selection = self
            .world
            .components
            .mesh_instance(index)
            .map(|mesh| (mesh.source.as_str(), mesh.variant));
        if let Some((source, variant)) = mesh_selection
            && !source.is_empty()
            && self.unresolved_loading_asset(crate::MESH_TYPE, source, variant)
        {
            return true;
        }
        #[cfg(feature = "mesh-poses")]
        if let Some(pose) = self.world.components.mesh_pose(index)
            && !pose.source.is_empty()
            && self.unresolved_loading_asset(crate::MESH_TYPE, &pose.source, pose.variant)
        {
            return true;
        }
        #[cfg(feature = "particles")]
        let texture_selection = if let Some(sprite) = sprite {
            (!sprite.source.is_empty()).then_some((sprite.source.as_str(), sprite.variant))
        } else {
            self.base_color_texture(index)
        };
        #[cfg(not(feature = "particles"))]
        let texture_selection = self.base_color_texture(index);
        if let Some((source, variant)) = texture_selection
            && !source.is_empty()
            && self.unresolved_loading_asset(crate::TEXTURE_TYPE, source, variant)
        {
            return true;
        }
        false
    }

    fn unresolved_loading_asset(
        &self,
        kind: crate::services::asset_management::AssetTypeId,
        source: &str,
        variant: u32,
    ) -> bool {
        let Some(key) = self
            .assets
            .find_source(self.world.id, kind, source, variant)
        else {
            return false;
        };
        let Some(provider) = self.assets.get(key) else {
            return false;
        };
        provider.data().is_none()
            && matches!(
                provider.status(),
                crate::services::asset_management::AssetLoadStatus::Unloaded
                    | crate::services::asset_management::AssetLoadStatus::Start
                    | crate::services::asset_management::AssetLoadStatus::Progress { .. }
            )
    }

    /// Observe per-use compatibility without poisoning data valid for other uses.
    pub(in crate::world) fn prepare_render_diagnostics(
        &self,
        diagnostics: Vec<RenderDiagnostic>,
    ) -> Vec<RenderDiagnostic> {
        let diagnostics = {
            let mut diagnostics = diagnostics;
            for &entity in self.world.state.entities.keys() {
                let index = entity.index() as usize;
                if self.world.components.transform(index).is_some()
                    && (self.world.components.unlit_material(index).is_some()
                        || self.world.components.pbr_material(index).is_some())
                    && self.base_color_texture(index).is_some()
                    && let Some(key) = self
                        .world
                        .components
                        .mesh_instance(index)
                        .and_then(|mesh| self.resolved_mesh(entity, &mesh.source, mesh.variant))
                    && self.mesh_metadata(key).is_some_and(|mesh| !mesh.has_uvs())
                {
                    diagnostics.push(RenderDiagnostic {
                        entity,
                        reason: ErrorReason::InvalidAsset,
                    });
                }
            }
            diagnostics
        };
        #[cfg(feature = "mesh-poses")]
        let diagnostics = {
            let mut diagnostics = diagnostics;
            for &entity in self.world.state.entities.keys() {
                if let Err(reason) = self.mesh_pose(entity)
                    && reason != ErrorReason::GeometryUnavailable
                {
                    diagnostics.push(RenderDiagnostic {
                        entity,
                        reason,
                    });
                }
            }
            diagnostics
        };
        diagnostics
    }

    /// Read the decoded mesh retained by its concrete resource.
    pub fn mesh(&self, key: MeshKey) -> Option<&'a MeshAsset> {
        self.assets
            .get(resolve_asset_key(
                self.assets,
                self.world.id,
                crate::services::asset_management::AssetUploadIdentity {
                    kind: crate::MESH_TYPE,
                    asset: key.asset,
                    variant: key.variant,
                },
            )?)
            .and_then(|resource| resource.data()?.decoded().downcast_ref())
    }

    /// Read compact geometry and topology independently of bulk mesh streams.
    pub fn mesh_metadata(
        &self,
        key: MeshKey,
    ) -> Option<&'a crate::services::asset_management::mesh_metadata::MeshMetadata> {
        self.assets
            .get(resolve_asset_key(
                self.assets,
                self.world.id,
                crate::services::asset_management::AssetUploadIdentity {
                    kind: crate::MESH_TYPE,
                    asset: key.asset,
                    variant: key.variant,
                },
            )?)
            .and_then(|resource| {
                let data = resource.data()?;
                data.metadata().downcast_ref()
            })
    }

    /// CPU pixels, when retained by the selected concrete resource implementation.
    pub fn texture(&self, key: TextureKey) -> Option<&'a TextureAsset> {
        self.assets
            .get(resolve_asset_key(
                self.assets,
                self.world.id,
                crate::services::asset_management::AssetUploadIdentity {
                    kind: crate::TEXTURE_TYPE,
                    asset: key.asset,
                    variant: key.variant,
                },
            )?)
            .and_then(|resource| resource.data()?.decoded().downcast_ref())
    }

    /// Read only complete renderables from final effective components.
    /// Visible debug declarations, independent of client-managed asset resources.
    /// Renderers without private debug assets simply do not consume these items.
    pub(in crate::world) fn prepare_debug_render_items(
        &self,
        mut items: Vec<DebugRenderItem>,
    ) -> Vec<DebugRenderItem> {
        items.clear();
        for entry in &self.render.debug_entries {
            let entity = entry.entity;
            macro_rules! append {
                ($component:ident) => {
                    if let Some(value) = entry
                        .$component
                        .map(|binding| binding.get(&self.world.components))
                        && (value.is_rendered || self.render.render_state.show_all_debug_geometries)
                        && let Some(state) = &value.runtime.evaluation
                        && let Ok(shape) = &state.evaluated
                    {
                        for part in &shape.parts {
                            if let Some((geometry, model)) =
                                crate::systems::geometry::GeometryPrimitiveVisual::from_shape(
                                    part,
                                    value.outline,
                                    value.stroke,
                                )
                            {
                                items.push(DebugRenderItem {
                                    entity,
                                    model,
                                    geometry,
                                    color: if value.has_color_override {
                                        [value.r, value.g, value.b]
                                    } else {
                                        self.render.render_state.debug_geometry_color
                                    },
                                });
                            }
                        }
                    }
                };
            }
            append!(bounds);
            append!(picking);
        }
        items
    }

    /// Active direct lights in stable entity order, copied from effective storage.
    pub fn light_items(&self) -> impl Iterator<Item = (EntityId, [f32; 16], Light)> + use<'a> {
        let storage = &self.world.components;
        self.render
            .light_entries
            .iter()
            .filter_map(move |entry| entry.sample(storage))
    }

    pub(in crate::world) fn compile_render_item(
        &self,
        entity: EntityId,
        transform: Transform,
    ) -> Option<RenderItem> {
        let index = entity.index() as usize;
        #[cfg(feature = "particles")]
        let sprite = self.world.components.particle_sprite(index);
        #[cfg(feature = "particles")]
        let particle_mesh = self.world.components.particle_mesh(index);
        #[cfg(feature = "particles")]
        if (sprite.is_some() || particle_mesh.is_some())
            && self.world.components.particle_emitter(index).is_none()
            && self.world.components.particle_playback(index).is_none()
        {
            return None;
        }
        #[cfg(feature = "particles")]
        if sprite.is_some() && particle_mesh.is_some() {
            return None;
        }
        #[cfg(feature = "particles")]
        let selection = particle_mesh
            .map(|m| (m.source.as_str(), m.variant))
            .or_else(|| {
                self.world
                    .components
                    .mesh_instance(index)
                    .map(|m| (m.source.as_str(), m.variant))
            });
        #[cfg(not(feature = "particles"))]
        let selection = self
            .world
            .components
            .mesh_instance(index)
            .map(|m| (m.source.as_str(), m.variant));
        #[cfg(feature = "particles")]
        let mesh = if sprite.is_some() {
            MeshKey {
                asset: 0,
                variant: 0,
            }
        } else {
            let (source, variant) = selection?;
            self.resolved_mesh(entity, source, variant)?
        };
        #[cfg(not(feature = "particles"))]
        let mesh = {
            let (source, variant) = selection?;
            self.resolved_mesh(entity, source, variant)?
        };
        let custom = self.world.components.custom_material(index).is_some();
        let mut solid_fallback = custom
            && self.world.components.pbr_material(index).is_none()
            && self.world.components.unlit_material(index).is_none();
        let mut pbr = self.world.components.pbr_material(index).copied();
        #[allow(unused_mut)]
        let mut material = pbr
            .map(|p| UnlitMaterial {
                r: p.r,
                g: p.g,
                b: p.b,
            })
            .or_else(|| self.world.components.unlit_material(index).copied())
            .or_else(|| {
                custom.then_some(UnlitMaterial {
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                })
            });
        #[cfg(feature = "particles")]
        if let Some(sprite) = sprite {
            material = Some(UnlitMaterial {
                r: sprite.r,
                g: sprite.g,
                b: sprite.b,
            });
            pbr = None;
            solid_fallback = false;
        }
        let mut material = material?;
        let texture_selection = self.base_color_texture(index);
        #[cfg(feature = "particles")]
        let texture_selection = if let Some(s) = sprite {
            (!s.source.is_empty()).then_some((s.source.as_str(), s.variant))
        } else {
            texture_selection
        };
        let texture = match texture_selection {
            Some((source, variant)) => {
                match self.resolved_texture(entity, source, variant).filter(|_| {
                    #[cfg(feature = "particles")]
                    if sprite.is_some() {
                        return true;
                    }
                    self.mesh_metadata(mesh).is_some_and(|m| m.has_uvs())
                }) {
                    Some(key) => Some(key),
                    None if custom => {
                        solid_fallback = true;
                        pbr = None;
                        material = UnlitMaterial {
                            r: 1.0,
                            g: 0.0,
                            b: 0.0,
                        };
                        None
                    }
                    None => return None,
                }
            }
            None => None,
        };
        let metadata = self.mesh_metadata(mesh);
        let normals = metadata.is_some_and(|m| m.has_normals());
        #[cfg(feature = "mesh-poses")]
        let pose = self.mesh_pose(entity).ok()?;
        #[cfg(feature = "mesh-poses")]
        let normals = normals
            && pose.is_none_or(|(key, _)| self.mesh_metadata(key).is_some_and(|m| m.has_normals()));
        Some(RenderItem {
            #[cfg(feature = "particles")]
            particle: None,
            solid_fallback,
            custom_material: custom,
            normals,
            texture_weights: metadata.is_some_and(|m| m.has_texture_weights()),
            #[cfg(feature = "skeletal-animation")]
            skinned: self.world.components.skin(index).is_some(),
            entity,
            transform,
            model: [0.0; 16],
            normal: Err(ErrorReason::InvalidValue),
            material,
            pbr,
            mesh,
            #[cfg(feature = "mesh-poses")]
            pose,
            texture,
        })
    }
}

impl crate::WorldContext<'_> {
    pub(in crate::world) fn render_read(&self) -> RenderReadAccess<'_> {
        RenderReadAccess::new(
            self.world,
            self.asset_acquisition,
            &self
                .system::<RenderSystem>(RenderSystem::ID)
                .expect("World requires RenderSystem")
                .state,
        )
    }

    /// Evaluated draw inputs retained by RenderSystem for the completed world frame.
    pub fn render_items(&self) -> &[RenderItem] {
        self.render_read().render_items()
    }

    /// Evaluated debug shape inputs for the completed world frame.
    pub fn debug_render_items(&self) -> &[DebugRenderItem] {
        self.render_read().debug_render_items()
    }

    /// Evaluated Surface inputs retained independently of mesh submissions.
    #[cfg(feature = "surfaces")]
    pub fn surface_render_items(&self) -> &[crate::SurfaceRenderItem] {
        self.render_read().surface_render_items()
    }

    /// Data compatibility diagnostics from the completed render preparation pass.
    pub fn render_diagnostics(&self) -> Vec<RenderDiagnostic> {
        self.render_read().render_diagnostics()
    }

    /// Read compact geometry and topology independently of bulk mesh streams.
    pub fn mesh_metadata(
        &self,
        key: MeshKey,
    ) -> Option<&crate::services::asset_management::mesh_metadata::MeshMetadata> {
        self.render_read().mesh_metadata(key)
    }

    /// Clone only requested diagnostic records for a completed-frame page.
    pub fn render_diagnostic_page(
        &self,
        after: u64,
        target: u64,
        limit: usize,
    ) -> Vec<RenderDiagnostic> {
        self.render_read()
            .render
            .diagnostics
            .iter()
            .filter(|item| {
                item.entity.to_bits() > after && (target == 0 || item.entity.to_bits() == target)
            })
            .take(limit)
            .cloned()
            .collect()
    }

    /// Full CPU mesh when retained by the selected provider.
    pub fn mesh(&self, key: MeshKey) -> Option<&MeshAsset> {
        self.render_read().mesh(key)
    }

    /// CPU pixels, when retained by the selected concrete resource implementation.
    pub fn texture(&self, key: TextureKey) -> Option<&TextureAsset> {
        self.render_read().texture(key)
    }

    /// Active direct lights in stable entity order, copied from effective storage.
    pub fn light_items(&self) -> impl Iterator<Item = (EntityId, [f32; 16], Light)> + '_ {
        self.render_read().light_items()
    }
}
