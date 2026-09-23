//! World pass retaining GUI constraint evaluation across frames.
//!
//! [`GuiLayoutSystem`] runs after the GUI state, animation and Surface
//! passes, borrows effective [`GuiRoot`](super::super::GuiRoot) and
//! [`Surface`](crate::systems::surface::Surface) values, and retains one
//! [`GuiEvaluatedView`](super::super::GuiEvaluatedView) per root entity in its
//! [`GuiLayoutCache`](super::super::GuiLayoutCache). Input routing, skinning and
//! semantic readers borrow the retained views through the declared
//! dependency; render preparation consumes retained paint through the
//! internal Surface path. The pass never mutates components, never issues
//! client commands and never advances simulation time.

use super::evaluation::{
    DEFAULT_UNITS_PER_METRE, GuiEvaluatedView, GuiFontResolution, GuiLayoutCache, GuiLayoutRequest,
    GuiResourceResolver,
};
use crate::services::asset_management::font::FontAsset;
use crate::services::asset_management::{AssetManagementService, AssetSource};
use crate::systems::surface::SurfaceRenderResource;
use crate::systems::{
    System, SystemDependency, SystemFactory, SystemId, SystemInitContext, SystemInitError,
};
use crate::{ComponentValue, EntityId, ErrorReason, WorldId};
use std::collections::{BTreeMap, BTreeSet};

/// Retained GUI layout pass. See the module documentation for the pass
/// contract; algorithmic detail lives in [`super::evaluation`].
#[derive(Default)]
pub struct GuiLayoutSystem {
    cache: GuiLayoutCache,
    bindings: crate::systems::SystemBindings<Self>,
    /// Client-owned density contract per root entity: explicit logical units
    /// per Surface metre. Absent entries use [`DEFAULT_UNITS_PER_METRE`];
    /// entries clear with their entity before identity reuse.
    units: BTreeMap<EntityId, f32>,
    /// Entities holding a GuiRoot, in entity order, maintained by the
    /// commit lifecycle so each frame visits roots rather than every entity.
    roots: crate::world::component_query::ComponentQuery<super::super::GuiRoot>,
    /// Roots whose GuiRoot or Surface changed through a commit or numeric
    /// write since their last evaluation. Other retained roots keep their
    /// output without recomputing input fingerprints.
    stale: BTreeSet<EntityId>,
    /// Every root re-evaluates: resource readiness, replacement or release
    /// can change measurement and paint without a component change.
    all_stale: bool,
    /// Count of committed GuiRoot changes per root entity. Numeric writes do
    /// not advance it, so consumers of authored (restored) input can tell a
    /// commit from an animation sample.
    commits: BTreeMap<EntityId, u64>,
}

crate::system_parameter!(super::super::GuiSystem);
crate::system_parameter!(crate::systems::surface::SurfaceSystem);

impl GuiLayoutSystem {
    /// Stable system identity.
    pub const ID: SystemId = SystemId("ipp.gui-layout");

    /// Read-only retained view for one root entity, if ever evaluated.
    /// Returned borrows end before the next update; consumers must not
    /// retain them across frames.
    pub fn view(&self, entity: EntityId) -> Option<&GuiEvaluatedView> {
        self.cache.view(entity)
    }

    /// Current paint revision for one root entity, if ever evaluated.
    pub fn paint_revision(&self, entity: EntityId) -> Option<u64> {
        self.cache.paint_revision(entity)
    }

    /// Every evaluated root entity with its current paint revision, in
    /// entity order. Render preparation consumes this to rebuild Surface
    /// primitives only when retained paint actually changed.
    pub fn paint_states(&self) -> Vec<(EntityId, u64)> {
        let mut states = Vec::new();
        // GuiLayoutCache borrows views by entity; collect identities here
        // so preparation can compare without holding the borrow.
        for entity in self.evaluated_entities() {
            if let Some(revision) = self.cache.paint_revision(entity) {
                states.push((entity, revision));
            }
        }
        states
    }

    /// Every evaluated root entity with the revisions consumers of its
    /// evaluated records key on, in entity order: the content revision,
    /// which follows paint except across in-place translations, and the
    /// count of committed GuiRoot changes.
    pub(crate) fn content_states(&self) -> Vec<(EntityId, u64, u64)> {
        self.evaluated_entities()
            .into_iter()
            .filter_map(|entity| {
                let revision = self.cache.content_revision(entity)?;
                let commits = self.commits.get(&entity).copied().unwrap_or(0);
                Some((entity, revision, commits))
            })
            .collect()
    }

    /// Entities with retained layout output, in ascending order.
    pub fn evaluated_entities(&self) -> Vec<EntityId> {
        self.cache.entities()
    }

    /// Number of roots with retained output.
    pub fn retained_roots(&self) -> usize {
        self.cache.len()
    }

    /// Effective logical units per Surface metre for one root entity: the
    /// client-set value, or [`DEFAULT_UNITS_PER_METRE`] when unset.
    pub fn units_per_metre(&self, entity: EntityId) -> f32 {
        self.units
            .get(&entity)
            .copied()
            .unwrap_or(DEFAULT_UNITS_PER_METRE)
    }

    /// Set the client-owned density contract for one root entity. Finite
    /// positive values only; the change reflows that root on the next
    /// evaluation through the ordinary layout fingerprint.
    pub fn set_units_per_metre(&mut self, entity: EntityId, units: f32) -> Result<(), ErrorReason> {
        if !units.is_finite() || units <= 0.0 {
            return Err(ErrorReason::InvalidValue);
        }
        self.units.insert(entity, units);
        self.stale.insert(entity);
        Ok(())
    }

    /// Evaluate one root against explicit inputs without a World. Test and
    /// tooling seam sharing the retained cache with the scheduled pass,
    /// which re-evaluates the root from World inputs on its next update.
    pub fn evaluate_for_test(
        &mut self,
        entity: EntityId,
        request: &GuiLayoutRequest<'_>,
        resolver: &dyn GuiResourceResolver,
    ) -> &GuiEvaluatedView {
        self.stale.insert(entity);
        self.cache.evaluate(entity, request, resolver)
    }
}

/// Factory for [`GuiLayoutSystem`].
#[derive(Default)]
pub struct GuiLayoutSystemFactory;

impl SystemFactory for GuiLayoutSystemFactory {
    fn id(&self) -> SystemId {
        GuiLayoutSystem::ID
    }

    fn dependencies(&self) -> &[SystemDependency] {
        &[
            SystemDependency::Required(super::super::GuiSystem::ID),
            SystemDependency::After(crate::systems::animation::AnimationSystem::ID),
            SystemDependency::After(crate::systems::surface::SurfaceSystem::ID),
        ]
    }

    fn create(
        &self,
        context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(GuiLayoutSystem {
            cache: GuiLayoutCache::default(),
            bindings: crate::systems::SystemBindings::resolve(context)?,
            units: BTreeMap::new(),
            roots: Default::default(),
            stale: BTreeSet::new(),
            all_stale: true,
            commits: BTreeMap::new(),
        }))
    }
}

impl System for GuiLayoutSystem {
    fn before_numeric_update(&mut self, context: &mut crate::systems::SystemNumericContext<'_>) {
        for &(entity, component) in context.changed_components() {
            if layout_input(component) {
                self.stale.insert(entity);
            }
        }
    }

    fn before_commit(&mut self, context: &mut crate::systems::SystemCommitContext<'_>) {
        for (entity, component) in context.changed_components() {
            if layout_input(component) {
                self.stale.insert(entity);
            }
            if component == ComponentValue::GUI_ROOT {
                let commits = self.commits.entry(entity).or_default();
                *commits = commits.wrapping_add(1);
            }
        }
        self.roots.before_commit(context, ComponentValue::GUI_ROOT);
    }

    fn before_asset_release(
        &mut self,
        _context: &mut crate::systems::SystemAssetContext<'_>,
        _event: &crate::services::asset_management::AssetLifecycleEvent,
    ) {
        self.all_stale = true;
    }

    fn asset_lifecycle(
        &mut self,
        _context: &mut crate::systems::SystemAssetContext<'_>,
        _event: &crate::services::asset_management::AssetLifecycleEvent,
    ) {
        self.all_stale = true;
    }

    fn after_commit(&mut self, context: &mut crate::systems::SystemCommitContext<'_>) {
        self.roots.after_commit(
            context,
            ComponentValue::GUI_ROOT,
            crate::components::registry::ComponentStorage::gui_root_ptr,
        );
    }

    crate::system_update!(bindings);
}

crate::system_parameter!(GuiLayoutSystem);

/// Host asset lookup bridging component asset references to the immutable
/// resources measurement and paint need. Missing registrations resolve to
/// [`GuiFontResolution::Missing`]; registered but undecoded assets resolve
/// to [`GuiFontResolution::Pending`]. Metrics are never guessed.
struct SystemResolver<'a> {
    world: WorldId,
    assets: &'a AssetManagementService,
}

impl GuiResourceResolver for SystemResolver<'_> {
    fn text_font(&self, source: &AssetSource) -> GuiFontResolution<'_> {
        let Some(key) =
            self.assets
                .find_source(self.world, source.kind, &source.uri, source.variant)
        else {
            return GuiFontResolution::Missing;
        };

        match self
            .assets
            .get(key)
            .and_then(|resource| resource.data())
            .and_then(|asset| asset.decoded().downcast_ref::<FontAsset>())
        {
            Some(font) => GuiFontResolution::Ready {
                key,
                font,
            },
            None => GuiFontResolution::Pending {
                key,
            },
        }
    }

    fn surface_resource(&self, source: &AssetSource) -> Option<SurfaceRenderResource> {
        let key = self
            .assets
            .find_source(self.world, source.kind, &source.uri, source.variant)?;
        self.assets.get(key)?.data()?;
        Some(SurfaceRenderResource {
            key,
            source: source.clone(),
        })
    }

    /// Slot generation while decoded data backs retained output. Pending
    /// or missing sources report nothing, so a readiness flip rebuilds
    /// only affected branches through the layout fingerprint; replacement
    /// and recovery flow through the same signal without authored edits.
    fn resource_generation(&self, source: &AssetSource) -> Option<u64> {
        let key = self
            .assets
            .find_source(self.world, source.kind, &source.uri, source.variant)?;
        self.assets
            .get(key)?
            .data()
            .map(|_| u64::from(key.generation))
    }
}

#[crate::systems::system_update(
    SystemDependency::Required(super::super::GuiSystem::ID),
    SystemDependency::After(crate::systems::animation::AnimationSystem::ID),
    SystemDependency::After(crate::systems::surface::SurfaceSystem::ID)
)]
impl GuiLayoutSystem {
    fn update(
        &mut self,
        ecs: crate::systems::SystemEcsAccess<'_>,
        assets: &crate::services::asset_management::AssetManagementService,
        _state: &super::super::GuiSystem,
        _animation: &crate::systems::animation::AnimationSystem,
        _surface: &crate::systems::surface::SurfaceSystem,
        _dt: f64,
    ) {
        let world_id = ecs.id();
        let tick = ecs.world.tick;
        let resolver = SystemResolver {
            world: world_id,
            assets,
        };
        self.roots.prepare(
            ecs.world,
            crate::components::registry::ComponentStorage::gui_root_ptr,
        );

        let mut live = BTreeSet::new();
        for &(entity, _) in self.roots.entries() {
            let index = entity.index() as usize;
            let (Some(surface), Some(root)) = (
                ecs.world.components.surface(index),
                ecs.world.components.gui_root(index),
            ) else {
                continue;
            };
            // Raw-authored Surfaces never carry layout output; the content
            // owner rule keeps the domains disjoint.
            if !surface.items().is_empty() {
                continue;
            }

            let root_incarnation = ecs
                .world
                .state
                .entities
                .get(&entity)
                .and_then(|record| record.input(ComponentValue::GUI_ROOT))
                .map(|input| input.incarnation)
                .unwrap_or(0);
            live.insert(entity);

            // Unchanged inputs keep retained output; only the evaluation
            // tick advances, as it would on a no-op refresh.
            if !self.all_stale
                && !self.stale.contains(&entity)
                && self.cache.is_current(entity, root_incarnation)
            {
                self.cache.touch(entity, tick);
                continue;
            }

            let request = GuiLayoutRequest {
                root,
                root_incarnation,
                surface_size: [surface.width, surface.height],
                units_per_metre: self
                    .units
                    .get(&entity)
                    .copied()
                    .unwrap_or(DEFAULT_UNITS_PER_METRE),
                evaluation_tick: tick,
            };
            self.cache.evaluate(entity, &request, &resolver);
        }

        // Entities and components that went away invalidate their retained
        // output before any identity can be reused.
        self.cache.retain_entities(&live);
        self.units.retain(|entity, _| live.contains(entity));
        self.commits.retain(|entity, _| live.contains(entity));
        self.stale.clear();
        self.all_stale = false;
    }
}

/// Components whose changes can alter a root's evaluated layout or paint.
fn layout_input(component: u16) -> bool {
    component == ComponentValue::GUI_ROOT || component == ComponentValue::SURFACE
}

impl crate::WorldContext<'_> {
    /// Set the client-owned GUI density contract for one root entity.
    /// Finite positive values only; unset roots keep the default. The change
    /// reflows that root on the next evaluation; React and host clients own
    /// the per-display value behind this setter.
    pub fn set_gui_units_per_metre(
        &mut self,
        entity: EntityId,
        units: f32,
    ) -> Result<(), ErrorReason> {
        if !units.is_finite() || units <= 0.0 {
            return Err(ErrorReason::InvalidValue);
        }
        self.with_system::<GuiLayoutSystem, _>(GuiLayoutSystem::ID, |system, _| {
            system.units.insert(entity, units);
            system.stale.insert(entity);
        });
        Ok(())
    }

    /// Effective GUI density contract for one root entity: the client-set
    /// value, or the default when unset.
    pub fn gui_units_per_metre(&self, entity: EntityId) -> f32 {
        self.system::<GuiLayoutSystem>(GuiLayoutSystem::ID)
            .map(|system| system.units_per_metre(entity))
            .unwrap_or(DEFAULT_UNITS_PER_METRE)
    }
}

#[cfg(test)]
#[path = "system_tests.rs"]
mod tests;
