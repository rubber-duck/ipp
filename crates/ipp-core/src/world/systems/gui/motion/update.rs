use super::runtime::{GuiMotionChannels, GuiMotionOwner, GuiMotionRequest, GuiMotionSkinTarget};
use super::{GuiSkinMotionStatus, GuiThemeMotion};
use crate::systems::animation::AnimationTransitionEasing;
use crate::systems::gui::local::GuiLocalState;
use crate::systems::gui::local::control::{GuiControl, eligibility, entity_control};
use crate::systems::gui::{GuiPartId, GuiPartVariant, GuiPrimitivePart, GuiSkinState};
use crate::systems::{SystemCommitContext, SystemRuntimeAccess};
use crate::{ComponentValue, DynamicValue, EntityId};
use std::collections::{BTreeMap, BTreeSet};

struct GuiMotionMembership {
    target: GuiMotionSkinTarget,
    theme: EntityId,
}

#[derive(Default)]
pub(in crate::world::systems::gui) struct GuiMotionState {
    initialized: bool,
    skins: BTreeMap<EntityId, GuiMotionMembership>,
    users: BTreeMap<EntityId, BTreeSet<EntityId>>,
    membership: BTreeSet<EntityId>,
    dirty: BTreeSet<EntityId>,
    theme_dirty: BTreeSet<EntityId>,
    branches: BTreeSet<EntityId>,
    ancestors: BTreeMap<EntityId, BTreeSet<EntityId>>,
    dependents: BTreeMap<EntityId, BTreeSet<EntityId>>,
    watched: BTreeSet<EntityId>,
    owners: BTreeMap<EntityId, Vec<GuiMotionOwner>>,
    invalidated: BTreeSet<EntityId>,
    withdrawn: BTreeSet<EntityId>,
    paint_dirty: BTreeMap<EntityId, GuiMotionSkinTarget>,
    changed: Vec<GuiMotionOwner>,
    #[cfg(feature = "diagnostics")]
    pub statistics: super::GuiMotionPreparationWork,
}

impl GuiMotionState {
    #[cfg(test)]
    pub(in crate::world::systems::gui) fn index_counts(&self) -> [usize; 12] {
        [
            self.skins.len(),
            self.users.len(),
            self.ancestors.len(),
            self.dependents.len(),
            self.watched.len(),
            self.owners.len(),
            self.membership.len(),
            self.dirty.len(),
            self.branches.len(),
            self.invalidated.len(),
            self.withdrawn.len(),
            self.paint_dirty.len(),
        ]
    }

    pub fn changes(&self) -> &[GuiMotionOwner] {
        &self.changed
    }

    pub fn pending_entities(&self) -> BTreeSet<EntityId> {
        let mut pending = self.dirty.clone();
        pending.extend(&self.membership);
        pending.extend(&self.watched);
        for theme in &self.theme_dirty {
            pending.extend(self.users.get(theme).into_iter().flatten().copied());
        }
        for entity in &self.branches {
            if self.skins.contains_key(entity) {
                pending.insert(*entity);
            }
            pending.extend(self.dependents.get(entity).into_iter().flatten().copied());
        }
        pending
    }

    pub fn request_source(
        local: &GuiLocalState,
        world: &crate::world::WorldSimulationState,
        owner: GuiMotionOwner,
    ) -> Option<crate::services::asset_management::AssetSource> {
        let skin = owner.skin(world)?.get(world)?;
        if !super::theme_live(&world.state, skin.theme) {
            return None;
        }
        let control = GuiMotionControl::read(local, world, owner.entity)?;
        if control.control.target.component != owner.control
            || control.control.target.incarnation != owner.control_incarnation
        {
            return None;
        }
        let theme = world.components.gui_theme(skin.theme.index() as usize)?;
        let motion = world
            .components
            .gui_theme_motion(skin.theme.index() as usize)?;
        resolve_part(
            skin,
            theme,
            motion,
            &control,
            local.focus_visible(control.control.target),
            owner,
        )
        .map(|request| request.source)
    }

    pub fn dirty_entity(&mut self, entity: EntityId) {
        self.dirty.insert(entity);
    }

    pub fn watch(&mut self, entity: EntityId) {
        self.watched.insert(entity);
    }

    pub fn dirty_watched(&mut self) {
        self.dirty.extend(self.watched.iter().copied());
    }

    pub fn before_commit(&mut self, context: &SystemCommitContext<'_>) {
        for (entity, component) in context.changed_components() {
            if component != ComponentValue::GUI_SKIN {
                continue;
            }
            if !context.retains_component(entity, component) {
                self.retire(entity);
            }
            if let Some((target, theme)) =
                GuiMotionSkinTarget::declaration_at_commit(context, entity)
            {
                if self
                    .skins
                    .get(&entity)
                    .is_some_and(|previous| previous.theme != theme)
                {
                    self.invalidated.insert(entity);
                }
                self.set_membership(entity, target, theme);
                self.dirty.insert(entity);
                self.withdrawn.remove(&entity);
                if !super::theme_live(context.staged, theme) {
                    self.withdrawn.insert(entity);
                }
            }
        }

        for (entity, component) in context.changed_components() {
            if component == ComponentValue::GUI_SKIN {
                continue;
            }
            if matches!(
                component,
                ComponentValue::GUI_THEME | ComponentValue::GUI_THEME_MOTION
            ) {
                self.theme_dirty.insert(entity);
                if !super::theme_live(context.staged, entity) {
                    self.withdrawn
                        .extend(self.users.get(&entity).into_iter().flatten().copied());
                }
                continue;
            }

            if !context.staged.entities.contains_key(&entity) {
                continue;
            }

            if !context.retains_component(entity, component)
                && self
                    .owners
                    .get(&entity)
                    .is_some_and(|owners| owners.iter().any(|owner| owner.control == component))
            {
                self.invalidated.insert(entity);
            }

            self.component_changed(entity, component);
        }

        self.branches
            .extend(context.changed_entity_links().filter(|entity| {
                context.staged.entities.contains_key(entity) || self.dependents.contains_key(entity)
            }));
    }

    fn remove_membership(&mut self, entity: EntityId) {
        if let Some(membership) = self.skins.remove(&entity)
            && let Some(users) = self.users.get_mut(&membership.theme)
        {
            users.remove(&entity);
            if users.is_empty() {
                self.users.remove(&membership.theme);
            }
        }
    }

    fn set_membership(&mut self, entity: EntityId, target: GuiMotionSkinTarget, theme: EntityId) {
        if self.skins.get(&entity).is_some_and(|previous| {
            previous.target.incarnation == target.incarnation && previous.theme == theme
        }) {
            return;
        }

        self.remove_membership(entity);
        self.skins.insert(
            entity,
            GuiMotionMembership {
                target,
                theme,
            },
        );
        self.users.entry(theme).or_default().insert(entity);
    }

    fn remove_ancestry(&mut self, entity: EntityId) {
        if let Some(ancestors) = self.ancestors.remove(&entity) {
            for ancestor in ancestors {
                if let Some(dependents) = self.dependents.get_mut(&ancestor) {
                    dependents.remove(&entity);
                    if dependents.is_empty() {
                        self.dependents.remove(&ancestor);
                    }
                }
            }
        }
    }

    fn retire(&mut self, entity: EntityId) {
        self.remove_membership(entity);
        self.remove_ancestry(entity);
        self.watched.remove(&entity);
        self.owners.remove(&entity);
        self.membership.remove(&entity);
        self.dirty.remove(&entity);
        self.branches.remove(&entity);
        self.invalidated.remove(&entity);
        self.withdrawn.remove(&entity);
        self.paint_dirty.remove(&entity);
    }

    pub fn after_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        for entity in std::mem::take(&mut self.withdrawn) {
            let Some(target) = self.skins.get(&entity).map(|membership| membership.target) else {
                continue;
            };
            if let Some(skin) = target.get_mut_at_commit(context)
                && !skin.runtime.parts.is_empty()
            {
                skin.runtime.parts.clear();
                self.paint_dirty.insert(entity, target);
            }
        }
    }

    pub fn component_changed(&mut self, entity: EntityId, component: u16) {
        match component {
            ComponentValue::GUI_SKIN => {
                self.membership.insert(entity);
            }
            ComponentValue::GUI_THEME | ComponentValue::GUI_THEME_MOTION => {
                self.theme_dirty.insert(entity);
            }
            ComponentValue::GUI_BUTTON
            | ComponentValue::GUI_CHECKBOX
            | ComponentValue::GUI_SLIDER
            | ComponentValue::GUI_TEXT_INPUT
            | ComponentValue::GUI_SCROLL_VIEW
            | ComponentValue::GUI_VIRTUAL_LIST => {
                self.dirty.insert(entity);
            }
            ComponentValue::GUI_BEHAVIOR
            | ComponentValue::CANVAS_STYLE
            | ComponentValue::GUI_LAYOUT => {
                self.branches.insert(entity);
            }
            _ => {}
        }
    }

    pub fn prepare(&mut self, local: &GuiLocalState, context: &mut SystemRuntimeAccess<'_>) {
        self.changed.clear();
        for (entity, target) in std::mem::take(&mut self.paint_dirty) {
            if let Some(skin) = target.get_mut(context.world) {
                skin.runtime.notifying_sample = true;
                context.before_numeric_update(&[(entity, ComponentValue::GUI_SKIN)]);
                if let Some(skin) = target.get_mut(context.world) {
                    skin.runtime.notifying_sample = false;
                }
            }
        }

        #[cfg(feature = "diagnostics")]
        {
            self.statistics = Default::default();
        }

        if !self.initialized {
            self.membership.extend(
                context
                    .world
                    .state
                    .entities
                    .keys()
                    .copied()
                    .filter(|entity| {
                        GuiMotionSkinTarget::resolve(context.world, *entity).is_some()
                    }),
            );
            self.initialized = true;
        }

        for entity in std::mem::take(&mut self.membership) {
            let Some(target) = GuiMotionSkinTarget::resolve(context.world, entity) else {
                self.retire(entity);
                continue;
            };
            let theme = target
                .get(context.world)
                .expect("resolved skin target")
                .theme;
            self.set_membership(entity, target, theme);

            self.dirty.insert(entity);
        }

        for theme in std::mem::take(&mut self.theme_dirty) {
            if let Some(users) = self.users.get(&theme) {
                self.dirty.extend(users.iter().copied());
            }
        }

        for entity in std::mem::take(&mut self.branches) {
            if self.skins.contains_key(&entity) {
                self.dirty.insert(entity);
            }

            if let Some(dependents) = self.dependents.get(&entity) {
                self.dirty.extend(dependents.iter().copied());
            }
        }

        for entity in std::mem::take(&mut self.dirty) {
            let invalidated = self.invalidated.remove(&entity);
            let Some(target) = self
                .skins
                .get(&entity)
                .map(|membership| membership.target)
                .filter(|target| target.get(context.world).is_some())
            else {
                self.retire(entity);
                continue;
            };
            let configured = self.skins.get(&entity).is_some_and(|membership| {
                super::theme_live(&context.world.state, membership.theme)
            });
            self.remove_ancestry(entity);

            if configured {
                let mut ancestors = BTreeSet::new();
                let mut current = Some(entity);
                while let Some(ancestor) = current {
                    if !ancestors.insert(ancestor) {
                        break;
                    }

                    self.dependents.entry(ancestor).or_default().insert(entity);
                    current = context
                        .world
                        .state
                        .links
                        .effective(ancestor)
                        .and_then(|link| link.parent);
                }

                self.ancestors.insert(entity, ancestors);
            }

            let requests = if configured {
                #[cfg(feature = "diagnostics")]
                {
                    self.statistics.snapshots += 1;
                }
                GuiMotionControl::read(local, context.world, entity)
                    .and_then(|view| {
                        if !view.interaction.hovered
                            && !view.interaction.pressed
                            && !view.interaction.captured
                        {
                            self.watched.remove(&entity);
                        } else {
                            self.watched.insert(entity);
                        }

                        let skin = target.get(context.world)?;
                        let theme = context
                            .world
                            .components
                            .gui_theme(skin.theme.index() as usize)?;
                        let motion = context
                            .world
                            .components
                            .gui_theme_motion(skin.theme.index() as usize)?;
                        let mut requests = BTreeMap::new();
                        for part in [
                            GuiPrimitivePart::Background,
                            GuiPrimitivePart::Fill,
                            GuiPrimitivePart::Label,
                            GuiPrimitivePart::Icon,
                            GuiPrimitivePart::FocusRing,
                        ] {
                            #[cfg(feature = "diagnostics")]
                            {
                                self.statistics.parts += 1;
                            }

                            let owner = GuiMotionOwner {
                                entity,
                                control: view.control.target.component,
                                control_incarnation: view.control.target.incarnation,
                                skin_incarnation: target.incarnation,
                                part: part as u32,
                            };
                            if let Some(request) = resolve_part(
                                skin,
                                theme,
                                motion,
                                &view,
                                local.focus_visible(view.control.target),
                                owner,
                            ) {
                                requests.insert(part as u32, request);
                            }
                        }
                        Some(requests)
                    })
                    .unwrap_or_default()
            } else {
                self.watched.remove(&entity);
                BTreeMap::new()
            };
            if requests.is_empty() {
                self.watched.remove(&entity);
            }

            let previous_owners = self.owners.remove(&entity).unwrap_or_default();
            let next_owners: Vec<_> = requests.values().map(|request| request.owner).collect();
            self.changed.extend(
                previous_owners
                    .into_iter()
                    .filter(|owner| !next_owners.contains(owner)),
            );
            if !next_owners.is_empty() {
                self.owners.insert(entity, next_owners);
            }

            let Some(skin) = target.get_mut(context.world) else {
                continue;
            };
            let previous = skin.runtime.parts.len();
            skin.runtime
                .parts
                .retain(|part, _| requests.contains_key(part));
            let withdrawn = previous != skin.runtime.parts.len();
            for (part, request) in requests {
                match skin.runtime.parts.get_mut(&part) {
                    Some(channels) if channels.request.owner == request.owner => {
                        if invalidated || channels.request != request {
                            self.changed.push(request.owner);
                            channels.request = request;
                            channels.status = GuiSkinMotionStatus::Pending;
                        }
                    }
                    _ => {
                        self.changed.push(request.owner);
                        let mut values = request.values.clone();
                        values.resize(4, DynamicValue::F32(0.0));
                        skin.runtime.parts.insert(
                            part,
                            GuiMotionChannels {
                                values,
                                live_arity: request.values.len(),
                                ready_appearance: None,
                                request,
                                active: false,
                                status: GuiSkinMotionStatus::Pending,
                            },
                        );
                    }
                }
            }

            if withdrawn {
                skin.runtime.notifying_sample = true;
                context.before_numeric_update(&[(entity, ComponentValue::GUI_SKIN)]);
                if let Some(skin) = target.get_mut(context.world) {
                    skin.runtime.notifying_sample = false;
                }
            }
        }
    }
}

/// The control fields and System state a motion request resolves against.
struct GuiMotionControl {
    control: GuiControl,
    enabled: bool,
    interaction: crate::systems::gui::local::GuiInteractionFlags,
    checked: Option<bool>,
}

impl GuiMotionControl {
    /// An available, visible control; others resolve no motion.
    fn read(
        local: &GuiLocalState,
        world: &crate::world::WorldSimulationState,
        entity: EntityId,
    ) -> Option<Self> {
        let control = entity_control(world, &world.state, entity)?;
        let eligibility = eligibility(world, entity);
        if !eligibility.available || !eligibility.visible {
            return None;
        }
        let interaction = if eligibility.enabled {
            local.interaction_flags(control.target)
        } else {
            Default::default()
        };
        Some(Self {
            control,
            enabled: eligibility.enabled,
            interaction,
            checked: world
                .components
                .gui_checkbox(entity.index() as usize)
                .filter(|_| control.target.component == ComponentValue::GUI_CHECKBOX)
                .map(|checkbox| checkbox.checked),
        })
    }
}

fn resolve_part(
    skin: &super::super::presentation::GuiSkin,
    theme: &super::super::presentation::GuiTheme,
    motion: &GuiThemeMotion,
    view: &GuiMotionControl,
    focus_visible: bool,
    owner: GuiMotionOwner,
) -> Option<GuiMotionRequest> {
    let part = [
        GuiPrimitivePart::Background,
        GuiPrimitivePart::Fill,
        GuiPrimitivePart::Label,
        GuiPrimitivePart::Icon,
        GuiPrimitivePart::FocusRing,
    ]
    .into_iter()
    .find(|part| *part as u32 == owner.part)?;
    let state = GuiSkinState::resolve(
        !view.enabled,
        view.interaction.pressed,
        view.interaction.hovered,
    );
    let variant = view.checked.map(|checked| {
        if checked {
            GuiPartVariant::Checked
        } else {
            GuiPartVariant::Unchecked
        }
    });
    let mut style = super::super::presentation::paint::appearance(
        Some(skin),
        Some(theme),
        part,
        state,
        variant,
    );
    if part == GuiPrimitivePart::Icon
        && variant == Some(GuiPartVariant::Unchecked)
        && !super::super::presentation::paint::explicit_unchecked(Some(theme), state)
    {
        style.opacity = Some(0.0);
    }
    let base = GuiPartId::base(part).index();
    let aligned = skin
        .parts
        .iter()
        .any(|(_, row)| Some(row.part) == base && row.align_x.is_some())
        || theme
            .parts
            .iter()
            .any(|(_, row)| Some(row.part) == base && row.align_x.is_some());
    let mut request = resolve(motion, owner, skin.theme, state, variant, style, aligned)?;
    request.visible = part != GuiPrimitivePart::FocusRing || (focus_visible && view.enabled);
    Some(request)
}

fn resolve(
    table: &GuiThemeMotion,
    owner: GuiMotionOwner,
    theme: EntityId,
    state: GuiSkinState,
    variant: Option<GuiPartVariant>,
    style: crate::systems::gui::GuiPartStyle,
    aligned: bool,
) -> Option<GuiMotionRequest> {
    let part = [
        GuiPrimitivePart::Background,
        GuiPrimitivePart::Fill,
        GuiPrimitivePart::Label,
        GuiPrimitivePart::Icon,
        GuiPrimitivePart::FocusRing,
    ]
    .into_iter()
    .find(|part| *part as u32 == owner.part)?;
    let mut source = None;
    let mut duration = None;
    let mut easing = None;
    let mut track = None;
    let mut time = None;
    for key in GuiPartId::candidates(part, state, variant) {
        if let Some((_, row)) = table
            .parts
            .iter()
            .find(|(_, row)| Some(row.part) == key.index())
        {
            source = source.or_else(|| row.source.clone());
            duration = duration.or(row.duration);
            easing = easing.or(row.easing);
            track = track.or(row.track);
            time = time.or(row.time);
        }
    }

    let mut values = vec![
        DynamicValue::Vec4(style.color?),
        DynamicValue::F32(style.opacity?),
        DynamicValue::Vec2(style.scale?),
    ];
    if aligned {
        track?.checked_add(3)?;
        values.push(DynamicValue::F32(style.align_x?));
    }
    Some(GuiMotionRequest {
        owner,
        theme,
        source: source?,
        duration: f64::from(duration?),
        easing: match easing? {
            0 => AnimationTransitionEasing::Linear,
            1 => AnimationTransitionEasing::Smoothstep,
            _ => return None,
        },
        track: track?,
        time: f64::from(time?),
        values,
        appearance: style,
        visible: true,
    })
}
