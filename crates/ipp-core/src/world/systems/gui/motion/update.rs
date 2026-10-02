use super::runtime::{
    GuiMotionChannels, GuiMotionFollows, GuiMotionKey, GuiMotionOwner, GuiMotionValues,
    channel_focus_part, focus_part_channel, notify_sample,
};
use super::timing::GuiMotionRows;
use crate::systems::gui::local::control::{GuiControl, eligibility, entity_control, focus_parts};
use crate::systems::gui::local::{GUI_MAX_FOCUS_PARTS, GuiBehavior, GuiControlKind, GuiLocalState};
use crate::systems::gui::presentation::looks::{GuiSkinLook, control_look};
use crate::systems::gui::presentation::paint::{
    appearance, control_variant, explicit_unchecked, inherited_font_size,
};
use crate::systems::gui::presentation::{GuiSkin, GuiTheme};
use crate::systems::gui::{GuiPartStyle, GuiPartVariant, GuiPrimitivePart, GuiSkinState};
use crate::systems::{SystemCommitContext, SystemRuntimeAccess};
use crate::world::WorldSimulationState;
use crate::{ComponentValue, EntityId};
use std::collections::BTreeSet;

/// Parts whose appearance follows the control's interaction key. Scroll bar
/// parts resolve their states from layout, which follows animation, so they
/// change immediately.
const MOTION_PARTS: [GuiPrimitivePart; 5] = [
    GuiPrimitivePart::Background,
    GuiPrimitivePart::Fill,
    GuiPrimitivePart::Label,
    GuiPrimitivePart::Icon,
    GuiPrimitivePart::FocusRing,
];

/// The parts focus part `part` of a control `kind` with several paints for
/// itself: a range's thumb, and a colour control's surface with the field's
/// marker or a rail's thumb.
fn focus_part_paints(kind: GuiControlKind, part: u32) -> &'static [GuiPrimitivePart] {
    use GuiPrimitivePart::{Icon, Marker, Track};

    match kind {
        GuiControlKind::Color if part == 0 => &[Track, Marker],
        GuiControlKind::Color => &[Track, Icon],
        _ => &[Icon],
    }
}

/// A numeric input's step parts, each following its decrement (0) or
/// increment (1) part's state.
const STEP_PARTS: [(GuiPrimitivePart, usize); 4] = [
    (GuiPrimitivePart::Decrement, 0),
    (GuiPrimitivePart::DecrementMark, 0),
    (GuiPrimitivePart::Increment, 1),
    (GuiPrimitivePart::IncrementMark, 1),
];

/// The transition slots of a control `kind` with `parts` focus parts and,
/// when `steps`, a numeric input's step parts: each channel key, the part it
/// paints and whose state it follows. A control with several focus parts
/// paints each focus part's own parts, which move on channels of their own,
/// in place of the control's one Icon.
fn motion_slots(
    kind: GuiControlKind,
    parts: u32,
    steps: bool,
) -> impl Iterator<Item = (u32, GuiPrimitivePart, GuiMotionFollows)> {
    let several = parts > 1;
    MOTION_PARTS
        .into_iter()
        .filter(move |part| !(several && *part == GuiPrimitivePart::Icon))
        .map(|part| (part as u32, part, GuiMotionFollows::Control))
        .chain(
            (0..if several {
                parts
            } else {
                0
            })
                .flat_map(move |focus| {
                    focus_part_paints(kind, focus).iter().map(move |&part| {
                        (
                            focus_part_channel(focus, part),
                            part,
                            GuiMotionFollows::FocusPart(focus),
                        )
                    })
                }),
        )
        .chain(
            STEP_PARTS
                .into_iter()
                .filter(move |_| steps)
                .map(|(part, step)| (part as u32, part, GuiMotionFollows::Step(step))),
        )
}

/// The part a channel key of [`motion_slots`] paints and whose state it
/// follows.
fn slot_of(channel: u32) -> Option<(GuiPrimitivePart, GuiMotionFollows)> {
    match channel_focus_part(channel) {
        Some((part, focus)) => Some((part, GuiMotionFollows::FocusPart(focus))),
        None => MOTION_PARTS
            .into_iter()
            .find(|part| *part as u32 == channel)
            .map(|part| (part, GuiMotionFollows::Control))
            .or_else(|| {
                STEP_PARTS
                    .into_iter()
                    .find(|(part, _)| *part as u32 == channel)
                    .map(|(part, step)| (part, GuiMotionFollows::Step(step)))
            }),
    }
}

/// GUI's side of skin motion: it records each control's interaction key and,
/// when the key changes, writes each changed part's transition request into
/// the control's `GuiBehavior` for AnimationSystem to sample.
#[derive(Default)]
pub(in crate::world::systems::gui) struct GuiMotionState {
    /// The World selects AnimationSystem, the sole sampler. Without it no key
    /// is recorded and every part paints its resolved appearance.
    enabled: bool,
    initialized: bool,
    /// Controls to inspect at the next preparation.
    dirty: BTreeSet<EntityId>,
    /// Controls whose recorded key is hovered or pressed, which pointer
    /// feedback revalidation may change without naming them.
    watched: BTreeSet<EntityId>,
    /// Controls holding transition channels.
    in_flight: BTreeSet<EntityId>,
    /// Theme and font holders edited, and whether links changed, since the
    /// last preparation; only in-flight controls re-resolve their destination.
    themes: BTreeSet<EntityId>,
    fonts: BTreeSet<EntityId>,
    links: bool,
    /// Reduced motion as last applied.
    reduced: bool,
    /// Identity of the latest transition started.
    transitions: u64,
    changed: Vec<GuiMotionOwner>,
    pub statistics: super::GuiMotionPreparationWork,
}

impl GuiMotionState {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub(in crate::world::systems::gui) fn index_counts(&self) -> [usize; 5] {
        [
            self.dirty.len(),
            self.watched.len(),
            self.in_flight.len(),
            self.themes.len(),
            self.fonts.len(),
        ]
    }

    /// Owners whose transition started in the latest preparation.
    pub fn changes(&self) -> &[GuiMotionOwner] {
        &self.changed
    }

    pub fn dirty_entity(&mut self, entity: EntityId) {
        if self.enabled {
            self.dirty.insert(entity);
        }
    }

    pub fn dirty_entities(&mut self, entities: impl IntoIterator<Item = EntityId>) {
        if self.enabled {
            self.dirty.extend(entities);
        }
    }

    pub fn dirty_watched(&mut self) {
        if self.enabled {
            self.dirty.extend(self.watched.iter().copied());
        }
    }

    pub fn component_changed(&mut self, entity: EntityId, component: u16) {
        if !self.enabled {
            return;
        }

        match component {
            ComponentValue::GUI_BEHAVIOR | ComponentValue::GUI_SKIN => {
                self.dirty.insert(entity);
            }
            ComponentValue::GUI_THEME | ComponentValue::GUI_THEME_MOTION => {
                self.themes.insert(entity);
            }
            ComponentValue::GUI_FONT => {
                self.fonts.insert(entity);
            }
            component if GuiControlKind::of_component(component).is_some() => {
                self.dirty.insert(entity);
            }
            _ => {}
        }
    }

    pub fn before_commit(&mut self, context: &SystemCommitContext<'_>) {
        if !self.enabled {
            return;
        }

        for (entity, component) in context.changed_components() {
            self.component_changed(entity, component);
        }
        self.links |= context.changed_entity_links().next().is_some();
    }

    pub fn prepare(
        &mut self,
        local: &GuiLocalState,
        reduced: bool,
        context: &mut SystemRuntimeAccess<'_>,
    ) {
        self.changed.clear();
        self.statistics = Default::default();
        if !self.enabled {
            return;
        }

        self.release(reduced && !self.reduced, context);
        self.reduced = reduced;
        self.retarget_edited(context.world);
        if !self.initialized {
            let world = &*context.world;
            self.dirty.extend(
                world
                    .state
                    .entities
                    .keys()
                    .copied()
                    .filter(|entity| entity_control(world, &world.state, *entity).is_some()),
            );
            self.initialized = true;
        }

        for entity in std::mem::take(&mut self.dirty) {
            self.visit(local, context, entity);
        }
    }

    /// Release channels the sampler settled, which paint now resolves without
    /// them, or every channel when reduced motion has just been turned on.
    fn release(&mut self, snap: bool, context: &mut SystemRuntimeAccess<'_>) {
        for entity in std::mem::take(&mut self.in_flight) {
            let Some(behavior) = behavior_mut(context.world, entity) else {
                continue;
            };
            if snap {
                if !behavior.motion.parts.is_empty() {
                    behavior.motion.parts.clear();
                    notify_sample(context, entity);
                }
                continue;
            }

            behavior
                .motion
                .parts
                .retain(|_, channels| !channels.settled);
            if !behavior.motion.parts.is_empty() {
                self.in_flight.insert(entity);
            }
        }
    }

    /// Dirty the in-flight controls whose destination an edited theme, an
    /// edited inherited font or a tree change may have moved.
    fn retarget_edited(&mut self, world: &WorldSimulationState) {
        if self.links || !self.themes.is_empty() || !self.fonts.is_empty() {
            for &entity in &self.in_flight {
                let themed = world
                    .components
                    .gui_skin(entity.index() as usize)
                    .is_some_and(|skin| self.themes.contains(&skin.theme));
                // The walk visits each entity at most once, even on a cycle.
                let font = !self.fonts.is_empty()
                    && std::iter::successors(Some(entity), |&ancestor| {
                        world.state.links.parent(ancestor)
                    })
                    .take(world.state.entities.len())
                    .any(|ancestor| self.fonts.contains(&ancestor));
                if self.links || themed || font {
                    self.dirty.insert(entity);
                }
            }
        }

        self.themes.clear();
        self.fonts.clear();
        self.links = false;
    }

    /// Record a control's key; when it changed, start each changed part's
    /// transition, and when it did not, retarget its in-flight parts.
    fn visit(
        &mut self,
        local: &GuiLocalState,
        context: &mut SystemRuntimeAccess<'_>,
        entity: EntityId,
    ) {
        let world = &*context.world;
        let Some(control) = entity_control(world, &world.state, entity) else {
            self.forget(context, entity);
            return;
        };
        let Some(behavior_incarnation) = world
            .state
            .entities
            .get(&entity)
            .and_then(|record| record.input(ComponentValue::GUI_BEHAVIOR))
            .map(|input| input.incarnation)
        else {
            self.watched.remove(&entity);
            return;
        };

        self.statistics.snapshots += 1;
        let eligibility = eligibility(world, entity);
        if !eligibility.available || !eligibility.visible {
            // A hidden control has nothing displayed to move from.
            self.forget(context, entity);
            return;
        }

        let target = control.target;
        let interaction = if eligibility.enabled {
            local.interaction_flags(target)
        } else {
            Default::default()
        };
        let parts = focus_parts(world, control).min(GUI_MAX_FOCUS_PARTS as u32);
        let feedback = local.part_interaction(target, interaction);

        // A numeric input's step parts take the pointers over them, and its
        // field only the pointers over the rest of it, as paint resolves them.
        let number = (control.kind == GuiControlKind::TextInput)
            .then(|| world.components.gui_text_input(entity.index() as usize))
            .flatten()
            .filter(|input| input.shows_step_parts());
        let body = match number {
            Some(_) => feedback.body,
            None => interaction,
        };
        let key = GuiMotionKey {
            state: GuiSkinState::resolve(!eligibility.enabled, body.pressed, body.hovered),
            variant: control_variant(world, control),
            focused: eligibility.enabled && local.focus_visible(target),
            parts,
            part_states: std::array::from_fn(|part| {
                GuiSkinState::resolve(
                    !eligibility.enabled,
                    feedback.parts[part].pressed,
                    feedback.parts[part].hovered,
                )
            }),
            steps: number.map(|input| {
                let stepping = input.step_enabled();
                std::array::from_fn(|step| {
                    GuiSkinState::resolve(
                        !eligibility.enabled || !stepping[step],
                        feedback.steps[step].pressed,
                        feedback.steps[step].hovered,
                    )
                })
            }),
        };
        let pointed = |state| matches!(state, GuiSkinState::Hovered | GuiSkinState::Pressed);
        if pointed(key.state)
            || key.part_states[..parts as usize]
                .iter()
                .chain(key.steps.iter().flatten())
                .any(|s| pointed(*s))
        {
            self.watched.insert(entity);
        } else {
            self.watched.remove(&entity);
        }

        let owner = GuiMotionOwner {
            entity,
            control: target.component,
            control_incarnation: target.incarnation,
            behavior_incarnation,
            part: 0,
        };
        let Some(behavior) = behavior_mut(context.world, entity) else {
            return;
        };
        let previous = behavior
            .motion
            .key
            .replace((target.component, target.incarnation, key))
            .filter(|(component, incarnation, _)| {
                *component == target.component && *incarnation == target.incarnation
            })
            .map(|(_, _, key)| key);
        // Channels of a replaced control or behavior incarnation are stale.
        let stale = behavior.motion.parts.len();
        behavior.motion.parts.retain(|&part, channels| {
            channels.owner
                == GuiMotionOwner {
                    part,
                    ..owner
                }
        });
        let mut withdrawn = stale != behavior.motion.parts.len();

        match previous {
            Some(previous) if previous != key => {
                withdrawn |= self.start(context, control, owner, previous, key);
            }
            Some(_) => self.retarget(context, control, key),
            None => {
                // First sight: nothing displayed to move from.
                if let Some(behavior) = behavior_mut(context.world, entity)
                    && !behavior.motion.parts.is_empty()
                {
                    behavior.motion.parts.clear();
                    withdrawn = true;
                }
            }
        }

        if withdrawn {
            notify_sample(context, entity);
        }
        if context
            .world
            .components
            .gui_behavior(entity.index() as usize)
            .is_some_and(|behavior| !behavior.motion.parts.is_empty())
        {
            self.in_flight.insert(entity);
        }
    }

    /// Start a transition of every part whose animated values differ between
    /// the displayed appearance and `key`'s; return whether any channels were
    /// withdrawn instead.
    fn start(
        &mut self,
        context: &mut SystemRuntimeAccess<'_>,
        control: GuiControl,
        owner: GuiMotionOwner,
        previous: GuiMotionKey,
        key: GuiMotionKey,
    ) -> bool {
        let entity = control.target.entity;
        let mut started = Vec::new();
        {
            let world = &*context.world;
            let sources = GuiMotionSources::read(world, control);
            let Some(behavior) = world.components.gui_behavior(entity.index() as usize) else {
                return false;
            };
            for (channel, part, follows) in
                motion_slots(control.kind, key.parts, key.steps.is_some())
            {
                self.statistics.parts += 1;
                let (from, to) = (previous.slot(follows), key.slot(follows));
                let destination = sources.style(part, to);
                let (origin, origin_style, origin_present) =
                    match behavior.motion.parts.get(&channel) {
                        Some(channels) => (
                            channels.values,
                            channels.appearance.clone(),
                            channels.present,
                        ),
                        None => {
                            let style = sources.style(part, from);
                            let (values, present) = GuiMotionValues::of(&style);
                            (values, style, present)
                        }
                    };
                let (values, present) = GuiMotionValues::of(&destination);
                let timing = (!self.reduced)
                    .then(|| sources.rows().timing(part, from, to))
                    .flatten()
                    .filter(|(duration, _)| *duration > 0.0);
                let Some((duration, easing)) = timing.filter(|_| origin != values) else {
                    started.push((channel, None));
                    continue;
                };

                self.transitions += 1;
                let mut appearance = destination;
                appearance.inherit(&origin_style);
                appearance.asset = origin_style.asset;
                started.push((
                    channel,
                    Some(GuiMotionChannels {
                        owner: GuiMotionOwner {
                            part: channel,
                            ..owner
                        },
                        transition: self.transitions,
                        origin,
                        destination: values,
                        present: origin_present | present,
                        duration,
                        easing,
                        appearance,
                        values: origin,
                        settled: false,
                    }),
                ));
            }
        }

        let Some(behavior) = behavior_mut(context.world, entity) else {
            return false;
        };
        let mut withdrawn = false;
        for (channel, channels) in started {
            match channels {
                Some(channels) => {
                    self.changed.push(channels.owner);
                    behavior.motion.parts.insert(channel, channels);
                }
                None => withdrawn |= behavior.motion.parts.remove(&channel).is_some(),
            }
        }
        withdrawn
    }

    /// Move each in-flight part's destination and material to what `key`
    /// resolves to now, keeping its origin, timing and clock.
    fn retarget(
        &mut self,
        context: &mut SystemRuntimeAccess<'_>,
        control: GuiControl,
        key: GuiMotionKey,
    ) {
        let entity = control.target.entity;
        let mut updates = Vec::new();
        {
            let world = &*context.world;
            let Some(behavior) = world.components.gui_behavior(entity.index() as usize) else {
                return;
            };
            if behavior.motion.parts.is_empty() {
                return;
            }

            let sources = GuiMotionSources::read(world, control);
            for (&index, channels) in &behavior.motion.parts {
                let Some((part, follows)) = slot_of(index) else {
                    continue;
                };
                self.statistics.parts += 1;
                let destination = sources.style(part, key.slot(follows));
                let (values, present) = GuiMotionValues::of(&destination);
                let mut appearance = destination;
                appearance.inherit(&channels.appearance);
                appearance.asset.clone_from(&channels.appearance.asset);
                if values != channels.destination || appearance != channels.appearance {
                    updates.push((index, values, present, appearance));
                }
            }
        }

        let Some(behavior) = behavior_mut(context.world, entity) else {
            return;
        };
        for (index, values, present, appearance) in updates {
            if let Some(channels) = behavior.motion.parts.get_mut(&index) {
                channels.destination = values;
                channels.present |= present;
                channels.appearance = appearance;
            }
        }
    }

    /// Drop a control's key and channels: it next appears without a transition.
    fn forget(&mut self, context: &mut SystemRuntimeAccess<'_>, entity: EntityId) {
        self.watched.remove(&entity);
        self.in_flight.remove(&entity);
        let Some(behavior) = behavior_mut(context.world, entity) else {
            return;
        };
        behavior.motion.key = None;
        if !behavior.motion.parts.is_empty() {
            behavior.motion.parts.clear();
            notify_sample(context, entity);
        }
    }
}

/// The `GuiBehavior` of a live entity, never a later occupant of its slot.
fn behavior_mut(world: &mut WorldSimulationState, entity: EntityId) -> Option<&mut GuiBehavior> {
    if !world.state.entities.contains_key(&entity) {
        return None;
    }

    world.components.gui_behavior_mut(entity.index() as usize)
}

/// What a control's parts resolve their appearance and timing from.
struct GuiMotionSources<'a> {
    kind: GuiControlKind,
    /// The control's default look: its kind's, or the dial's.
    look: &'static GuiSkinLook,
    skin: Option<&'a GuiSkin>,
    theme: Option<&'a GuiTheme>,
    motion: Option<&'a super::GuiThemeMotion>,
    /// Inherited label font size, against which theme and look lengths draw.
    font_size: f32,
}

impl<'a> GuiMotionSources<'a> {
    fn read(world: &'a WorldSimulationState, control: GuiControl) -> Self {
        let entity = control.target.entity;
        let skin = world.components.gui_skin(entity.index() as usize);
        let theme = skin
            .map(|skin| skin.theme)
            .filter(|theme| theme.to_bits() != 0 && world.state.entities.contains_key(theme));
        Self {
            kind: control.kind,
            look: control_look(world, control),
            skin,
            theme: theme.and_then(|theme| world.components.gui_theme(theme.index() as usize)),
            motion: theme
                .and_then(|theme| world.components.gui_theme_motion(theme.index() as usize)),
            font_size: inherited_font_size(world, entity),
        }
    }

    fn rows(&self) -> GuiMotionRows<'a> {
        GuiMotionRows {
            theme: self.motion,
            look: &self.look.motion,
        }
    }

    /// `part`'s appearance at `key`, as paint resolves it, with the opacity of
    /// a part that paints only while visible at zero where it is not: an
    /// unchecked indicator its look leaves unstyled, and a hidden focus ring.
    fn style(&self, part: GuiPrimitivePart, key: GuiMotionKey) -> GuiPartStyle {
        let mut style = appearance(
            self.skin,
            self.theme,
            self.look,
            self.font_size,
            part,
            key.state,
            key.variant,
        );
        let hidden = match part {
            GuiPrimitivePart::Icon => {
                self.kind == GuiControlKind::Checkbox
                    && key.variant == Some(GuiPartVariant::Unchecked)
                    && !explicit_unchecked(self.theme, self.kind, key.state)
            }
            GuiPrimitivePart::FocusRing => !key.focused,
            _ => false,
        };
        if hidden {
            style.opacity = Some(0.0);
        }
        style
    }
}
