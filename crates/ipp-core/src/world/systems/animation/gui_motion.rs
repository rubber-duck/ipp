use super::driver::make_driver;
use super::system_state::AnimationRuntimeFrozenTransitionValue;
use super::transition::AnimationTransitionProgram;
use super::*;
use crate::components::schema::FieldValue;
use crate::services::asset_management::{
    AssetKey, AssetLifecycleEvent, AssetLifecycleKind, AssetSource,
};
use crate::systems::gui::motion::{
    GuiMotionDestination, GuiMotionOwner, GuiMotionRequest, GuiMotionSkinTarget,
    GuiSkinMotionStatus,
};
use crate::systems::{SystemCommitContext, SystemRuntimeAccess};
use crate::{ComponentValue, DynamicValue};
use std::collections::{BTreeMap, BTreeSet};

struct GuiMotionBinding {
    request: GuiMotionRequest,
    source: AssetKey,
    prepared: Option<(AnimationController, AnimationTransitionProgram)>,
    origin: Vec<AnimationRuntimeFrozenTransitionValue>,
    elapsed: f64,
    settled: bool,
}

struct GuiMotionDependency {
    source: Option<AssetSource>,
    declared_theme: EntityId,
    revoked: bool,
}

#[derive(Default)]
pub(super) struct GuiMotionAnimations {
    bindings: BTreeMap<GuiMotionOwner, GuiMotionBinding>,
    bound: BTreeMap<AssetKey, BTreeSet<GuiMotionOwner>>,
    requested: BTreeMap<GuiMotionOwner, GuiMotionDependency>,
    themes: BTreeMap<EntityId, BTreeSet<GuiMotionOwner>>,
    requested_keys: BTreeMap<AssetKey, BTreeSet<GuiMotionOwner>>,
    owner_keys: BTreeMap<GuiMotionOwner, AssetKey>,
    unresolved: BTreeSet<GuiMotionOwner>,
    entities: BTreeMap<EntityId, BTreeSet<GuiMotionOwner>>,
    active: BTreeSet<GuiMotionOwner>,
    wake: BTreeSet<GuiMotionOwner>,
    refresh: BTreeSet<GuiMotionOwner>,
    demand_dirty: bool,
    #[cfg(feature = "diagnostics")]
    pub statistics: crate::systems::gui::motion::GuiMotionSamplingWork,
}

impl GuiMotionAnimations {
    fn remove_binding(&mut self, owner: GuiMotionOwner) {
        if let Some(binding) = self.bindings.remove(&owner) {
            if let Some(owners) = self.bound.get_mut(&binding.source) {
                owners.remove(&owner);
                if owners.is_empty() {
                    self.bound.remove(&binding.source);
                }
            }

            self.demand_dirty = true;
        }
    }

    fn remove_request(&mut self, owner: GuiMotionOwner) {
        if let Some(dependency) = self.requested.remove(&owner)
            && let Some(owners) = self.themes.get_mut(&dependency.declared_theme)
        {
            owners.remove(&owner);
            if owners.is_empty() {
                self.themes.remove(&dependency.declared_theme);
            }
        }
        self.remove_source_request(owner);

        if let Some(owners) = self.entities.get_mut(&owner.entity) {
            owners.remove(&owner);
            if owners.is_empty() {
                self.entities.remove(&owner.entity);
            }
        }

        self.active.remove(&owner);
        self.wake.remove(&owner);
        self.refresh.remove(&owner);
    }

    fn remove_source_request(&mut self, owner: GuiMotionOwner) {
        self.unresolved.remove(&owner);
        if let Some(key) = self.owner_keys.remove(&owner)
            && let Some(owners) = self.requested_keys.get_mut(&key)
        {
            owners.remove(&owner);
            if owners.is_empty() {
                self.requested_keys.remove(&key);
            }
        }
    }

    pub fn before_commit(&mut self, context: &SystemCommitContext<'_>) {
        let mut invalid = BTreeSet::new();
        for (entity, component) in context.changed_components() {
            if component != ComponentValue::GUI_SKIN
                || !context.retains_component(entity, component)
            {
                continue;
            }
            let Some((target, theme)) = GuiMotionSkinTarget::declaration_at_commit(context, entity)
            else {
                continue;
            };
            let owners: Vec<_> = self
                .entities
                .get(&entity)
                .into_iter()
                .flatten()
                .copied()
                .collect();
            for owner in owners {
                if owner.skin_incarnation != target.incarnation
                    || !crate::systems::gui::motion::theme_live(context.staged, theme)
                {
                    invalid.insert(owner);
                } else {
                    self.set_declared_theme(owner, theme);
                }
            }
        }

        for (entity, component) in context.changed_components() {
            if matches!(
                component,
                ComponentValue::GUI_THEME | ComponentValue::GUI_THEME_MOTION
            ) && !crate::systems::gui::motion::theme_live(context.staged, entity)
            {
                invalid.extend(self.themes.get(&entity).into_iter().flatten().copied());
            }

            if context.retains_component(entity, component) {
                continue;
            }

            invalid.extend(
                self.entities
                    .get(&entity)
                    .into_iter()
                    .flatten()
                    .filter(|owner| {
                        component == ComponentValue::GUI_SKIN || component == owner.control
                    })
                    .copied(),
            );
        }

        for owner in invalid {
            self.remove_binding(owner);
            self.remove_request(owner);
        }
    }

    fn set_declared_theme(&mut self, owner: GuiMotionOwner, theme: EntityId) {
        let dependency = self
            .requested
            .get_mut(&owner)
            .expect("indexed motion owner");
        if dependency.declared_theme == theme {
            return;
        }

        let previous = dependency.declared_theme;
        dependency.declared_theme = theme;
        dependency.source = None;
        dependency.revoked = false;
        self.remove_source_request(owner);
        self.active.remove(&owner);
        self.wake.remove(&owner);
        if let Some(owners) = self.themes.get_mut(&previous) {
            owners.remove(&owner);
            if owners.is_empty() {
                self.themes.remove(&previous);
            }
        }
        self.themes.entry(theme).or_default().insert(owner);
    }

    pub fn reconcile_sources(
        &mut self,
        context: &SystemRuntimeAccess<'_>,
        gui: Option<crate::systems::SystemDependencyBinding<crate::systems::gui::GuiSystem>>,
        event: &AssetLifecycleEvent,
    ) {
        if self.requested.is_empty() || event.kind == AssetLifecycleKind::GraphicsInvalidated {
            return;
        }
        let Some(gui) = gui.and_then(|binding| context.dependency(binding)) else {
            return;
        };
        let mut owners = BTreeSet::new();
        for entity in gui.pending_motion_entities() {
            owners.extend(self.entities.get(&entity).into_iter().flatten().copied());
        }
        owners.extend(
            self.requested_keys
                .get(&event.key)
                .into_iter()
                .flatten()
                .copied(),
        );
        owners.extend(self.bound.get(&event.key).into_iter().flatten().copied());
        for owner in owners {
            let source = gui.motion_request_source(context.world, owner);
            let Some(dependency) = self.requested.get_mut(&owner) else {
                continue;
            };
            if dependency.source == source {
                continue;
            }
            dependency.source.clone_from(&source);
            dependency.revoked = false;
            self.remove_source_request(owner);
            if let Some(source) = source {
                if let Some(key) = context.asset_acquisition.find_source(
                    context.world.id,
                    source.kind,
                    &source.uri,
                    source.variant,
                ) {
                    self.owner_keys.insert(owner, key);
                    self.requested_keys.entry(key).or_default().insert(owner);
                } else {
                    self.unresolved.insert(owner);
                }
            }
            self.active.remove(&owner);
            self.wake.remove(&owner);
            self.refresh.insert(owner);
        }
    }

    pub fn release(&mut self, context: &mut SystemRuntimeAccess<'_>, event: &AssetLifecycleEvent) {
        if event.kind == AssetLifecycleKind::GraphicsInvalidated {
            return;
        }

        self.resolve_waiting(context, event.key);
        let bound = self.bound.get(&event.key).cloned().unwrap_or_default();
        let requested = self
            .requested_keys
            .get(&event.key)
            .cloned()
            .unwrap_or_default();
        for owner in bound.union(&requested).copied() {
            let requested_removed =
                requested.contains(&owner) && event.kind == AssetLifecycleKind::Removed;
            if bound.contains(&owner) {
                if let Some(binding) = self.bindings.get_mut(&owner) {
                    binding.prepared = None;
                }

                if event.kind == AssetLifecycleKind::Removed {
                    self.remove_binding(owner);
                }
            }

            if requested_removed {
                self.remove_binding(owner);
                self.requested
                    .get_mut(&owner)
                    .expect("indexed request")
                    .revoked = true;
            }

            if let Some(channels) = channels_mut(context, owner) {
                if bound.contains(&owner) || requested_removed {
                    channels.active = false;
                }

                if requested_removed {
                    channels.status = GuiSkinMotionStatus::Unavailable;
                } else if channels.status != GuiSkinMotionStatus::Unavailable {
                    channels.status = GuiSkinMotionStatus::Pending;
                }
            }

            self.active.remove(&owner);
            if !requested.contains(&owner) {
                self.wake.insert(owner);
            }

            if requested.contains(&owner) && event.kind == AssetLifecycleKind::Removed {
                self.owner_keys.remove(&owner);
                self.unresolved.insert(owner);
            }
            notify(context, owner);
        }

        if event.kind == AssetLifecycleKind::Removed {
            self.requested_keys.remove(&event.key);
        }
        self.flush_demand(context.world.id, context.asset_acquisition);
    }

    pub fn asset_lifecycle(
        &mut self,
        context: &mut SystemRuntimeAccess<'_>,
        event: &AssetLifecycleEvent,
    ) {
        if event.kind != AssetLifecycleKind::StatusChanged {
            return;
        }

        self.resolve_waiting(context, event.key);
        if let Some(owners) = self.requested_keys.get(&event.key) {
            for &owner in owners {
                if let Some(channels) = channels_mut(context, owner)
                    && channels.status == GuiSkinMotionStatus::Unavailable
                {
                    channels.status = GuiSkinMotionStatus::Pending;
                }

                self.wake.insert(owner);
            }
        }
    }

    fn resolve_waiting(&mut self, context: &SystemRuntimeAccess<'_>, key: AssetKey) {
        let resolved: Vec<_> = self
            .unresolved
            .iter()
            .filter_map(|owner| {
                let source = self.requested.get(owner)?.source.as_ref()?;
                (context.asset_acquisition.find_source(
                    context.world.id,
                    source.kind,
                    &source.uri,
                    source.variant,
                ) == Some(key))
                .then_some(*owner)
            })
            .collect();
        for owner in resolved {
            self.unresolved.remove(&owner);
            self.owner_keys.insert(owner, key);
            self.requested_keys.entry(key).or_default().insert(owner);
            self.requested
                .get_mut(&owner)
                .expect("unresolved request")
                .revoked = false;
        }
    }

    pub fn update(
        &mut self,
        context: &mut SystemRuntimeAccess<'_>,
        dt: f64,
        changes: &[GuiMotionOwner],
    ) {
        #[cfg(feature = "diagnostics")]
        {
            self.statistics = Default::default();
        }

        let mut changes: BTreeSet<_> = changes.iter().copied().collect();
        changes.append(&mut self.refresh);
        for owner in changes {
            let Some(channels) = channels(context, owner) else {
                self.remove_request(owner);
                self.remove_binding(owner);
                continue;
            };
            let source = channels.request.source.clone();
            let theme = channels.request.theme;
            let mut revoked = self.requested.get(&owner).is_some_and(|dependency| {
                dependency.source.as_ref() == Some(&source) && dependency.revoked
            });
            self.remove_request(owner);
            if let Some(key) = context.asset_acquisition.find_source(
                context.world.id,
                source.kind,
                &source.uri,
                source.variant,
            ) {
                self.owner_keys.insert(owner, key);
                self.requested_keys.entry(key).or_default().insert(owner);
                revoked = false;
            } else {
                self.unresolved.insert(owner);
            }

            self.requested.insert(
                owner,
                GuiMotionDependency {
                    source: Some(source),
                    declared_theme: theme,
                    revoked,
                },
            );
            self.themes.entry(theme).or_default().insert(owner);
            self.entities.entry(owner.entity).or_default().insert(owner);
            self.wake.insert(owner);
            if revoked && let Some(channels) = channels_mut(context, owner) {
                channels.status = GuiSkinMotionStatus::Unavailable;
            }
        }

        let mut work = std::mem::take(&mut self.active);
        work.append(&mut self.wake);
        for owner in work {
            #[cfg(feature = "diagnostics")]
            {
                self.statistics.owners += 1;
            }

            let Some(channels) = channels(context, owner) else {
                continue;
            };
            if channels.status == GuiSkinMotionStatus::Unavailable {
                continue;
            }

            let changed = self
                .bindings
                .get(&owner)
                .is_none_or(|binding| binding.request != channels.request);
            let suspended = self
                .bindings
                .get(&owner)
                .is_some_and(|binding| binding.prepared.is_none());
            if changed || suspended {
                #[cfg(feature = "diagnostics")]
                {
                    self.statistics.bindings += 1;
                }

                let request = channels.request.clone();
                let origin = match self.bindings.get(&owner) {
                    Some(binding) if !changed => Ok(Some(binding.origin.clone())),
                    Some(binding) => binding
                        .prepared
                        .as_ref()
                        .map(|(_, program)| program.freeze(None))
                        .transpose(),
                    None => Ok(None),
                };
                let result = origin.and_then(|origin| build(context, &request, origin));
                match result {
                    Ok(Some(mut binding)) => {
                        if !changed && let Some(previous) = self.bindings.get(&owner) {
                            binding.elapsed = previous.elapsed;
                        }

                        if let Some(channels) = channels_mut(context, owner) {
                            let mut appearance = request.appearance.clone();
                            if binding.elapsed < request.duration
                                && let Some(ready) = &channels.ready_appearance
                            {
                                appearance.asset.clone_from(&ready.asset);
                            }
                            channels.ready_appearance = Some(appearance);
                        }

                        self.remove_binding(owner);
                        self.bound.entry(binding.source).or_default().insert(owner);
                        self.bindings.insert(owner, binding);
                        self.demand_dirty = true;
                    }
                    Ok(None) => {
                        if let Some(channels) = channels_mut(context, owner) {
                            channels.status = GuiSkinMotionStatus::Pending;
                        }
                        continue;
                    }
                    Err(_reason) => {
                        crate::diagnostic!(
                            Warn,
                            "[IPP core] gui_motion.reject entity={:?} part={} reason={_reason}",
                            owner.entity,
                            owner.part
                        );
                        self.remove_binding(owner);
                        self.demand_dirty = true;
                        notify(context, owner);
                        if let Some(channels) = channels_mut(context, owner) {
                            channels.active = false;
                            channels.status = GuiSkinMotionStatus::Unavailable;
                        }
                        continue;
                    }
                }
            }

            let Some(binding) = self.bindings.get_mut(&owner) else {
                continue;
            };
            if !binding.settled {
                binding.elapsed = (binding.elapsed + dt).min(binding.request.duration);
                let raw = if binding.request.duration == 0.0 {
                    1.0
                } else {
                    binding.elapsed / binding.request.duration
                };
                let progress = match binding.request.easing {
                    AnimationTransitionEasing::Linear => raw,
                    AnimationTransitionEasing::Smoothstep => raw * raw * (3.0 - 2.0 * raw),
                };
                let Some((controller, program)) = &mut binding.prepared else {
                    continue;
                };
                #[cfg(feature = "diagnostics")]
                {
                    self.statistics.samples += 1;
                }
                notify(context, owner);
                if program
                    .evaluate(
                        None,
                        &controller.drivers,
                        controller.snapshot.time,
                        &mut Default::default(),
                        progress,
                        &mut context.world.components,
                    )
                    .is_err()
                {
                    if let Some(channels) = channels_mut(context, owner) {
                        channels.active = false;
                        channels.status = GuiSkinMotionStatus::Unavailable;
                    }

                    self.remove_binding(owner);
                    self.demand_dirty = true;
                    continue;
                }
                binding.settled = binding.elapsed >= binding.request.duration;
            }

            if let Some(channels) = channels_mut(context, owner) {
                if binding.settled
                    && let Some(ready) = &mut channels.ready_appearance
                    && ready.asset != binding.request.appearance.asset
                {
                    ready.asset.clone_from(&binding.request.appearance.asset);
                }
                channels.active = true;
                channels.live_arity = binding.request.values.len();
                channels.status = GuiSkinMotionStatus::Ready;
            }

            if !binding.settled {
                self.active.insert(owner);
            }
        }

        self.flush_demand(context.world.id, context.asset_acquisition);
    }

    pub fn flush_demand(
        &mut self,
        world: crate::WorldId,
        assets: &mut crate::services::asset_management::AssetManagementService,
    ) {
        if self.demand_dirty {
            let demand = self
                .bound
                .values()
                .filter_map(|owners| owners.first().and_then(|owner| self.bindings.get(owner)))
                .map(|binding| {
                    crate::services::asset_management::service::AssetDemandSelection::new(
                        ANIMATION_TYPE,
                        &binding.request.source.uri,
                        binding.request.source.variant,
                    )
                })
                .collect();
            assets.update_system_users(world, AnimationSystem::ID.0, demand);
            self.demand_dirty = false;
        }
    }
}

fn channels<'a>(
    context: &'a SystemRuntimeAccess<'_>,
    owner: GuiMotionOwner,
) -> Option<&'a crate::systems::gui::motion::GuiMotionChannels> {
    owner
        .skin(context.world)?
        .get(context.world)?
        .runtime
        .parts
        .get(&owner.part)
        .filter(|channels| channels.request.owner == owner)
}

fn channels_mut<'a>(
    context: &'a mut SystemRuntimeAccess<'_>,
    owner: GuiMotionOwner,
) -> Option<&'a mut crate::systems::gui::motion::GuiMotionChannels> {
    owner
        .skin(context.world)?
        .get_mut(context.world)?
        .runtime
        .parts
        .get_mut(&owner.part)
        .filter(|channels| channels.request.owner == owner)
}

fn notify(context: &mut SystemRuntimeAccess<'_>, owner: GuiMotionOwner) {
    let Some(target) = owner.skin(context.world) else {
        return;
    };
    let Some(skin) = target.get_mut(context.world) else {
        return;
    };
    skin.runtime.notifying_sample = true;
    context.before_numeric_update(&[(owner.entity, ComponentValue::GUI_SKIN)]);
    if let Some(skin) = target.get_mut(context.world) {
        skin.runtime.notifying_sample = false;
    }
}

fn build(
    context: &SystemRuntimeAccess<'_>,
    request: &GuiMotionRequest,
    origin: Option<Vec<AnimationRuntimeFrozenTransitionValue>>,
) -> Result<Option<GuiMotionBinding>, ErrorReason> {
    let Some(source) = context.asset_acquisition.find_source(
        context.world.id,
        ANIMATION_TYPE,
        &request.source.uri,
        request.source.variant,
    ) else {
        return Ok(None);
    };
    let resource = context
        .asset_acquisition
        .get(source)
        .ok_or(ErrorReason::InvalidAsset)?;
    if matches!(
        resource.status(),
        crate::services::asset_management::AssetLoadStatus::Failed(_)
    ) {
        return Err(ErrorReason::InvalidAsset);
    }

    let Some(clip) = context.asset_acquisition.get_typed::<AnimationClip>(source) else {
        return if resource.data().is_some() {
            Err(ErrorReason::InvalidAsset)
        } else {
            Ok(None)
        };
    };
    if request.time > clip.duration() {
        return Err(ErrorReason::InvalidValue);
    }

    let mut drivers = Vec::with_capacity(request.values.len());
    let mut descriptions = Vec::with_capacity(request.values.len());
    let mut frozen = Vec::with_capacity(request.values.len());
    for (channel, value) in request.values.iter().enumerate() {
        let offset = GuiMotionDestination::offset(request.owner.part, channel);
        let property = AnimationTrackTarget::AnimationProperty(AnimationProperty {
            component: ComponentValue::GUI_SKIN,
            offsets: vec![offset],
        });
        let track = request
            .track
            .checked_add(channel as u32)
            .ok_or(ErrorReason::InvalidField)?;
        let data = clip
            .tracks()
            .get(track as usize)
            .ok_or(ErrorReason::InvalidField)?;
        let sampled = data.sample_value(request.time);
        let AnimationValue::Field(FieldValue::Dynamic(sampled)) = sampled else {
            return Err(ErrorReason::InvalidField);
        };
        GuiMotionDestination::validate(offset, &sampled)?;
        if !skin_samples_match(&sampled, value) {
            return Err(ErrorReason::InvalidValue);
        }

        let description = AnimationDriverDescription {
            source: request.source.uri.clone(),
            variant: request.source.variant,
            track,
            target: request.owner.entity,
            property: property.clone(),
            entity_bindings: Vec::new(),
            weight: 1.0,
            additive: false,
            reference_time: 0.0,
            repeat: false,
        };
        let baseline =
            AnimationValue::Field(FieldValue::Dynamic(if channel == 1 && !request.visible {
                DynamicValue::F32(0.0)
            } else {
                value.clone()
            }));
        let mut driver = make_driver(
            description.clone(),
            request.owner.skin_incarnation,
            property.clone(),
            source,
            clip.duration(),
            baseline.clone(),
            #[cfg(feature = "skeletal-animation")]
            None,
        )?;
        driver.resolve_track(clip)?;
        drivers.push(driver);
        descriptions.push(description);
        frozen.push(AnimationRuntimeFrozenTransitionValue {
            target: request.owner.entity,
            incarnation: request.owner.skin_incarnation,
            property,
            value: baseline.clone(),
            baseline,
        });
    }

    let initial = origin.is_none();
    let mut controller = AnimationController {
        snapshot: AnimationControllerSnapshot {
            id: AnimationControllerId::from_bits(0),
            description: AnimationControllerDescription {
                drivers: descriptions,
                speed: 0.0,
                looping: false,
            },
            state: AnimationPlaybackStatus::Playing,
            time: request.time,
            transition: None,
        },
        drivers,
        structural_drivers: Vec::new(),
        driver_targets: BTreeMap::new(),
        incarnations: Vec::new(),
        sought: false,
        directional_start_pending: false,
        duration: clip.duration(),
        ready: true,
        numeric_targets: Vec::new(),
        discrete_drivers: Vec::new(),
        numeric_outputs: Vec::new(),
        failure: None,
        transition: None,
        contributions: Default::default(),
    };
    let mut origin = origin.unwrap_or(frozen);
    origin.retain(|value| match &value.property {
        AnimationTrackTarget::AnimationProperty(property) => {
            property.offsets[0]
                < GuiMotionDestination::offset(request.owner.part, request.values.len())
        }
        _ => false,
    });
    let mut program = AnimationTransitionProgram::bind_frozen_values(
        &mut controller,
        &origin,
        &context.world.components,
        false,
    )?;
    if !request.visible {
        program.set_constant_destination(
            request.owner.entity,
            request.owner.skin_incarnation,
            AnimationTrackTarget::AnimationProperty(AnimationProperty {
                component: ComponentValue::GUI_SKIN,
                offsets: vec![GuiMotionDestination::offset(request.owner.part, 1)],
            }),
            AnimationValue::Field(FieldValue::Dynamic(DynamicValue::F32(0.0))),
        )?;
    }
    Ok(Some(GuiMotionBinding {
        request: request.clone(),
        source,
        prepared: Some((controller, program)),
        origin,
        elapsed: if initial {
            request.duration
        } else {
            0.0
        },
        settled: false,
    }))
}

fn skin_samples_match(actual: &crate::DynamicValue, expected: &crate::DynamicValue) -> bool {
    const EPSILON: f32 = 1.0e-5;

    let components_match = |actual: &[f32], expected: &[f32]| {
        actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| (*actual - *expected).abs() <= EPSILON)
    };
    match (actual, expected) {
        (crate::DynamicValue::F32(actual), crate::DynamicValue::F32(expected)) => {
            (*actual - *expected).abs() <= EPSILON
        }
        (crate::DynamicValue::Vec2(actual), crate::DynamicValue::Vec2(expected)) => {
            components_match(actual, expected)
        }
        (crate::DynamicValue::Vec4(actual), crate::DynamicValue::Vec4(expected)) => {
            components_match(actual, expected)
        }
        _ => false,
    }
}
