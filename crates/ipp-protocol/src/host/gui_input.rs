//! Physical context/input envelopes. Contexts are connection-owned and never
//! authorize authoring or supply private Core input/path capabilities.

use crate::codec::{Reader, Writer};
use crate::contract::wire_manifest::*;
use crate::{ProtocolError, host::presentation::PresentationView};
use ipp_core::services::gui_input::GUI_INPUT_MAX_POINTERS;
use ipp_core::services::gui_input::router::{GuiPhysicalButton, GuiPhysicalInput, GuiPhysicalKey};
use ipp_core::systems::gui::local::{GuiNativeTextState, GuiTextComposition, GuiTextEdit};

// Cancellations encode their pointer count in one byte.
const _: () = assert!(GUI_INPUT_MAX_POINTERS <= u8::MAX as usize);

/// All physical input is fenced to an explicitly selected actual surface.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)]
pub enum GuiPhysicalRequest {
    Text {
        context: u64,
        target: crate::world::gui::GuiTarget,
        generation: u64,
        edit: GuiTextEdit,
    },
    Open {
        view: PresentationView,
        blockers: Vec<GuiPickingBlocker>,
    },
    Close(u64),
    Event {
        context: u64,
        input: GuiPhysicalInput,
    },
}

/// Untrusted exact picking-shape identity, never an implicit rendered-mesh blocker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub struct GuiPickingBlocker {
    pub world: crate::references::WorldReference,
    pub entity: ipp_core::EntityId,
    pub incarnation: u64,
}

/// One correlated terminal. Applied child counts preserve partial admission;
/// application callbacks consume the independent effect stream and field observation.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)]
pub enum GuiPhysicalResponse {
    Native {
        context: u64,
        state: Option<Vec<u8>>,
    },
    Opened(u64),
    Closed,
    Routed {
        disposition: u8,
        applied: u32,
        rejected: u32,
        cancelled: u32,
        error: Option<String>,
        remaining: Option<[f32; 2]>,
        native: Option<Vec<u8>>,
    },
    Rejected(String),
    Revoked(u64),
    Cancelled {
        context: u64,
        pointers: Vec<u64>,
        focus: bool,
    },
}

const KEYS: &[(u8, GuiPhysicalKey)] = &[
    (GUI_PHYSICAL_KEY_TAB, GuiPhysicalKey::Tab),
    (GUI_PHYSICAL_KEY_BACK_TAB, GuiPhysicalKey::BackTab),
    (GUI_PHYSICAL_KEY_ENTER, GuiPhysicalKey::Enter),
    (GUI_PHYSICAL_KEY_SPACE, GuiPhysicalKey::Space),
    (GUI_PHYSICAL_KEY_ESCAPE, GuiPhysicalKey::Escape),
    (GUI_PHYSICAL_KEY_LEFT, GuiPhysicalKey::Left),
    (GUI_PHYSICAL_KEY_RIGHT, GuiPhysicalKey::Right),
    (GUI_PHYSICAL_KEY_UP, GuiPhysicalKey::Up),
    (GUI_PHYSICAL_KEY_DOWN, GuiPhysicalKey::Down),
    (GUI_PHYSICAL_KEY_HOME, GuiPhysicalKey::Home),
    (GUI_PHYSICAL_KEY_END, GuiPhysicalKey::End),
    (GUI_PHYSICAL_KEY_CONTEXT_MENU, GuiPhysicalKey::ContextMenu),
    (GUI_PHYSICAL_KEY_F10, GuiPhysicalKey::F10),
];

impl Reader<'_> {
    pub(crate) fn gui_physical_request(&mut self) -> Result<GuiPhysicalRequest, ProtocolError> {
        let bytes = self.bytes_bounded(crate::MAX_MESSAGE_BYTES)?;
        let mut reader = Reader {
            bytes: &bytes,
            at: 0,
        };
        let request = match reader.u8()? {
            GUI_PHYSICAL_REQUEST_TEXT => GuiPhysicalRequest::Text {
                context: reader.u64()?,
                target: crate::world::gui::GuiTarget {
                    world: reader.world_reference()?,
                    entity: ipp_core::EntityId::from_bits(reader.u64()?),
                    component: reader.u16()?,
                    incarnation: reader.u64()?,
                },
                generation: reader.u64()?,
                edit: reader.native_edit()?,
            },
            GUI_PHYSICAL_REQUEST_OPEN => {
                let view = reader.presentation_view()?;
                let count = reader.count(crate::MAX_MESSAGE_BYTES / 32)?;
                if count > reader.bytes.len().saturating_sub(reader.at) / 32 {
                    return Err(ProtocolError::Malformed("truncated physical blockers"));
                }
                let mut blockers = Vec::with_capacity(count);
                for _ in 0..count {
                    blockers.push(GuiPickingBlocker {
                        world: reader.world_reference()?,
                        entity: ipp_core::EntityId::from_bits(reader.u64()?),
                        incarnation: reader.u64()?,
                    });
                }
                GuiPhysicalRequest::Open {
                    view,
                    blockers,
                }
            }
            GUI_PHYSICAL_REQUEST_CLOSE => GuiPhysicalRequest::Close(reader.u64()?),
            GUI_PHYSICAL_REQUEST_EVENT => {
                let context = reader.u64()?;
                let input = match reader.u8()? {
                    GUI_PHYSICAL_EVENT_POINTER_DOWN => GuiPhysicalInput::PointerDown {
                        pointer: reader.u64()?,
                        point: [reader.f32()?, reader.f32()?],
                        button: reader.physical_button()?,
                    },
                    GUI_PHYSICAL_EVENT_POINTER_MOVE => GuiPhysicalInput::PointerMove {
                        pointer: reader.u64()?,
                        point: [reader.f32()?, reader.f32()?],
                    },
                    GUI_PHYSICAL_EVENT_POINTER_UP => GuiPhysicalInput::PointerUp {
                        pointer: reader.u64()?,
                        point: [reader.f32()?, reader.f32()?],
                        button: reader.physical_button()?,
                    },
                    GUI_PHYSICAL_EVENT_POINTER_CANCEL => GuiPhysicalInput::PointerCancel {
                        pointer: reader.u64()?,
                    },
                    GUI_PHYSICAL_EVENT_WHEEL => GuiPhysicalInput::Wheel {
                        point: [reader.f32()?, reader.f32()?],
                        delta: [reader.f32()?, reader.f32()?],
                        shift: reader.boolean()?,
                    },
                    GUI_PHYSICAL_EVENT_KEY => GuiPhysicalInput::Key {
                        key: {
                            let key_tag = reader.u8()?;
                            KEYS.iter()
                                .find(|(tag, _)| *tag == key_tag)
                                .ok_or(ProtocolError::Malformed("physical key"))?
                                .1
                        },
                        shift: reader.boolean()?,
                    },
                    GUI_PHYSICAL_EVENT_BLUR => GuiPhysicalInput::Blur,
                    tag => return Err(ProtocolError::Unsupported(tag)),
                };
                GuiPhysicalRequest::Event {
                    context,
                    input,
                }
            }
            tag => return Err(ProtocolError::Unsupported(tag)),
        };
        if reader.at != bytes.len() {
            return Err(ProtocolError::Malformed("physical input trailing bytes"));
        }
        Ok(request)
    }

    pub(crate) fn gui_physical_response(&mut self) -> Result<GuiPhysicalResponse, ProtocolError> {
        let bytes = self.bytes_bounded(crate::MAX_MESSAGE_BYTES)?;
        let mut reader = Reader {
            bytes: &bytes,
            at: 0,
        };
        let response = match reader.u8()? {
            GUI_PHYSICAL_RESPONSE_OPENED => GuiPhysicalResponse::Opened(reader.u64()?),
            GUI_PHYSICAL_RESPONSE_CLOSED => GuiPhysicalResponse::Closed,
            GUI_PHYSICAL_RESPONSE_ROUTED => GuiPhysicalResponse::Routed {
                disposition: reader.u8()?,
                applied: reader.u32()?,
                rejected: reader.u32()?,
                cancelled: reader.u32()?,
                error: if reader.boolean()? {
                    Some(reader.string()?)
                } else {
                    None
                },
                remaining: if reader.boolean()? {
                    Some([reader.f32()?, reader.f32()?])
                } else {
                    None
                },
                native: if reader.boolean()? {
                    Some(reader.bytes_bounded(crate::MAX_MESSAGE_BYTES)?)
                } else {
                    None
                },
            },
            GUI_PHYSICAL_RESPONSE_REJECTED => GuiPhysicalResponse::Rejected(reader.string()?),
            GUI_PHYSICAL_RESPONSE_REVOKED => GuiPhysicalResponse::Revoked(reader.u64()?),
            GUI_PHYSICAL_RESPONSE_CANCELLED => {
                let context = reader.u64()?;
                let count = reader.u8()?;
                if usize::from(count) > GUI_INPUT_MAX_POINTERS {
                    return Err(ProtocolError::Malformed("physical cancellation count"));
                }
                let mut pointers = Vec::with_capacity(usize::from(count));
                for _ in 0..count {
                    pointers.push(reader.u64()?);
                }
                GuiPhysicalResponse::Cancelled {
                    context,
                    pointers,
                    focus: reader.boolean()?,
                }
            }
            GUI_PHYSICAL_RESPONSE_NATIVE => GuiPhysicalResponse::Native {
                context: reader.u64()?,
                state: if reader.boolean()? {
                    Some(reader.bytes_bounded(crate::MAX_MESSAGE_BYTES)?)
                } else {
                    None
                },
            },
            tag => return Err(ProtocolError::Unsupported(tag)),
        };
        if reader.at != bytes.len() {
            return Err(ProtocolError::Malformed("physical response trailing bytes"));
        }
        Ok(response)
    }
}

impl Writer {
    pub(crate) fn gui_physical_request(
        &mut self,
        request: &GuiPhysicalRequest,
    ) -> Result<(), ProtocolError> {
        self.framed(|writer| {
            match request {
                GuiPhysicalRequest::Text {
                    context,
                    target,
                    generation,
                    edit,
                } => {
                    writer.u8(GUI_PHYSICAL_REQUEST_TEXT)?;
                    writer.u64(*context)?;
                    writer.world_reference(target.world)?;
                    writer.u64(target.entity.to_bits())?;
                    writer.u16(target.component)?;
                    writer.u64(target.incarnation)?;
                    writer.u64(*generation)?;
                    writer.native_edit(edit)?;
                }
                GuiPhysicalRequest::Open {
                    view,
                    blockers,
                } => {
                    writer.u8(GUI_PHYSICAL_REQUEST_OPEN)?;
                    writer.presentation_view(*view)?;
                    writer.count(blockers.len(), crate::MAX_MESSAGE_BYTES / 32)?;
                    for blocker in blockers {
                        writer.world_reference(blocker.world)?;
                        writer.u64(blocker.entity.to_bits())?;
                        writer.u64(blocker.incarnation)?;
                    }
                }
                GuiPhysicalRequest::Close(context) => {
                    writer.u8(GUI_PHYSICAL_REQUEST_CLOSE)?;
                    writer.u64(*context)?;
                }
                GuiPhysicalRequest::Event {
                    context,
                    input,
                } => {
                    writer.u8(GUI_PHYSICAL_REQUEST_EVENT)?;
                    writer.u64(*context)?;
                    match input {
                        GuiPhysicalInput::Text {
                            ..
                        } => {
                            return Err(ProtocolError::Malformed(
                                "resolved native input is not wire syntax",
                            ));
                        }
                        GuiPhysicalInput::PointerDown {
                            pointer,
                            point,
                            ..
                        }
                        | GuiPhysicalInput::PointerMove {
                            pointer,
                            point,
                        }
                        | GuiPhysicalInput::PointerUp {
                            pointer,
                            point,
                            ..
                        } => {
                            writer.u8(match input {
                                GuiPhysicalInput::PointerDown {
                                    ..
                                } => GUI_PHYSICAL_EVENT_POINTER_DOWN,
                                GuiPhysicalInput::PointerMove {
                                    ..
                                } => GUI_PHYSICAL_EVENT_POINTER_MOVE,
                                _ => GUI_PHYSICAL_EVENT_POINTER_UP,
                            })?;
                            writer.u64(*pointer)?;
                            writer.f32(point[0])?;
                            writer.f32(point[1])?;
                            if let GuiPhysicalInput::PointerDown {
                                button,
                                ..
                            }
                            | GuiPhysicalInput::PointerUp {
                                button,
                                ..
                            } = input
                            {
                                writer.u8(match button {
                                    GuiPhysicalButton::Primary => GUI_PHYSICAL_BUTTON_PRIMARY,
                                    GuiPhysicalButton::Secondary => GUI_PHYSICAL_BUTTON_SECONDARY,
                                    GuiPhysicalButton::Auxiliary => GUI_PHYSICAL_BUTTON_AUXILIARY,
                                })?;
                            }
                        }
                        GuiPhysicalInput::PointerCancel {
                            pointer,
                        } => {
                            writer.u8(GUI_PHYSICAL_EVENT_POINTER_CANCEL)?;
                            writer.u64(*pointer)?;
                        }
                        GuiPhysicalInput::Wheel {
                            point,
                            delta,
                            shift,
                        } => {
                            writer.u8(GUI_PHYSICAL_EVENT_WHEEL)?;
                            for value in point.iter().chain(delta) {
                                writer.f32(*value)?;
                            }
                            writer.u8(u8::from(*shift))?;
                        }
                        GuiPhysicalInput::Key {
                            key,
                            shift,
                        } => {
                            writer.u8(GUI_PHYSICAL_EVENT_KEY)?;
                            writer.u8(KEYS
                                .iter()
                                .find(|(_, candidate)| candidate == key)
                                .ok_or(ProtocolError::Malformed("physical key"))?
                                .0)?;
                            writer.u8(u8::from(*shift))?;
                        }
                        GuiPhysicalInput::Blur => writer.u8(GUI_PHYSICAL_EVENT_BLUR)?,
                    }
                }
            }
            Ok(())
        })
    }

    pub(crate) fn gui_physical_response(
        &mut self,
        response: &GuiPhysicalResponse,
    ) -> Result<(), ProtocolError> {
        self.framed(|writer| {
            match response {
                GuiPhysicalResponse::Native {
                    context,
                    state,
                } => {
                    writer.u8(GUI_PHYSICAL_RESPONSE_NATIVE)?;
                    writer.u64(*context)?;
                    writer.u8(u8::from(state.is_some()))?;
                    if let Some(state) = state {
                        writer.count(state.len(), crate::MAX_MESSAGE_BYTES)?;
                        writer.raw(state)?;
                    }
                }
                GuiPhysicalResponse::Opened(context) => {
                    writer.u8(GUI_PHYSICAL_RESPONSE_OPENED)?;
                    writer.u64(*context)?;
                }
                GuiPhysicalResponse::Closed => writer.u8(GUI_PHYSICAL_RESPONSE_CLOSED)?,
                GuiPhysicalResponse::Routed {
                    disposition,
                    applied,
                    rejected,
                    cancelled,
                    error,
                    remaining,
                    native,
                } => {
                    writer.u8(GUI_PHYSICAL_RESPONSE_ROUTED)?;
                    writer.u8(*disposition)?;
                    writer.u32(*applied)?;
                    writer.u32(*rejected)?;
                    writer.u32(*cancelled)?;
                    writer.u8(u8::from(error.is_some()))?;
                    if let Some(error) = error {
                        writer.string(error)?;
                    }
                    writer.u8(u8::from(remaining.is_some()))?;
                    if let Some(remaining) = remaining {
                        writer.f32(remaining[0])?;
                        writer.f32(remaining[1])?;
                    }
                    writer.u8(u8::from(native.is_some()))?;
                    if let Some(native) = native {
                        writer.count(native.len(), crate::MAX_MESSAGE_BYTES)?;
                        writer.raw(native)?;
                    }
                }
                GuiPhysicalResponse::Rejected(error) => {
                    writer.u8(GUI_PHYSICAL_RESPONSE_REJECTED)?;
                    writer.string(error)?;
                }
                GuiPhysicalResponse::Revoked(context) => {
                    writer.u8(GUI_PHYSICAL_RESPONSE_REVOKED)?;
                    writer.u64(*context)?;
                }
                GuiPhysicalResponse::Cancelled {
                    context,
                    pointers,
                    focus,
                } => {
                    if pointers.len() > GUI_INPUT_MAX_POINTERS {
                        return Err(ProtocolError::Malformed("physical cancellation count"));
                    }
                    writer.u8(GUI_PHYSICAL_RESPONSE_CANCELLED)?;
                    writer.u64(*context)?;
                    writer.u8(pointers.len() as u8)?;
                    for pointer in pointers {
                        writer.u64(*pointer)?;
                    }
                    writer.u8(u8::from(*focus))?;
                }
            }
            Ok(())
        })
    }
}

impl Reader<'_> {
    fn physical_button(&mut self) -> Result<GuiPhysicalButton, ProtocolError> {
        match self.u8()? {
            GUI_PHYSICAL_BUTTON_PRIMARY => Ok(GuiPhysicalButton::Primary),
            GUI_PHYSICAL_BUTTON_SECONDARY => Ok(GuiPhysicalButton::Secondary),
            GUI_PHYSICAL_BUTTON_AUXILIARY => Ok(GuiPhysicalButton::Auxiliary),
            _ => Err(ProtocolError::Malformed("physical pointer button")),
        }
    }

    fn native_edit(&mut self) -> Result<GuiTextEdit, ProtocolError> {
        Ok(match self.u8()? {
            GUI_NATIVE_EDIT_INSERT => GuiTextEdit::Insert(self.text()?),
            GUI_NATIVE_EDIT_SELECTION => GuiTextEdit::Selection([self.u32()?, self.u32()?]),
            GUI_NATIVE_EDIT_COMPOSE => GuiTextEdit::Compose(GuiTextComposition {
                text: self.text()?,
                selection: [self.u32()?, self.u32()?],
            }),
            GUI_NATIVE_EDIT_COMMIT_COMPOSITION => GuiTextEdit::CommitComposition,
            GUI_NATIVE_EDIT_CANCEL_COMPOSITION => GuiTextEdit::CancelComposition,
            GUI_NATIVE_EDIT_BACKSPACE => GuiTextEdit::Backspace,
            GUI_NATIVE_EDIT_DELETE => GuiTextEdit::Delete,
            GUI_NATIVE_EDIT_LEFT => GuiTextEdit::Left,
            GUI_NATIVE_EDIT_RIGHT => GuiTextEdit::Right,
            GUI_NATIVE_EDIT_HOME => GuiTextEdit::Home,
            GUI_NATIVE_EDIT_END => GuiTextEdit::End,
            GUI_NATIVE_EDIT_SELECT_ALL => GuiTextEdit::SelectAll,
            GUI_NATIVE_EDIT_SUBMIT => GuiTextEdit::Submit,
            _ => return Err(ProtocolError::Malformed("native edit")),
        })
    }
}

impl Writer {
    fn native_edit(&mut self, edit: &GuiTextEdit) -> Result<(), ProtocolError> {
        match edit {
            GuiTextEdit::Insert(text) => {
                self.u8(GUI_NATIVE_EDIT_INSERT)?;
                self.string(text)
            }
            GuiTextEdit::Selection(selection) => {
                self.u8(GUI_NATIVE_EDIT_SELECTION)?;
                self.u32(selection[0])?;
                self.u32(selection[1])
            }
            GuiTextEdit::Compose(composition) => {
                self.u8(GUI_NATIVE_EDIT_COMPOSE)?;
                self.string(&composition.text)?;
                self.u32(composition.selection[0])?;
                self.u32(composition.selection[1])
            }
            GuiTextEdit::CommitComposition => self.u8(GUI_NATIVE_EDIT_COMMIT_COMPOSITION),
            GuiTextEdit::CancelComposition => self.u8(GUI_NATIVE_EDIT_CANCEL_COMPOSITION),
            GuiTextEdit::Backspace => self.u8(GUI_NATIVE_EDIT_BACKSPACE),
            GuiTextEdit::Delete => self.u8(GUI_NATIVE_EDIT_DELETE),
            GuiTextEdit::Left => self.u8(GUI_NATIVE_EDIT_LEFT),
            GuiTextEdit::Right => self.u8(GUI_NATIVE_EDIT_RIGHT),
            GuiTextEdit::Home => self.u8(GUI_NATIVE_EDIT_HOME),
            GuiTextEdit::End => self.u8(GUI_NATIVE_EDIT_END),
            GuiTextEdit::SelectAll => self.u8(GUI_NATIVE_EDIT_SELECT_ALL),
            GuiTextEdit::Submit => self.u8(GUI_NATIVE_EDIT_SUBMIT),
        }
    }

    fn native_state(&mut self, state: &GuiNativeTextState) -> Result<(), ProtocolError> {
        self.world_reference(state.fence.target.world.into())?;
        self.u64(state.fence.target.entity.to_bits())?;
        self.u16(state.fence.target.component)?;
        self.u64(state.fence.target.incarnation)?;
        self.u64(state.fence.generation)?;
        self.string(&state.text)?;
        self.u32(state.selection[0])?;
        self.u32(state.selection[1])?;
        self.u8(u8::from(state.masked))?;
        self.u8(u8::from(state.composition.is_some()))?;
        if let Some(composition) = &state.composition {
            self.string(&composition.text)?;
            self.u32(composition.selection[0])?;
            self.u32(composition.selection[1])?;
        }
        Ok(())
    }
}

/// Allocation-free sizing before reserving the same connection's encoding credit.
pub fn native_state_size(state: &GuiNativeTextState) -> Result<usize, ProtocolError> {
    let mut writer = Writer::measuring();
    writer.native_state(state)?;
    Ok(writer.len())
}

/// Encode only after the owner reserved payload plus simultaneous outer framing storage.
pub fn encode_native_state(state: &GuiNativeTextState) -> Result<Vec<u8>, ProtocolError> {
    let mut writer = Writer::new(Vec::with_capacity(native_state_size(state)?));
    writer.native_state(state)?;
    Ok(writer.0)
}
