//! GUI action command payloads, momentary effect observations and the GUI
//! System query records. Control values are component fields read through
//! ordinary inspection and field observation.

use crate::codec::{ProtocolError, Reader, Writer};
use crate::references::WorldReference;
use ipp_core::EntityId;
use ipp_core::systems::gui::local::{
    GuiActiveItemRecord, GuiEntityTarget, GuiFocusRecord, GuiLocalAction, GuiLocalEffect,
    GuiLocalEffectKind, GuiLocalEffectSource, GuiPointerRecord,
};
use ipp_core::systems::gui::observations::{
    GuiObservationClasses, GuiObservationControlResult, GuiObservationEncoding,
    GuiObservationRecord, GuiObservationRejection, GuiObservationSubscriptionId,
};

#[cfg(test)]
#[path = "gui_tests.rs"]
mod tests;

/// Owned registration syntax; subscription lifetimes are validated by the receiving session.
#[derive(Clone, Debug, PartialEq)]
pub enum GuiObservationRequest {
    /// The selected World is explicit and must match the enclosing World session.
    Subscribe {
        /// Exact World lifetime.
        world: WorldReference,
        /// Applied record classes to retain.
        classes: GuiObservationClasses,
    },
    /// Retire only this exact output generation at the ordered receiver boundary.
    Unsubscribe {
        /// Exact World lifetime.
        world: WorldReference,
        /// Previously acknowledged output generation.
        subscription: GuiObservationSubscriptionId,
    },
}

const GUI_OBSERVATION_ENVELOPE_BYTES: usize = 3 * 8 + 1 + 4 + 2 * 8 + 1;
const GUI_EFFECT_ID_BYTES: usize = 1 + 16 + 8;
const GUI_TARGET_BYTES: usize = 16 + 8 + 2 + 8;
const GUI_ROUTED_SOURCE_BYTES: usize = 1 + 16;
/// Largest fixed effect payload: a pointer number and four feedback flags.
const GUI_EFFECT_PAYLOAD_BYTES: usize = 8 + 4;

/// Peak encoded allocations, including the ordinary response header and framing.
/// The largest fixed payload is a routed pointer feedback effect; submitted,
/// rejected and discarded text is charged per byte.
pub const GUI_OBSERVATION_ENCODING: GuiObservationEncoding = GuiObservationEncoding {
    control_bytes: 80,
    effect_bytes: GUI_OBSERVATION_ENVELOPE_BYTES
        + GUI_EFFECT_ID_BYTES
        + GUI_TARGET_BYTES
        + GUI_ROUTED_SOURCE_BYTES
        + 8
        + 4
        + 1
        + GUI_EFFECT_PAYLOAD_BYTES,
    ancestry_entry_bytes: 8,
    text_byte_bytes: 1,
};

/// Untrusted exact control identity; only the receiving Host resolves its World.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiTarget {
    /// Exact World lifetime.
    pub world: WorldReference,
    /// Generational ordinary entity.
    pub entity: EntityId,
    /// Concrete control component.
    pub component: u16,
    /// Exact component lifetime.
    pub incarnation: u64,
}

impl GuiTarget {
    /// Resolve only the World; the local receiver validates the current control lifetime.
    pub fn resolve(&self, host: &ipp_core::HostRuntime) -> Result<GuiEntityTarget, ProtocolError> {
        Ok(GuiEntityTarget {
            world: self.world.resolve(host)?,
            entity: self.entity,
            component: self.component,
            incarnation: self.incarnation,
        })
    }
}

impl Reader<'_> {
    pub(crate) fn gui_observation_request(
        &mut self,
    ) -> Result<GuiObservationRequest, ProtocolError> {
        let bytes = self.bytes_bounded(64)?;
        let mut reader = Reader {
            bytes: &bytes,
            at: 0,
        };
        let request = reader.gui_observation_body()?;
        if reader.at != bytes.len() {
            return Err(ProtocolError::Malformed("trailing GUI observation control"));
        }
        Ok(request)
    }

    fn gui_observation_body(&mut self) -> Result<GuiObservationRequest, ProtocolError> {
        let operation = self.u8()?;
        let world = self.world_reference()?;
        match operation {
            0 => Ok(GuiObservationRequest::Subscribe {
                world,
                classes: match self.u8()? {
                    0 => GuiObservationClasses::Application,
                    1 => GuiObservationClasses::Feedback,
                    2 => GuiObservationClasses::All,
                    _ => return Err(ProtocolError::Malformed("GUI observation class")),
                },
            }),
            1 => {
                let subscription = GuiObservationSubscriptionId {
                    output: self.u64()?,
                    generation: self.u64()?,
                };
                if subscription.output == 0 || subscription.generation == 0 {
                    return Err(ProtocolError::Malformed("GUI observation identity"));
                }
                Ok(GuiObservationRequest::Unsubscribe {
                    world,
                    subscription,
                })
            }
            _ => Err(ProtocolError::Malformed("GUI observation operation")),
        }
    }

    /// Decode one `gui-action` union member.
    pub(crate) fn gui_action(&mut self) -> Result<GuiLocalAction, ProtocolError> {
        use crate::contract::wire_manifest::*;
        Ok(match self.u8()? {
            GUI_ACTION_PRESS => GuiLocalAction::Press,
            GUI_ACTION_TOGGLE => GuiLocalAction::Toggle,
            GUI_ACTION_SET_SCALAR => GuiLocalAction::SetScalar(self.f32()?),
            GUI_ACTION_SET_TEXT => GuiLocalAction::SetText(self.text()?),
            GUI_ACTION_FOCUS => GuiLocalAction::Focus(self.u32()?),
            GUI_ACTION_BLUR => GuiLocalAction::Blur,
            GUI_ACTION_SUBMIT => GuiLocalAction::Submit,
            GUI_ACTION_SCROLL_TO => GuiLocalAction::ScrollTo([self.f32()?, self.f32()?]),
            GUI_ACTION_SCROLL_BY => GuiLocalAction::ScrollBy([self.f32()?, self.f32()?]),
            GUI_ACTION_SCROLL_TO_INDEX => GuiLocalAction::ScrollToIndex {
                index: self.u32()?,
                offset: self.f32()?,
            },
            GUI_ACTION_SET_COLOR => {
                GuiLocalAction::SetColor([self.f32()?, self.f32()?, self.f32()?, self.f32()?])
            }
            _ => return Err(ProtocolError::Malformed("GUI action")),
        })
    }
}

impl Writer {
    /// Encode one `GuiFocus` System query record.
    pub(crate) fn gui_focus_record(
        &mut self,
        record: &GuiFocusRecord,
    ) -> Result<(), ProtocolError> {
        self.gui_target(record.target)?;
        self.u8(u8::from(record.visible))?;
        self.u32(record.part)
    }

    /// Encode one `GuiPointers` System query record.
    pub(crate) fn gui_pointer_record(
        &mut self,
        record: &GuiPointerRecord,
    ) -> Result<(), ProtocolError> {
        self.gui_target(record.target)?;
        self.u64(record.pointer)?;
        self.raw(&[
            u8::from(record.state.hovered),
            u8::from(record.state.pressed),
            u8::from(record.state.captured),
        ])
    }

    /// Encode one `GuiActiveItems` System query record.
    pub(crate) fn gui_active_item_record(
        &mut self,
        record: &GuiActiveItemRecord,
    ) -> Result<(), ProtocolError> {
        self.u64(record.group.to_bits())?;
        self.gui_target(record.target)
    }

    fn gui_target(&mut self, target: GuiEntityTarget) -> Result<(), ProtocolError> {
        self.world_reference(target.world.into())?;
        self.u64(target.entity.to_bits())?;
        self.u16(target.component)?;
        self.u64(target.incarnation)
    }

    fn gui_ancestry(&mut self, ancestry: &[EntityId]) -> Result<(), ProtocolError> {
        self.count(ancestry.len(), crate::MAX_MESSAGE_BYTES / 8)?;
        for entity in ancestry {
            self.u64(entity.to_bits())?;
        }
        Ok(())
    }

    fn gui_effect(&mut self, effect: &GuiLocalEffect) -> Result<(), ProtocolError> {
        self.u8(u8::from(effect.id.is_some()))?;
        if let Some(id) = effect.id {
            if id.world != effect.target.world || id.ordinal == 0 {
                return Err(ProtocolError::Malformed("GUI effect identity"));
            }
            self.world_reference(id.world.into())?;
            self.u64(id.ordinal)?;
        }
        self.gui_target(effect.target)?;
        // Sources 1 (replacement) and 3 (layout) are retired.
        match effect.source {
            GuiLocalEffectSource::Semantic => self.u8(0)?,
            GuiLocalEffectSource::Routed {
                publication,
            } => {
                self.u8(2)?;
                self.publication_reference(publication)?;
            }
        }
        self.u64(effect.tick)?;
        self.gui_ancestry(&effect.ancestry)?;
        // Kinds 2 (value applied) and 5 (scroll changed) are retired; values
        // are observed as fields.
        match &effect.kind {
            GuiLocalEffectKind::Pressed => self.u8(0),
            GuiLocalEffectKind::FocusChanged {
                focused,
                changed,
                part,
            } => {
                self.u8(1)?;
                self.u8(u8::from(*focused))?;
                self.u8(u8::from(*changed))?;
                self.u32(*part)
            }
            GuiLocalEffectKind::Submitted(text) => {
                self.u8(4)?;
                self.string(text)
            }
            GuiLocalEffectKind::Rejected(text) => {
                self.u8(7)?;
                self.string(text)
            }
            GuiLocalEffectKind::Discarded(text) => {
                self.u8(8)?;
                self.string(text)
            }
            GuiLocalEffectKind::ContextRequested {
                point,
            } => {
                self.u8(6)?;
                self.f32(point[0])?;
                self.f32(point[1])
            }
            GuiLocalEffectKind::InteractionChanged(interaction) => {
                self.u8(3)?;
                self.u64(interaction.pointer)?;
                for flag in [
                    interaction.state.hovered,
                    interaction.state.pressed,
                    interaction.state.captured,
                    interaction.changed,
                ] {
                    self.u8(u8::from(flag))?;
                }
                Ok(())
            }
        }
    }

    pub(crate) fn gui_observation(
        &mut self,
        record: &GuiObservationRecord,
    ) -> Result<(), ProtocolError> {
        self.framed(|writer| {
            let subscription = match record {
                GuiObservationRecord::Control {
                    subscription,
                    ..
                }
                | GuiObservationRecord::Effect {
                    subscription,
                    ..
                } => subscription,
            };
            if subscription.output == 0 || subscription.generation == 0 {
                return Err(ProtocolError::Malformed("GUI observation identity"));
            }
            writer.u64(subscription.output)?;
            writer.u64(subscription.generation)?;
            match record {
                GuiObservationRecord::Control {
                    world,
                    result,
                    ..
                } => {
                    writer.u8(0)?;
                    writer.world_reference((*world).into())?;
                    writer.u8(match result {
                        GuiObservationControlResult::Subscribed => 0,
                        GuiObservationControlResult::Unsubscribed => 1,
                        GuiObservationControlResult::Cancelled => 2,
                        GuiObservationControlResult::Rejected(
                            GuiObservationRejection::StaleWorld,
                        ) => 3,
                        GuiObservationControlResult::Rejected(
                            GuiObservationRejection::StaleSubscription,
                        ) => 4,
                        GuiObservationControlResult::Rejected(
                            GuiObservationRejection::AlreadySubscribed,
                        ) => 5,
                    })
                }
                GuiObservationRecord::Effect {
                    effect,
                    ..
                } => {
                    if effect.id.is_none() {
                        return Err(ProtocolError::Malformed(
                            "GUI observation without applied identity",
                        ));
                    }
                    writer.u8(1)?;
                    writer.gui_effect(effect)
                }
            }
        })
    }
}
