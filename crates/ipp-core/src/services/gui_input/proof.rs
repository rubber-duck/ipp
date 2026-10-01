use super::{GuiInputContext, GuiInputError};
use crate::systems::SystemWorldView;
use crate::systems::canvas::{CanvasHitKind, CanvasPublication, CanvasSystem};
use crate::systems::gui::local::{GuiEntityTarget, GuiLocalEffectSource};
use crate::systems::gui::presentation::GuiCanvasPublication;
use crate::{
    ComponentValue, HostIngressView, HostRuntime, OutputKind, OutputRef, PublishedWorldAttachment,
    WorldAttachmentMode, WorldAttachmentToken, WorldPublication, WorldPublicationId, WorldRef,
};
use std::collections::BTreeSet;

/// Immutable command-owned provenance minted from an explicit Host context and completed path.
/// Its fields cannot be supplied by a protocol client. It is not a hit-testing implementation.
pub struct GuiRoutedInputProof {
    _source_lease: crate::host::publication::PublicationReadLease,
    pub(super) context: GuiInputContext,
    pub(super) source: WorldPublicationId,
    pub(super) path: Box<[WorldAttachmentToken]>,
    pub(super) target: GuiEntityTarget,
}

impl GuiRoutedInputProof {
    /// Routed provenance of the effects this input applies.
    pub(super) fn effect_source(&self) -> GuiLocalEffectSource {
        GuiLocalEffectSource::Routed {
            publication: self.source(),
        }
    }

    /// Retained attachment path length, charged to the service budget.
    pub(super) fn path_nodes(&self) -> usize {
        self.path.len()
    }

    /// Visit every World the context and path reference.
    pub(super) fn visit(&self, visit: &mut dyn FnMut(WorldRef)) {
        visit(self.context.root().output.world());
        for token in &self.path {
            visit(token.parent());
            if let Some(child) = token.child() {
                visit(child);
            }
        }
    }

    /// Check the target World's GUI readiness, then this proof's context and path.
    pub(super) fn validate_target(
        &self,
        view: &HostIngressView<'_>,
        target: GuiEntityTarget,
    ) -> Result<(), GuiInputError> {
        let world = view.world(target.world).ok_or(GuiInputError::Unavailable)?;
        if world.fault().is_some()
            || !world
                .manifest()
                .systems()
                .contains(&crate::systems::gui::GuiSystem::ID)
        {
            return Err(GuiInputError::Unavailable);
        }
        self.validate(view)
    }
}

impl GuiRoutedInputProof {
    pub(super) fn capture(
        host: &HostRuntime,
        context: GuiInputContext,
        target: GuiEntityTarget,
        path: &[WorldAttachmentToken],
        source: Option<WorldPublicationId>,
    ) -> Result<Self, GuiInputError> {
        let binding = context.root();
        if host
            .root_output_binding(binding.output.world())
            .ok()
            .flatten()
            != Some(binding)
        {
            return Err(GuiInputError::StaleContext);
        }
        let (output, _, current) = host
            .root_output(binding.output.world().id())
            .ok_or(GuiInputError::StaleContext)?;
        if output != binding.output {
            return Err(GuiInputError::StaleContext);
        }
        let source = source.unwrap_or(current);
        host.output(source, output)
            .ok_or(GuiInputError::StalePath)?;
        let mut publication = host.publication(source).ok_or(GuiInputError::StalePath)?;
        let mut containing = output;
        for token in path {
            let edge = publication
                .attachments
                .iter()
                .find(|edge| &edge.token == token)
                .ok_or(GuiInputError::StalePath)?;
            check_scope(publication, containing, edge)?;
            publication = host
                .attached_publication(edge)
                .ok_or(GuiInputError::StalePath)?;
            if edge.mode != WorldAttachmentMode::Spatial {
                containing = edge.output.ok_or(GuiInputError::StalePath)?;
            }
        }
        check_target(publication, containing, target)?;
        Ok(Self {
            context,
            source,
            _source_lease: host
                .retain_publication_read(source)
                .ok_or(GuiInputError::StalePath)?,
            path: path.into(),
            target,
        })
    }

    /// Completed root source; commit validation deliberately does not demand latest.
    pub fn source(&self) -> WorldPublicationId {
        self.source
    }

    /// Exact immutable path declarations for the generic command borrow collector.
    pub fn world_references(&self, visit: &mut dyn FnMut(WorldRef)) {
        visit(self.context.root().output.world());
        for token in &self.path {
            visit(token.parent());
            if let Some(child) = token.child() {
                visit(child);
            }
        }
        visit(self.target.world);
    }

    fn validate(&self, view: &HostIngressView<'_>) -> Result<(), GuiInputError> {
        let binding = self.context.root();
        if !self.context.0.live.get()
            || !self.context.0.session.live.get()
            || view.root_binding(binding.output.world()) != Some(binding)
        {
            return Err(GuiInputError::StaleContext);
        }
        let mut publication = view
            .publication(self.source)
            .ok_or(GuiInputError::StalePath)?;
        let mut containing = binding.output;
        view.output(publication.id, containing)
            .ok_or(GuiInputError::StalePath)?;
        live_output(view, containing)?;
        for token in &self.path {
            let world = view
                .world(publication.world)
                .ok_or(GuiInputError::StalePath)?;
            if world.fault().is_some() {
                return Err(GuiInputError::StalePath);
            }
            let edge = publication
                .attachments
                .iter()
                .find(|edge| &edge.token == token)
                .ok_or(GuiInputError::StalePath)?;
            check_scope(publication, containing, edge)?;
            live_policy(world, edge.anchor)?;
            if edge.mode != WorldAttachmentMode::Spatial {
                let Some(ComponentValue::Surface(surface)) =
                    world.effective_component(edge.anchor, ComponentValue::SURFACE)
                else {
                    return Err(GuiInputError::StalePath);
                };
                if ![surface.width, surface.height]
                    .into_iter()
                    .all(|size| size.is_finite() && size > 0.0)
                {
                    return Err(GuiInputError::StalePath);
                }
            }
            publication = view
                .attached_publication(edge)
                .ok_or(GuiInputError::StalePath)?;
            if edge.mode != WorldAttachmentMode::Spatial {
                containing = edge.output.ok_or(GuiInputError::StalePath)?;
                live_output(view, containing)?;
            }
        }
        check_target(publication, containing, self.target)?;
        let world = view
            .world(self.target.world)
            .ok_or(GuiInputError::StalePath)?;
        live_policy(world, self.target.entity)
    }
}

fn live_output(view: &HostIngressView<'_>, output: OutputRef) -> Result<(), GuiInputError> {
    let world = view.world(output.world()).ok_or(GuiInputError::StalePath)?;
    if world.fault().is_some() || !view.output_is_live(output) {
        return Err(GuiInputError::StalePath);
    }
    match output.camera_entity() {
        Some(camera) if !world.entity_link_valid(camera) => Err(GuiInputError::StalePath),
        _ => Ok(()),
    }
}

/// Every entity from `entity` to the top level is live, enabled, visible and
/// hittable; policy scopes end at the World boundary.
fn live_policy(world: SystemWorldView<'_>, entity: crate::EntityId) -> Result<(), GuiInputError> {
    let mut next = Some(entity);
    let mut visited = BTreeSet::new();
    while let Some(entity) = next {
        if !visited.insert(entity) || !world.entity_link_valid(entity) {
            return Err(GuiInputError::StalePath);
        }

        if let Some(ComponentValue::GuiBehavior(behavior)) =
            world.effective_component(entity, ComponentValue::GUI_BEHAVIOR)
            && (!behavior.enabled || !behavior.visible)
        {
            return Err(GuiInputError::StalePath);
        }

        if let Some(ComponentValue::CanvasStyle(style)) =
            world.effective_component(entity, ComponentValue::CANVAS_STYLE)
            && (style.opacity <= 0.0
                || style.scale_x == 0.0
                || style.scale_y == 0.0
                || (style.clipped
                    && (style.clip_max_x <= style.clip_min_x
                        || style.clip_max_y <= style.clip_min_y)))
        {
            return Err(GuiInputError::StalePath);
        }

        next = world
            .entity_link(entity)
            .ok_or(GuiInputError::StalePath)?
            .parent;
    }
    Ok(())
}

fn check_scope(
    publication: &WorldPublication,
    containing: OutputRef,
    edge: &PublishedWorldAttachment,
) -> Result<(), GuiInputError> {
    if edge.token.parent() != publication.world
        || edge
            .placement_output
            .is_some_and(|owner| owner != containing)
    {
        return Err(GuiInputError::StalePath);
    }
    if containing.kind() == OutputKind::Canvas {
        if edge.placement_output != Some(containing) || edge.mode == WorldAttachmentMode::Spatial {
            return Err(GuiInputError::StalePath);
        }
        let canvas = publication
            .output(containing)
            .and_then(|chunk| chunk.data::<CanvasPublication>())
            .ok_or(GuiInputError::StalePath)?;
        let hit = canvas
            .hits
            .iter()
            .find(|hit| {
                matches!(&hit.kind,
            CanvasHitKind::Attachment { token, .. } if token == &edge.token)
            })
            .ok_or(GuiInputError::StalePath)?;
        if !hit.eligible
            || [hit.bounds, hit.clip]
                .iter()
                .any(|bounds| bounds[0] >= bounds[2] || bounds[1] >= bounds[3])
        {
            return Err(GuiInputError::StalePath);
        }
    }
    Ok(())
}

fn check_target(
    publication: &WorldPublication,
    output: OutputRef,
    target: GuiEntityTarget,
) -> Result<(), GuiInputError> {
    if publication.world != target.world
        || output.world() != target.world
        || output.kind() != OutputKind::Canvas
    {
        return Err(GuiInputError::StalePath);
    }
    let gui = publication
        .chunk(CanvasSystem::ID)
        .and_then(|chunk| chunk.data::<GuiCanvasPublication>())
        .and_then(|gui| gui.views.get(&output))
        .ok_or(GuiInputError::StalePath)?;
    let control = gui
        .control(target.canvas_target())
        .ok_or(GuiInputError::StalePath)?;
    if !control.available || !control.record.available || !control.hit.eligible {
        return Err(GuiInputError::StalePath);
    }
    Ok(())
}
