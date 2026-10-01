use crate::components::registry::ComponentStorage;
use crate::services::asset_management::AssetSource;
use crate::systems::animation::AnimationTransitionEasing;
use crate::systems::gui::GuiPartStyle;
use crate::systems::gui::presentation::GuiSkin;
use crate::world::component_binding::ComponentBinding;
use crate::world::{WorldEntityState, WorldSimulationState};
use crate::{ComponentValue, DynamicValue, EntityId, ErrorReason};
use std::collections::BTreeMap;

/// Readiness of one transient, runtime-owned skin contribution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiSkinMotionStatus {
    /// The desired clip is not ready; only an existing legitimate contribution may remain.
    Pending,
    /// The desired clip is ready and the contribution is progressing or settled.
    Ready,
    /// Invalid or revoked input withdrew the sampled contribution.
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::world) struct GuiMotionOwner {
    pub entity: EntityId,
    pub control: u16,
    pub control_incarnation: u64,
    pub skin_incarnation: u64,
    pub part: u32,
}

impl GuiMotionOwner {
    pub fn skin(self, world: &WorldSimulationState) -> Option<GuiMotionSkinTarget> {
        let record = world.state.entities.get(&self.entity)?;
        (record.input(self.control)?.incarnation == self.control_incarnation).then_some(
            GuiMotionSkinTarget {
                entity: self.entity,
                incarnation: self.skin_incarnation,
            },
        )
    }
}

#[derive(Clone, Copy)]
pub(in crate::world) struct GuiMotionSkinTarget {
    entity: EntityId,
    pub incarnation: u64,
}

impl GuiMotionSkinTarget {
    pub fn declaration_at_commit(
        context: &crate::systems::SystemCommitContext<'_>,
        entity: EntityId,
    ) -> Option<(Self, EntityId)> {
        let incarnation = context
            .staged
            .entities
            .get(&entity)?
            .input(ComponentValue::GUI_SKIN)?
            .incarnation;
        let crate::components::schema::FieldValue::Entity(theme) = context.staged.input_field(
            &context.world_data.components,
            entity,
            ComponentValue::GUI_SKIN,
            std::mem::offset_of!(GuiSkin, theme) as u32,
        )?
        else {
            return None;
        };

        Some((
            Self {
                entity,
                incarnation,
            },
            theme,
        ))
    }

    pub fn resolve(world: &WorldSimulationState, entity: EntityId) -> Option<Self> {
        let incarnation = world
            .state
            .entities
            .get(&entity)?
            .input(ComponentValue::GUI_SKIN)?
            .incarnation;
        let target = Self {
            entity,
            incarnation,
        };
        target.get(world)?;
        Some(target)
    }

    fn live(self, state: &WorldEntityState) -> bool {
        state
            .entities
            .get(&self.entity)
            .and_then(|record| record.input(ComponentValue::GUI_SKIN))
            .is_some_and(|input| input.incarnation == self.incarnation)
    }

    pub fn get(self, world: &WorldSimulationState) -> Option<&GuiSkin> {
        if !self.live(&world.state) {
            return None;
        }

        world.components.gui_skin(self.entity.index() as usize)
    }

    pub fn get_mut(self, world: &mut WorldSimulationState) -> Option<&mut GuiSkin> {
        if !self.live(&world.state) {
            return None;
        }

        world.components.gui_skin_mut(self.entity.index() as usize)
    }

    pub fn get_mut_at_commit<'a>(
        self,
        context: &'a mut crate::systems::SystemCommitContext<'_>,
    ) -> Option<&'a mut GuiSkin> {
        if !self.live(context.staged) {
            return None;
        }

        context
            .world_data
            .components
            .gui_skin_mut(self.entity.index() as usize)
    }
}

pub(in crate::world) fn theme_live(state: &WorldEntityState, theme: EntityId) -> bool {
    state.entities.get(&theme).is_some_and(|record| {
        record.input(ComponentValue::GUI_THEME).is_some()
            && record.input(ComponentValue::GUI_THEME_MOTION).is_some()
    })
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::world) struct GuiMotionRequest {
    pub owner: GuiMotionOwner,
    pub theme: EntityId,
    pub source: AssetSource,
    pub duration: f64,
    pub easing: AnimationTransitionEasing,
    pub track: u32,
    pub time: f64,
    pub values: Vec<DynamicValue>,
    pub appearance: GuiPartStyle,
    pub visible: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::world) struct GuiMotionChannels {
    pub request: GuiMotionRequest,
    pub values: Vec<DynamicValue>,
    pub active: bool,
    pub live_arity: usize,
    pub ready_appearance: Option<GuiPartStyle>,
    pub status: GuiSkinMotionStatus,
}

/// Schema-ignored, sparse evaluated channels. Generic authoring and snapshots omit these.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuiSkinRuntime {
    pub(in crate::world) parts: BTreeMap<u32, GuiMotionChannels>,
    pub(in crate::world) notifying_sample: bool,
}

impl GuiSkinRuntime {
    /// Observe readiness without providing a writable animation target.
    pub fn status(
        &self,
        part: crate::systems::gui::GuiPrimitivePart,
    ) -> Option<GuiSkinMotionStatus> {
        self.parts.get(&(part as u32)).map(|part| part.status)
    }

    pub(in crate::world::systems::gui) fn appearance(&self, part: u32, style: &mut GuiPartStyle) {
        let Some(channels) = self.parts.get(&part).filter(|channels| channels.active) else {
            return;
        };
        if let Some(ready) = &channels.ready_appearance {
            style.clone_from(ready);
        }

        // Gradient stops are unanimated material: an implicit stop keeps the
        // destination colour rather than following the sampled colour channel.
        if let Some(color) = style.color {
            style.gradient_color0.get_or_insert(color);
        }

        for (index, value) in channels.values.iter().take(channels.live_arity).enumerate() {
            match (index, value) {
                (0, DynamicValue::Vec4(value)) => style.color = Some(*value),
                (1, DynamicValue::F32(value)) => style.opacity = Some(*value),
                (2, DynamicValue::Vec2(value)) => style.scale = Some(*value),
                (3, DynamicValue::F32(value)) => style.align_x = Some(*value),
                _ => unreachable!("validated skin channel"),
            }
        }
    }

    pub(in crate::world::systems::gui) fn visible_icon(&self) -> bool {
        self.visible_part(crate::systems::gui::GuiPrimitivePart::Icon)
    }

    pub(in crate::world::systems::gui) fn visible_part(
        &self,
        part: crate::systems::gui::GuiPrimitivePart,
    ) -> bool {
        self.parts.get(&(part as u32)).is_some_and(|channels| {
            channels.active
                && matches!(channels.values.get(1), Some(DynamicValue::F32(value)) if *value > 0.0)
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub(in crate::world) struct GuiMotionDestination {
    skin: ComponentBinding<GuiSkin>,
    owner: GuiMotionOwner,
    channel: usize,
}

impl GuiMotionDestination {
    pub fn offset(part: u32, channel: usize) -> u32 {
        0xffff_0000 + part * 4 + channel as u32
    }

    /// # Safety
    /// The caller drops all copies before this skin incarnation leaves its World storage.
    pub unsafe fn bind(storage: &ComponentStorage, entity: EntityId, offset: u32) -> Option<Self> {
        let offset = offset.checked_sub(0xffff_0000)?;
        let part = offset / 4;
        let channel = (offset % 4) as usize;
        let channels = storage
            .gui_skin(entity.index() as usize)?
            .runtime
            .parts
            .get(&part)?;
        channels.values.get(channel)?;
        Some(Self {
            // SAFETY: The typed slot belongs to storage; the caller fences every copy before reuse.
            skin: unsafe { ComponentBinding::new(storage.gui_skin_ptr(entity.index() as usize)?) },
            owner: channels.request.owner,
            channel,
        })
    }

    pub fn validate(offset: u32, value: &DynamicValue) -> Result<(), ErrorReason> {
        let channel = offset
            .checked_sub(0xffff_0000)
            .ok_or(ErrorReason::InvalidField)?
            % 4;
        let valid = match (channel, value) {
            (0, DynamicValue::Vec4(value)) => value
                .iter()
                .all(|value| value.is_finite() && (0.0..=1.0).contains(value)),
            (1, DynamicValue::F32(value)) => value.is_finite() && (0.0..=1.0).contains(value),
            (2, DynamicValue::Vec2(value)) => value.iter().all(|value| value.is_finite()),
            (3, DynamicValue::F32(value)) => value.is_finite(),
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(ErrorReason::InvalidValue)
        }
    }

    pub fn write(
        self,
        storage: &mut ComponentStorage,
        value: DynamicValue,
    ) -> Result<(), ErrorReason> {
        Self::validate(Self::offset(self.owner.part, self.channel), &value)?;
        let channels = self
            .skin
            .get_mut(storage)
            .runtime
            .parts
            .get_mut(&self.owner.part)
            .filter(|channels| channels.request.owner == self.owner)
            .ok_or(ErrorReason::MissingComponent)?;
        *channels
            .values
            .get_mut(self.channel)
            .ok_or(ErrorReason::InvalidField)? = value;
        Ok(())
    }
}
