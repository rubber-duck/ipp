//! Bounded GUI edit and inspection framing.

use super::{ProtocolError, Reader, Writer};
use crate::MAX_MESSAGE_BYTES;
use ipp_core::components::rows::{SchemaRow, decode_row, decode_row_value, encode_row};
use ipp_core::systems::gui::{
    GuiBlockerHit, GuiCommand, GuiContainerKind, GuiControlValue, GuiInputCommand, GuiInspectQuery,
    GuiInspectResponse, GuiKey, GuiNodeData, GuiNodeDataRow, GuiNodeHandle, GuiNodeId,
    GuiNodePatch, GuiNodeStyle, GuiNodeStyleProperty, GuiNodeStyleRow, GuiPointerButton,
};

/// GUI edit framing version; see the `gui-edit` wire convention.
const GUI_EDIT_VERSION: u8 = 3;
/// GUI inspection response framing version.
const GUI_INSPECT_VERSION: u8 = 2;

impl Reader<'_> {
    pub(super) fn gui_commands(&mut self) -> Result<Vec<GuiCommand>, ProtocolError> {
        let bytes = self.bytes_bounded(MAX_MESSAGE_BYTES)?;
        let mut r = Reader {
            bytes: &bytes,
            at: 0,
        };
        if r.u8()? != GUI_EDIT_VERSION {
            return Err(ProtocolError::Malformed("GUI edit version"));
        }
        let count = r.u32()? as usize;
        // A remove command is the smallest command body: one action byte and
        // one 28-byte node handle. Derive the admission bound from the framed
        // bytes so malformed counts cannot force disproportionate allocation.
        const MIN_COMMAND_BYTES: usize = 29;
        if count == 0 || count > bytes.len().saturating_sub(r.at) / MIN_COMMAND_BYTES {
            return Err(ProtocolError::Malformed("GUI edit count"));
        }
        let mut commands = Vec::with_capacity(count);
        for _ in 0..count {
            commands.push(r.gui_command_body()?);
        }
        if r.at != bytes.len() {
            return Err(ProtocolError::Malformed("trailing GUI edit bytes"));
        }
        Ok(commands)
    }

    fn gui_command_body(&mut self) -> Result<GuiCommand, ProtocolError> {
        let r = self;
        let action = r.u8()?;
        let command = match action {
            1 => {
                let entity = ipp_core::EntityId::from_bits(r.u64()?);
                let root_incarnation = r.u64()?;
                let id = GuiNodeId(r.u32()?);
                let has_parent = r.boolean()?;
                let parent = if has_parent {
                    Some(GuiNodeId(r.u32()?))
                } else {
                    None
                };
                let index = r.u32()?;
                let data = r.gui_data()?;
                let values = r.gui_row::<GuiNodeDataRow>()?;
                let style = GuiNodeStyle::from(&r.gui_row::<GuiNodeStyleRow>()?);
                GuiCommand::InsertNode {
                    entity,
                    root_incarnation,
                    id,
                    parent,
                    index,
                    data,
                    values,
                    style,
                }
            }
            2 => {
                let handle = r.gui_node_handle()?;
                let patch = r.gui_node_patch()?;
                GuiCommand::UpdateNode {
                    handle,
                    patch,
                }
            }
            3 => {
                let handle = r.gui_node_handle()?;
                let has_parent = r.boolean()?;
                let parent = if has_parent {
                    Some(GuiNodeId(r.u32()?))
                } else {
                    None
                };
                let index = r.u32()?;
                GuiCommand::MoveNode {
                    handle,
                    parent,
                    index,
                }
            }
            4 => {
                let handle = r.gui_node_handle()?;
                GuiCommand::RemoveNode {
                    handle,
                }
            }
            5 => {
                let handle = r.gui_node_handle()?;
                let expected_revision = r.u32()?;
                let value = r.gui_control_value()?;
                GuiCommand::SetControlValue {
                    handle,
                    expected_revision,
                    value,
                }
            }
            _ => return Err(ProtocolError::Malformed("GUI edit action")),
        };
        Ok(command)
    }

    pub(super) fn gui_input_command(&mut self) -> Result<GuiInputCommand, ProtocolError> {
        let bytes = self.bytes_bounded(MAX_MESSAGE_BYTES)?;
        let mut r = Reader {
            bytes: &bytes,
            at: 0,
        };
        if r.u8()? != 1 {
            return Err(ProtocolError::Malformed("GUI input version"));
        }
        let command = match r.u8()? {
            1 => GuiInputCommand::PointerDown {
                pointer: r.u32()?,
                panel: r.gui_input_panel()?,
                position: [r.f32()?, r.f32()?],
                button: r.gui_input_button()?,
                blockers: r.gui_input_blockers()?,
                panel_distance: r.gui_input_distance()?,
            },
            2 => GuiInputCommand::PointerUp {
                pointer: r.u32()?,
                panel: r.gui_input_panel()?,
                position: [r.f32()?, r.f32()?],
                button: r.gui_input_button()?,
                blockers: r.gui_input_blockers()?,
                panel_distance: r.gui_input_distance()?,
            },
            3 => GuiInputCommand::PointerMove {
                pointer: r.u32()?,
                panel: r.gui_input_panel()?,
                position: [r.f32()?, r.f32()?],
                blockers: r.gui_input_blockers()?,
                panel_distance: r.gui_input_distance()?,
            },
            4 => GuiInputCommand::PointerCancel {
                pointer: r.u32()?,
            },
            5 => GuiInputCommand::Scroll {
                panel: r.gui_input_panel()?,
                position: [r.f32()?, r.f32()?],
                delta: [r.f32()?, r.f32()?],
                blockers: r.gui_input_blockers()?,
                panel_distance: r.gui_input_distance()?,
            },
            6 => GuiInputCommand::Key {
                key: r.gui_input_key()?,
                pressed: r.boolean()?,
            },
            7 => GuiInputCommand::Text {
                text: r.string()?,
            },
            8 => GuiInputCommand::Focus {
                handle: r.gui_node_handle()?,
            },
            9 => GuiInputCommand::Blur,
            10 => GuiInputCommand::SetTextSelection {
                start: r.u32()?,
                end: r.u32()?,
            },
            11 => GuiInputCommand::UpdateComposition {
                text: r.string()?,
                caret_start: r.u32()?,
                caret_end: r.u32()?,
            },
            12 => GuiInputCommand::CommitComposition,
            13 => GuiInputCommand::CancelComposition,
            _ => return Err(ProtocolError::Malformed("GUI input action")),
        };
        if r.at != bytes.len() {
            return Err(ProtocolError::Malformed("trailing GUI input bytes"));
        }
        Ok(command)
    }

    fn gui_input_panel(&mut self) -> Result<Option<ipp_core::EntityId>, ProtocolError> {
        Ok(if self.boolean()? {
            let entity = self.u64()?;
            if entity == 0 {
                return Err(ProtocolError::Malformed("GUI input panel entity"));
            }
            Some(ipp_core::EntityId::from_bits(entity))
        } else {
            None
        })
    }

    fn gui_input_button(&mut self) -> Result<GuiPointerButton, ProtocolError> {
        match self.u8()? {
            0 => Ok(GuiPointerButton::Primary),
            1 => Ok(GuiPointerButton::Secondary),
            2 => Ok(GuiPointerButton::Auxiliary),
            _ => Err(ProtocolError::Malformed("GUI input button")),
        }
    }

    fn gui_input_blockers(&mut self) -> Result<Vec<GuiBlockerHit>, ProtocolError> {
        let count = self.count(1024)?;
        let mut blockers = Vec::with_capacity(count);
        for _ in 0..count {
            let entity = self.u64()?;
            if entity == 0 {
                return Err(ProtocolError::Malformed("GUI input blocker entity"));
            }
            blockers.push(GuiBlockerHit {
                entity: ipp_core::EntityId::from_bits(entity),
                distance: self.f32()?,
            });
        }
        Ok(blockers)
    }

    fn gui_input_distance(&mut self) -> Result<Option<f32>, ProtocolError> {
        Ok(if self.boolean()? {
            Some(self.f32()?)
        } else {
            None
        })
    }

    fn gui_input_key(&mut self) -> Result<GuiKey, ProtocolError> {
        match self.u8()? {
            0 => Ok(GuiKey::Tab),
            1 => Ok(GuiKey::Enter),
            2 => Ok(GuiKey::Space),
            3 => Ok(GuiKey::Escape),
            4 => Ok(GuiKey::Backspace),
            5 => Ok(GuiKey::Delete),
            6 => Ok(GuiKey::Left),
            7 => Ok(GuiKey::Right),
            8 => Ok(GuiKey::Up),
            9 => Ok(GuiKey::Down),
            10 => Ok(GuiKey::Home),
            11 => Ok(GuiKey::End),
            _ => Err(ProtocolError::Malformed("GUI input key")),
        }
    }

    fn gui_node_handle(&mut self) -> Result<GuiNodeHandle, ProtocolError> {
        let session = self.u64()?;
        let entity = ipp_core::EntityId::from_bits(self.u64()?);
        let root_incarnation = self.u64()?;
        let node_id = GuiNodeId(self.u32()?);
        Ok(GuiNodeHandle {
            session,
            entity,
            root_incarnation,
            node_id,
        })
    }

    /// Patch body: optional data, optional data row, then the style changes
    /// as a changed mask and a set mask over the style row layout followed by
    /// the set values in layout order; changed-but-unset clears an optional
    /// property.
    fn gui_node_patch(&mut self) -> Result<GuiNodePatch, ProtocolError> {
        let mut patch = GuiNodePatch {
            data: if self.boolean()? {
                Some(self.gui_data()?)
            } else {
                None
            },
            values: if self.boolean()? {
                Some(self.gui_row::<GuiNodeDataRow>()?)
            } else {
                None
            },
            ..GuiNodePatch::default()
        };
        let layout = GuiNodeStyleRow::LAYOUT;
        let changed = self.take(layout.mask_bytes())?.to_vec();
        let set = self.take(layout.mask_bytes())?.to_vec();
        let bit = |mask: &[u8], index: usize| mask[index / 8] & (1 << (index % 8)) != 0;
        if let Some(last) = changed.last()
            && layout.properties.len() % 8 != 0
            && (last >> (layout.properties.len() % 8) != 0
                || set[set.len() - 1] >> (layout.properties.len() % 8) != 0)
        {
            return Err(ProtocolError::Malformed("GUI patch mask"));
        }
        for property in GuiNodeStyleProperty::ALL {
            let index = property.index() as usize;
            let change = match (bit(&changed, index), bit(&set, index)) {
                (false, false) => continue,
                (false, true) => return Err(ProtocolError::Malformed("GUI patch mask")),
                (true, false) => Some(None),
                (true, true) => Some(Some(self.gui_row_value(property.kind())?)),
            };
            patch
                .set_style_change(property, change)
                .map_err(|_| ProtocolError::Malformed("GUI patch value"))?;
        }
        Ok(patch)
    }

    /// One row in the table's per-row encoding, validated by its layout.
    fn gui_row<R: SchemaRow>(&mut self) -> Result<R, ProtocolError> {
        let mut rest = &self.bytes[self.at..];
        let before = rest.len();
        let row = decode_row::<R>(&mut rest).map_err(|_| ProtocolError::Malformed("GUI row"))?;
        self.at += before - rest.len();
        Ok(row)
    }

    fn gui_row_value(
        &mut self,
        kind: ipp_core::DynamicPropertyKind,
    ) -> Result<ipp_core::DynamicValue, ProtocolError> {
        let mut rest = &self.bytes[self.at..];
        let before = rest.len();
        let value =
            decode_row_value(kind, &mut rest).map_err(|_| ProtocolError::Malformed("GUI value"))?;
        self.at += before - rest.len();
        Ok(value)
    }

    pub(super) fn gui_inspect_query(&mut self) -> Result<GuiInspectQuery, ProtocolError> {
        let bytes = self.bytes()?;
        let mut r = Reader {
            bytes: &bytes,
            at: 0,
        };
        if r.u8()? != 1 {
            return Err(ProtocolError::Malformed("GUI inspect query version"));
        }
        let entity = ipp_core::EntityId::from_bits(r.u64()?);
        let has_node = r.boolean()?;
        let node_id = if has_node {
            Some(GuiNodeId(r.u32()?))
        } else {
            None
        };
        let max_depth = r.u32()?;
        let limit = r.u32()?;
        if r.at != bytes.len() {
            return Err(ProtocolError::Malformed("trailing GUI inspect query bytes"));
        }
        Ok(GuiInspectQuery {
            entity,
            node_id,
            max_depth,
            limit,
        })
    }

    fn gui_control_value(&mut self) -> Result<GuiControlValue, ProtocolError> {
        match self.u8()? {
            0 => Ok(GuiControlValue::None),
            1 => Ok(GuiControlValue::Bool(self.boolean()?)),
            2 => Ok(GuiControlValue::Scalar(self.f32()?)),
            3 => Ok(GuiControlValue::Text(self.string()?)),
            _ => Err(ProtocolError::Malformed("GUI control value tag")),
        }
    }

    fn gui_data(&mut self) -> Result<GuiNodeData, ProtocolError> {
        Ok(match self.u8()? {
            1 => GuiNodeData::Container(match self.u8()? {
                0 => GuiContainerKind::Row,
                1 => GuiContainerKind::Column,
                2 => GuiContainerKind::Stack,
                3 => GuiContainerKind::Padding,
                4 => GuiContainerKind::Align,
                5 => GuiContainerKind::SizedBox,
                6 => GuiContainerKind::ScrollView,
                _ => return Err(ProtocolError::Malformed("GUI container kind")),
            }),
            2 => GuiNodeData::Text(self.string()?),
            3 => GuiNodeData::Drawing,
            4 => GuiNodeData::Image,
            5 => GuiNodeData::Button {
                label: self.string()?,
            },
            6 => GuiNodeData::Checkbox,
            7 => GuiNodeData::Slider,
            8 => GuiNodeData::TextInput {
                text: self.string()?,
                placeholder: self.string()?,
            },
            _ => return Err(ProtocolError::Malformed("GUI data tag")),
        })
    }
}

impl Writer {
    pub(super) fn gui_inspect_response(
        &mut self,
        response: &GuiInspectResponse,
    ) -> Result<(), ProtocolError> {
        let mut buf = vec![GUI_INSPECT_VERSION];
        buf.extend(response.root_entity.to_bits().to_le_bytes());
        buf.extend(response.root_incarnation.to_le_bytes());
        buf.extend((response.nodes.len() as u32).to_le_bytes());
        for node in &response.nodes {
            buf.extend(node.id.0.to_le_bytes());
            buf.extend(node.parent.map(|p| p.0).unwrap_or(0).to_le_bytes());
            buf.extend(node.control_revision.to_le_bytes());
            buf.extend((node.children.len() as u32).to_le_bytes());
            for child in &node.children {
                buf.extend(child.0.to_le_bytes());
            }

            encode_node_data_bytes(&mut buf, &node.data)?;
            encode_row(&node.values, &mut buf);
            encode_control_value_bytes(&mut buf, &node.control_value)?;
            encode_row(&GuiNodeStyleRow::from(&node.style), &mut buf);
        }
        self.count(buf.len(), MAX_MESSAGE_BYTES)?;
        self.raw(&buf)?;
        Ok(())
    }
}

fn encode_gui_text_bytes(buf: &mut Vec<u8>, text: &str) -> Result<(), ProtocolError> {
    if text.len() > ipp_core::MAX_GUI_TEXT_BYTES {
        return Err(ProtocolError::Limit("GUI text"));
    }
    buf.extend((text.len() as u32).to_le_bytes());
    buf.extend(text.as_bytes());
    Ok(())
}

fn encode_node_data_bytes(buf: &mut Vec<u8>, data: &GuiNodeData) -> Result<(), ProtocolError> {
    match data {
        GuiNodeData::Container(kind) => {
            buf.push(1);
            buf.push(*kind as u8);
        }
        GuiNodeData::Text(text) => {
            buf.push(2);
            encode_gui_text_bytes(buf, text)?;
        }
        GuiNodeData::Drawing => buf.push(3),
        GuiNodeData::Image => buf.push(4),
        GuiNodeData::Button {
            label,
        } => {
            buf.push(5);
            encode_gui_text_bytes(buf, label)?;
        }
        GuiNodeData::Checkbox => buf.push(6),
        GuiNodeData::Slider => buf.push(7),
        GuiNodeData::TextInput {
            text,
            placeholder,
        } => {
            buf.push(8);
            encode_gui_text_bytes(buf, text)?;
            encode_gui_text_bytes(buf, placeholder)?;
        }
    }
    Ok(())
}

fn encode_control_value_bytes(
    buf: &mut Vec<u8>,
    value: &GuiControlValue,
) -> Result<(), ProtocolError> {
    match value {
        GuiControlValue::None => buf.push(0),
        GuiControlValue::Bool(b) => {
            buf.push(1);
            buf.push(u8::from(*b));
        }
        GuiControlValue::Scalar(s) => {
            buf.push(2);
            buf.extend(s.to_le_bytes());
        }
        GuiControlValue::Text(t) => {
            buf.push(3);
            encode_gui_text_bytes(buf, t)?;
        }
    }
    Ok(())
}
