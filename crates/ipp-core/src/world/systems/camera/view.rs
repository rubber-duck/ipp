//! Explicit completed camera domains and producer navigation, without presentation ownership.

use super::{CameraMotion, CameraPublication, CameraSystem};
use crate::{
    ErrorReason, HostIngressView, HostRuntime, OutputKind, OutputRef, RootOutputBinding,
    ViewDescriptor, ViewQueryTarget, WorldAttachmentMode, WorldAttachmentToken, WorldPlane,
    WorldPublicationId, WorldRef,
};

/// Gesture deltas; projection dimensions come from the validated view, never client target pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CameraViewMotion {
    /// Orbit about the producer camera's focus pivot.
    Rotate {
        /// Camera-local up rotation in radians.
        yaw: f32,
        /// Camera-local right rotation in radians after yaw.
        pitch: f32,
    },
    /// Move the scene with normalized view-plane deltas.
    Pan {
        /// Horizontal delta, positive right.
        x: f32,
        /// Vertical delta, positive down.
        y: f32,
    },
    /// Scale the producer focus distance or orthographic extent.
    Zoom {
        /// Natural logarithmic factor; positive moves outward.
        amount: f32,
    },
}

/// Resolved CPU projection inputs. This is not a renderer completion or action authority token.
#[derive(Clone, Debug, PartialEq)]
pub struct CameraProjectionView {
    /// Exact source root and caller dimensions.
    pub root: ViewDescriptor,
    /// Camera selected by the completed path.
    pub output: OutputRef,
    /// Camera's still-available source publication.
    pub publication: WorldPublicationId,
    /// Root pixel extent or containing Surface physical extent.
    pub extent: [f64; 2],
    /// Root-to-Camera applied edge identities; no current World-value mirror.
    pub path: Vec<WorldAttachmentToken>,
}

/// Ordered view-qualified producer edit. Only Host validation constructs it.
pub struct CameraNavigationCommand {
    binding: RootOutputBinding,
    view: CameraProjectionView,
    motion: CameraViewMotion,
    exact_source: bool,
}

impl HostRuntime {
    /// Resolve an explicit completed Camera path without borrowing mutable World components.
    pub fn resolve_camera_view(
        &self,
        root: ViewQueryTarget,
        path: &[WorldAttachmentToken],
    ) -> Result<CameraProjectionView, ErrorReason> {
        let root = self.resolve_view(root)?;
        let mut output = root.output;
        let mut publication = self
            .publication(root.publication)
            .ok_or(ErrorReason::InvalidEntity)?;
        let mut extent = [
            f64::from(root.viewport.width),
            f64::from(root.viewport.height),
        ];
        for token in path {
            let edge = publication
                .attachments
                .iter()
                .find(|edge| &edge.token == token)
                .ok_or(ErrorReason::InvalidEntity)?;
            if edge.placement_output.is_some_and(|owner| owner != output)
                || (output.kind() == OutputKind::Canvas && edge.placement_output != Some(output))
            {
                return Err(ErrorReason::InvalidEntity);
            }
            publication = self
                .attached_publication(edge)
                .ok_or(ErrorReason::InvalidEntity)?;
            if edge.mode != WorldAttachmentMode::Spatial {
                output = edge.output.ok_or(ErrorReason::InvalidEntity)?;
                extent = edge.surface_extent.ok_or(ErrorReason::InvalidGeometry)?;
            }
        }
        let camera = self
            .output(publication.id, output)
            .and_then(|chunk| chunk.data::<CameraPublication>())
            .ok_or(ErrorReason::InvalidEntity)?;
        camera.prepare_for_extent(extent)?;
        Ok(CameraProjectionView {
            root,
            output,
            publication: publication.id,
            extent,
            path: path.into(),
        })
    }

    /// Project normalized coordinates in the selected nested Camera domain onto its explicit plane.
    pub fn project_camera_view(
        &self,
        root: ViewQueryTarget,
        path: &[WorldAttachmentToken],
        point: [f32; 2],
        plane: WorldPlane,
    ) -> Result<(CameraProjectionView, Option<[f32; 3]>), ErrorReason> {
        let view = self.resolve_camera_view(root, path)?;
        let camera = self
            .output(view.publication, view.output)
            .and_then(|chunk| chunk.data::<CameraPublication>())
            .ok_or(ErrorReason::InvalidEntity)?;
        let position = camera.project_for_extent(point, view.extent, plane)?;
        Ok((view, position))
    }

    /// Pick explicit geometry inside this Camera domain, returning its World-qualified identity.
    pub fn pick_camera_view(
        &self,
        root: ViewQueryTarget,
        path: &[WorldAttachmentToken],
        point: [f32; 2],
    ) -> Result<(CameraProjectionView, Option<crate::PublishedSceneHit>), ErrorReason> {
        let view = self.resolve_camera_view(root, path)?;
        let camera = self
            .output(view.publication, view.output)
            .and_then(|chunk| chunk.data::<CameraPublication>())
            .ok_or(ErrorReason::InvalidEntity)?;
        let ray = camera.ray_for_extent(point, view.extent)?;
        let hit =
            self.pick_publication(view.publication, view.output, &ray.ray, ray.near, ray.far)?;
        Ok((view, hit))
    }

    /// Prepare a normal queued CameraSystem command. No evaluation or authoring occurs here.
    /// No source constraint reads current completed state at mutation; an exact source never falls back.
    pub fn camera_navigation(
        &self,
        binding: RootOutputBinding,
        publication: Option<WorldPublicationId>,
        path: &[WorldAttachmentToken],
        motion: CameraViewMotion,
    ) -> Result<CameraNavigationCommand, ErrorReason> {
        let view = self.resolve_camera_view(
            ViewQueryTarget::BoundView {
                binding,
                publication,
            },
            path,
        )?;
        Ok(CameraNavigationCommand {
            binding,
            view,
            motion,
            exact_source: publication.is_some(),
        })
    }
}

impl CameraNavigationCommand {
    /// Exact mutation destination; no fallback to CameraSystem's legacy active selection.
    pub fn output(&self) -> OutputRef {
        self.view.output
    }

    pub(super) fn world_references(&self, visit: &mut dyn FnMut(WorldRef)) {
        visit(self.binding.output.world());
        for token in &self.view.path {
            visit(token.parent());
            if let Some(child) = token.child() {
                visit(child);
            }
        }
        visit(self.view.output.world());
    }

    fn validate(&self, ingress: &HostIngressView<'_>) -> Result<[f64; 2], ErrorReason> {
        if ingress.root_binding(self.binding.output.world()) != Some(self.binding) {
            return Err(ErrorReason::InvalidEntity);
        }
        let mut publication = if self.exact_source {
            ingress.publication(self.view.root.publication)
        } else {
            ingress.latest_publication(self.binding.output.world())
        }
        .ok_or(ErrorReason::InvalidEntity)?;
        let mut output = self.binding.output;
        let mut extent = [
            f64::from(self.binding.viewport.width),
            f64::from(self.binding.viewport.height),
        ];
        let check_world = |world| {
            if ingress
                .world(world)
                .is_none_or(|world| world.fault().is_some())
            {
                Err(ErrorReason::InvalidEntity)
            } else {
                Ok(())
            }
        };
        check_world(publication.world)?;
        ingress
            .output(publication.id, output)
            .ok_or(ErrorReason::InvalidEntity)?;
        for token in &self.view.path {
            let edge = publication
                .attachments
                .iter()
                .find(|edge| &edge.token == token)
                .ok_or(ErrorReason::InvalidEntity)?;
            if edge.placement_output.is_some_and(|owner| owner != output) {
                return Err(ErrorReason::InvalidEntity);
            }
            #[cfg(feature = "surfaces")]
            if edge.mode != WorldAttachmentMode::Spatial {
                let parent = ingress
                    .world(publication.world)
                    .ok_or(ErrorReason::InvalidEntity)?;
                let Some(crate::ComponentValue::Surface(surface)) =
                    parent.effective_component(edge.anchor, crate::ComponentValue::SURFACE)
                else {
                    return Err(ErrorReason::InvalidEntity);
                };
                if !parent.component_is_active(edge.anchor, crate::ComponentValue::SURFACE)
                    || Some([f64::from(surface.width), f64::from(surface.height)])
                        != edge.surface_extent
                {
                    return Err(ErrorReason::InvalidEntity);
                }
            }
            publication = ingress
                .attached_publication(edge)
                .ok_or(ErrorReason::InvalidEntity)?;
            check_world(publication.world)?;
            if edge.mode != WorldAttachmentMode::Spatial {
                output = edge.output.ok_or(ErrorReason::InvalidEntity)?;
                extent = edge.surface_extent.ok_or(ErrorReason::InvalidGeometry)?;
            }
        }
        if output != self.view.output
            || (self.exact_source && publication.id != self.view.publication)
        {
            return Err(ErrorReason::InvalidEntity);
        }
        ingress
            .output(publication.id, output)
            .ok_or(ErrorReason::InvalidEntity)?;
        Ok(extent)
    }

    pub(super) fn apply(
        &self,
        system: &mut CameraSystem,
        context: &mut crate::systems::SystemCommandContext<'_>,
    ) -> Result<(), ErrorReason> {
        let extent = self.validate(&context.host_ingress().ok_or(ErrorReason::InvalidEntity)?)?;
        if context.world.view().world_ref() != self.view.output.world() {
            return Err(ErrorReason::InvalidEntity);
        }
        let motion = match self.motion {
            CameraViewMotion::Rotate {
                yaw,
                pitch,
            } => CameraMotion::Rotate {
                yaw,
                pitch,
            },
            CameraViewMotion::Pan {
                x,
                y,
            } => CameraMotion::Pan {
                x,
                y,
                width: 1,
                height: 1,
            },
            CameraViewMotion::Zoom {
                amount,
            } => CameraMotion::Zoom {
                amount,
            },
        };
        system.navigate_entity(
            &mut context.world,
            self.view
                .output
                .camera_entity()
                .ok_or(ErrorReason::InvalidEntity)?,
            motion,
            Some(extent),
        )
    }
}
