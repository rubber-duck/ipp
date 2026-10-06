use super::GuiMotionEasing;
use crate::systems::SystemRuntimeAccess;
use crate::systems::gui::local::GUI_MAX_FOCUS_PARTS;
use crate::systems::gui::presentation::parts::{base_part, base_part_index};
use crate::systems::gui::{GuiPartStyle, GuiPartVariant, GuiPrimitivePart, GuiSkinState};
use crate::world::{WorldEntityState, WorldSimulationState};
use crate::{ComponentValue, EntityId};
use std::collections::BTreeMap;

/// One part's transition channels, identified by the exact control and
/// `GuiBehavior` lifetimes that requested them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::world) struct GuiMotionOwner {
    pub entity: EntityId,
    pub control: u16,
    pub control_incarnation: u64,
    pub behavior_incarnation: u64,
    pub part: u32,
}

impl GuiMotionOwner {
    /// Whether the control and `GuiBehavior` incarnations that requested the
    /// channels still occupy the entity.
    pub fn live(self, state: &WorldEntityState) -> bool {
        state.entities.get(&self.entity).is_some_and(|record| {
            record
                .input(self.control)
                .is_some_and(|input| input.incarnation == self.control_incarnation)
                && record
                    .input(ComponentValue::GUI_BEHAVIOR)
                    .is_some_and(|input| input.incarnation == self.behavior_incarnation)
        })
    }

    pub fn channels(self, world: &WorldSimulationState) -> Option<&GuiMotionChannels> {
        if !self.live(&world.state) {
            return None;
        }

        world
            .components
            .gui_behavior(self.entity.index() as usize)?
            .motion
            .parts
            .get(&self.part)
            .filter(|channels| channels.owner == self)
    }

    pub fn channels_mut(self, world: &mut WorldSimulationState) -> Option<&mut GuiMotionChannels> {
        if !self.live(&world.state) {
            return None;
        }

        world
            .components
            .gui_behavior_mut(self.entity.index() as usize)?
            .motion
            .parts
            .get_mut(&self.part)
            .filter(|channels| channels.owner == self)
    }
}

/// What a control's parts resolve their appearance from: its interaction
/// state, its checked variant, whether its focus ring shows and, for a
/// control with several focus parts or with step parts, the state of each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::world) struct GuiMotionKey {
    pub state: GuiSkinState,
    pub variant: Option<GuiPartVariant>,
    pub focused: bool,
    /// The control's focus parts, at most [`GUI_MAX_FOCUS_PARTS`].
    pub parts: u32,
    /// The interaction state of each focus part of a control with several.
    pub part_states: [GuiSkinState; GUI_MAX_FOCUS_PARTS],
    /// The interaction state of a numeric input's decrement and increment
    /// parts; none without them.
    pub steps: Option<[GuiSkinState; 2]>,
}

/// Whose interaction state a part's transition follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::world) enum GuiMotionFollows {
    /// The control's own.
    Control,
    /// A focus part's, on a control with several.
    FocusPart(u32),
    /// A numeric input's decrement (0) or increment (1) part's.
    Step(usize),
}

impl GuiMotionKey {
    /// The key a part following `follows` resolves from: the control's, with
    /// that focus part's state on a control with several, or that step
    /// part's on a numeric input with them.
    pub fn slot(self, follows: GuiMotionFollows) -> Self {
        let state = match follows {
            GuiMotionFollows::FocusPart(part) if self.parts > 1 && part < self.parts => {
                self.part_states[part as usize]
            }
            GuiMotionFollows::Step(step) => self.steps.map_or(self.state, |steps| steps[step]),
            _ => self.state,
        };
        Self {
            state,
            ..self
        }
    }
}

/// Channel keys of focus parts start above every primitive part's.
const FOCUS_PART_CHANNELS: u32 = 0x100;

/// The channel key of `primitive` painted for focus part `part` of a control
/// with several, such as a range's thumb or a colour control's rail and its
/// thumb, which move apart from the control's other parts.
pub(in crate::world::systems::gui) const fn focus_part_channel(
    part: u32,
    primitive: GuiPrimitivePart,
) -> u32 {
    FOCUS_PART_CHANNELS + base_part_index(primitive) * GUI_MAX_FOCUS_PARTS as u32 + part
}

/// The primitive part and the focus part a channel key follows, if it is one.
pub(in crate::world::systems::gui) fn channel_focus_part(
    channel: u32,
) -> Option<(GuiPrimitivePart, u32)> {
    let offset = channel.checked_sub(FOCUS_PART_CHANNELS)?;
    let parts = GUI_MAX_FOCUS_PARTS as u32;
    Some((base_part(offset / parts)?, offset % parts))
}

/// The appearance properties a transition animates, with paint's neutral value
/// where a resolved style leaves one absent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::world) struct GuiMotionValues {
    pub color: [f32; 4],
    pub opacity: f32,
    pub scale: [f32; 2],
    pub align_x: f32,
    pub border_color: [f32; 4],
    pub border_width: f32,
    pub glow_intensity: f32,
}

const COLOR: u8 = 1;
const OPACITY: u8 = 1 << 1;
const SCALE: u8 = 1 << 2;
const ALIGN_X: u8 = 1 << 3;
const BORDER_COLOR: u8 = 1 << 4;
const BORDER_WIDTH: u8 = 1 << 5;
const GLOW_INTENSITY: u8 = 1 << 6;

impl GuiMotionValues {
    /// The animated values of `style` and the mask of those it sets.
    pub fn of(style: &GuiPartStyle) -> (Self, u8) {
        let mut present = 0;
        let mut flag = |set: bool, bit: u8| {
            if set {
                present |= bit;
            }
        };
        flag(style.color.is_some(), COLOR);
        flag(style.opacity.is_some(), OPACITY);
        flag(style.scale.is_some(), SCALE);
        flag(style.align_x.is_some(), ALIGN_X);
        flag(style.border_color.is_some(), BORDER_COLOR);
        flag(style.border_width.is_some(), BORDER_WIDTH);
        flag(style.glow_intensity.is_some(), GLOW_INTENSITY);
        (
            Self {
                color: style.color.unwrap_or([1.0; 4]),
                opacity: style.opacity.unwrap_or(1.0),
                scale: style.scale.unwrap_or([1.0; 2]),
                align_x: style.align_x.unwrap_or(0.0),
                border_color: style.border_color.unwrap_or([0.0; 4]),
                border_width: style.border_width.unwrap_or(0.0),
                glow_intensity: style.glow_intensity.unwrap_or(0.0),
            },
            present,
        )
    }

    /// The values `t` of the way from `self` to `to`.
    pub fn mix(&self, to: &Self, t: f32) -> Self {
        let lerp = |from: f32, to: f32| from + (to - from) * t;
        let lanes = |from: [f32; 4], to: [f32; 4]| std::array::from_fn(|i| lerp(from[i], to[i]));
        Self {
            color: lanes(self.color, to.color),
            opacity: lerp(self.opacity, to.opacity),
            scale: [
                lerp(self.scale[0], to.scale[0]),
                lerp(self.scale[1], to.scale[1]),
            ],
            align_x: lerp(self.align_x, to.align_x),
            border_color: lanes(self.border_color, to.border_color),
            border_width: lerp(self.border_width, to.border_width),
            glow_intensity: lerp(self.glow_intensity, to.glow_intensity),
        }
    }

    /// Write the `present` values into `style`.
    fn apply(&self, present: u8, style: &mut GuiPartStyle) {
        let set = |bit: u8| present & bit != 0;
        if set(COLOR) {
            style.color = Some(self.color);
        }
        if set(OPACITY) {
            style.opacity = Some(self.opacity);
        }
        if set(SCALE) {
            style.scale = Some(self.scale);
        }
        if set(ALIGN_X) {
            style.align_x = Some(self.align_x);
        }
        if set(BORDER_COLOR) {
            style.border_color = Some(self.border_color);
        }
        if set(BORDER_WIDTH) {
            style.border_width = Some(self.border_width);
        }
        if set(GLOW_INTENSITY) {
            style.glow_intensity = Some(self.glow_intensity);
        }
    }
}

/// One part's live transition: GUI writes the request, AnimationSystem the
/// sample and its end.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::world) struct GuiMotionChannels {
    pub owner: GuiMotionOwner,
    /// Identity of the transition GUI started; a new one restarts the sampler's clock.
    pub transition: u64,
    pub origin: GuiMotionValues,
    pub destination: GuiMotionValues,
    /// Animated properties either end sets.
    pub present: u8,
    /// Host seconds, as authored; positive.
    pub duration: f32,
    pub easing: GuiMotionEasing,
    /// Material while the transition runs: the destination's appearance,
    /// keeping the origin's properties it leaves absent and the origin's paint
    /// asset.
    pub appearance: GuiPartStyle,
    /// The latest sample.
    pub values: GuiMotionValues,
    /// The sample reached the destination.
    pub settled: bool,
}

/// A control's interaction appearance key and its parts' live transition
/// channels: schema-ignored evaluated state of its `GuiBehavior`, never
/// authored, compared or saved. A part without channels paints its resolved
/// appearance.
#[derive(Clone, Debug, Default)]
pub struct GuiMotionRuntime {
    /// The control component and incarnation last prepared, and its key.
    pub(in crate::world) key: Option<(u16, u64, GuiMotionKey)>,
    pub(in crate::world) parts: BTreeMap<u32, GuiMotionChannels>,
    pub(in crate::world) notifying_sample: bool,
}

impl PartialEq for GuiMotionRuntime {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl GuiMotionRuntime {
    /// Parts with live transition channels; zero once every transition has
    /// settled and been released.
    pub fn transitions(&self) -> usize {
        self.parts.len()
    }

    /// Replace `style` with a transitioning part's material and current sample.
    pub(in crate::world::systems::gui) fn appearance(&self, part: u32, style: &mut GuiPartStyle) {
        let Some(channels) = self.parts.get(&part) else {
            return;
        };
        style.clone_from(&channels.appearance);

        // Gradient stops are unanimated material: an implicit stop keeps the
        // destination colour rather than following the sampled colour channel.
        if let Some(color) = style.color {
            style.gradient_color0.get_or_insert(color);
        }

        channels.values.apply(channels.present, style);
    }

    /// Whether a part that paints only while visible is still fading.
    pub(in crate::world::systems::gui) fn visible_part(&self, part: GuiPrimitivePart) -> bool {
        self.parts
            .get(&(part as u32))
            .is_some_and(|channels| channels.values.opacity > 0.0)
    }
}

/// Tell other Systems that `entity`'s transition channels changed without
/// reporting a `GuiBehavior` field change to GUI itself.
pub(in crate::world) fn notify_sample(context: &mut SystemRuntimeAccess<'_>, entity: EntityId) {
    let index = entity.index() as usize;
    let Some(behavior) = context.world.components.gui_behavior_mut(index) else {
        return;
    };
    behavior.motion.notifying_sample = true;
    context.before_numeric_update(&[(entity, ComponentValue::GUI_BEHAVIOR)]);
    if let Some(behavior) = context.world.components.gui_behavior_mut(index) {
        behavior.motion.notifying_sample = false;
    }
}
