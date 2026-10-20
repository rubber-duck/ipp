//! Shared typed input payloads for physical ingress and GUI observation delivery.

use crate::ProtocolError;
use crate::codec::{Reader, Writer};
use crate::contract::wire_manifest::*;
use ipp_core::systems::gui::local::{GUI_CLIENT_TEXT_MAX_BYTES, GuiClientInput};

impl Reader<'_> {
    fn client_input_text(&mut self, max: usize) -> Result<std::sync::Arc<str>, ProtocolError> {
        let length = self.count(max)?;
        std::str::from_utf8(self.take(length)?)
            .map(std::sync::Arc::from)
            .map_err(|_| ProtocolError::Malformed("client input utf8"))
    }

    pub(crate) fn gui_client_input(&mut self) -> Result<GuiClientInput, ProtocolError> {
        let value = match self.u8()? {
            GUI_CLIENT_INPUT_KEY => GuiClientInput::Key {
                key: self.client_input_text(64)?,
                code: self.client_input_text(64)?,
                modifiers: self.u8()?,
                repeat: self.boolean()?,
                pressed: self.boolean()?,
            },
            GUI_CLIENT_INPUT_TEXT => GuiClientInput::Text {
                text: self.client_input_text(GUI_CLIENT_TEXT_MAX_BYTES)?,
                paste: self.boolean()?,
            },
            GUI_CLIENT_INPUT_POINTER => GuiClientInput::Pointer {
                pointer: self.u64()?,
                phase: self.u8()?,
                button: self.u8()?,
                point: [self.f32()?, self.f32()?],
            },
            GUI_CLIENT_INPUT_WHEEL => GuiClientInput::Wheel {
                point: [self.f32()?, self.f32()?],
                delta: [self.f32()?, self.f32()?],
            },
            _ => return Err(ProtocolError::Malformed("client input kind")),
        };
        value
            .validate()
            .map_err(|_| ProtocolError::Malformed("client input value"))?;
        Ok(value)
    }
}

impl Writer {
    pub(crate) fn gui_client_input(&mut self, input: &GuiClientInput) -> Result<(), ProtocolError> {
        input
            .validate()
            .map_err(|_| ProtocolError::Malformed("client input value"))?;
        match input {
            GuiClientInput::Key {
                key,
                code,
                modifiers,
                repeat,
                pressed,
            } => {
                self.u8(GUI_CLIENT_INPUT_KEY)?;
                self.string(key)?;
                self.string(code)?;
                self.u8(*modifiers)?;
                self.u8(u8::from(*repeat))?;
                self.u8(u8::from(*pressed))
            }
            GuiClientInput::Text {
                text,
                paste,
            } => {
                self.u8(GUI_CLIENT_INPUT_TEXT)?;
                self.string(text)?;
                self.u8(u8::from(*paste))
            }
            GuiClientInput::Pointer {
                pointer,
                phase,
                button,
                point,
            } => {
                self.u8(GUI_CLIENT_INPUT_POINTER)?;
                self.u64(*pointer)?;
                self.u8(*phase)?;
                self.u8(*button)?;
                self.f32(point[0])?;
                self.f32(point[1])
            }
            GuiClientInput::Wheel {
                point,
                delta,
            } => {
                self.u8(GUI_CLIENT_INPUT_WHEEL)?;
                for value in point.iter().chain(delta) {
                    self.f32(*value)?;
                }
                Ok(())
            }
        }
    }
}
