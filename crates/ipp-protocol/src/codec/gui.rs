//! Bounded GUI edit and inspection framing.

use super::{ProtocolError, Reader, Writer};
use crate::MAX_MESSAGE_BYTES;
use ipp_core::services::asset_management::{AssetSource, AssetTypeId};
use ipp_core::systems::gui::{
    GuiBlockerHit, GuiCommand, GuiContainerKind, GuiControlValue, GuiInputCommand, GuiInspectQuery,
    GuiInspectResponse, GuiKey, GuiNodeContent, GuiNodeHandle, GuiNodeId, GuiNodePatch,
    GuiNodeStyle, GuiPointerButton,
};

impl Reader<'_> {
    pub(super) fn gui_commands(&mut self) -> Result<Vec<GuiCommand>, ProtocolError> {
        let bytes = self.bytes_bounded(MAX_MESSAGE_BYTES)?;
        let mut r = Reader {
            bytes: &bytes,
            at: 0,
        };
        if r.u8()? != 2 {
            return Err(ProtocolError::Malformed("GUI edit version"));
        }
        let count = r.u32()? as usize;
        // A remove command is the smallest command body: one action byte and
        // one 32-byte node handle. Derive the admission bound from the framed
        // bytes so malformed counts cannot force disproportionate allocation.
        const MIN_COMMAND_BYTES: usize = 33;
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
                let content = r.gui_content()?;
                let style = r.gui_style()?;
                GuiCommand::InsertNode {
                    entity,
                    root_incarnation,
                    id,
                    parent,
                    index,
                    content,
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
        let node_lifetime = self.u32()?;
        Ok(GuiNodeHandle {
            session,
            entity,
            root_incarnation,
            node_id,
            node_lifetime,
        })
    }

    fn gui_node_patch(&mut self) -> Result<GuiNodePatch, ProtocolError> {
        let has_content = self.boolean()?;
        let content = if has_content {
            Some(self.gui_content()?)
        } else {
            None
        };
        let has_style = self.boolean()?;
        if !has_style {
            return Ok(GuiNodePatch {
                content,
                enabled: None,
                width: None,
                height: None,
                min_width: None,
                min_height: None,
                max_width: None,
                max_height: None,
                padding: None,
                margin: None,
                flex: None,
                align_x: None,
                align_y: None,
                color: None,
                background_color: None,
                opacity: None,
                font_size: None,
                asset: None,
            });
        }
        let width = match self.u8()? {
            0 => None,
            1 => Some(None),
            2 => Some(Some(self.f32()?)),
            _ => return Err(ProtocolError::Malformed("GUI patch width")),
        };
        let height = match self.u8()? {
            0 => None,
            1 => Some(None),
            2 => Some(Some(self.f32()?)),
            _ => return Err(ProtocolError::Malformed("GUI patch height")),
        };
        let min_width = match self.u8()? {
            0 => None,
            1 => Some(None),
            2 => Some(Some(self.f32()?)),
            _ => return Err(ProtocolError::Malformed("GUI patch min_width")),
        };
        let min_height = match self.u8()? {
            0 => None,
            1 => Some(None),
            2 => Some(Some(self.f32()?)),
            _ => return Err(ProtocolError::Malformed("GUI patch min_height")),
        };
        let max_width = match self.u8()? {
            0 => None,
            1 => Some(None),
            2 => Some(Some(self.f32()?)),
            _ => return Err(ProtocolError::Malformed("GUI patch max_width")),
        };
        let max_height = match self.u8()? {
            0 => None,
            1 => Some(None),
            2 => Some(Some(self.f32()?)),
            _ => return Err(ProtocolError::Malformed("GUI patch max_height")),
        };
        let padding = match self.u8()? {
            0 => None,
            1 => Some(None),
            2 => Some(Some(self.gui_vector()?)),
            _ => return Err(ProtocolError::Malformed("GUI patch padding")),
        };
        let margin = match self.u8()? {
            0 => None,
            1 => Some(None),
            2 => Some(Some(self.gui_vector()?)),
            _ => return Err(ProtocolError::Malformed("GUI patch margin")),
        };
        let flex = match self.u8()? {
            0 => None,
            1 => Some(None),
            2 => Some(Some(self.f32()?)),
            _ => return Err(ProtocolError::Malformed("GUI patch flex")),
        };
        let align_x = match self.u8()? {
            0 => None,
            1 => Some(None),
            2 => Some(Some(self.f32()?)),
            _ => return Err(ProtocolError::Malformed("GUI patch align_x")),
        };
        let align_y = match self.u8()? {
            0 => None,
            1 => Some(None),
            2 => Some(Some(self.f32()?)),
            _ => return Err(ProtocolError::Malformed("GUI patch align_y")),
        };
        let color = match self.u8()? {
            0 => None,
            1 => Some(self.gui_vector()?),
            _ => return Err(ProtocolError::Malformed("GUI patch color")),
        };
        let background_color = match self.u8()? {
            0 => None,
            1 => Some(None),
            2 => Some(Some(self.gui_vector()?)),
            _ => return Err(ProtocolError::Malformed("GUI patch background_color")),
        };
        let opacity = match self.u8()? {
            0 => None,
            1 => Some(self.f32()?),
            _ => return Err(ProtocolError::Malformed("GUI patch opacity")),
        };
        let font_size = match self.u8()? {
            0 => None,
            1 => Some(self.f32()?),
            _ => return Err(ProtocolError::Malformed("GUI patch font_size")),
        };
        let asset = match self.u8()? {
            0 => None,
            1 => Some(None),
            2 => Some(Some(AssetSource {
                kind: AssetTypeId(self.u16()?),
                variant: self.u32()?,
                uri: self.string()?,
            })),
            _ => return Err(ProtocolError::Malformed("GUI patch asset")),
        };
        // The boolean lane has no cleared state: tag 1 preserves like 0.
        let enabled = match self.u8()? {
            0 | 1 => None,
            2 => Some(self.boolean()?),
            _ => return Err(ProtocolError::Malformed("GUI patch enabled")),
        };

        Ok(GuiNodePatch {
            content,
            enabled,
            width,
            height,
            min_width,
            min_height,
            max_width,
            max_height,
            padding,
            margin,
            flex,
            align_x,
            align_y,
            color,
            background_color,
            opacity,
            font_size,
            asset,
        })
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

    fn gui_vector<const N: usize>(&mut self) -> Result<[f32; N], ProtocolError> {
        let mut values = [0.0; N];
        for value in &mut values {
            *value = self.f32()?;
        }
        Ok(values)
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

    fn gui_content(&mut self) -> Result<GuiNodeContent, ProtocolError> {
        match self.u8()? {
            1 => {
                let kind_byte = self.u8()?;
                let kind = match kind_byte {
                    0 => GuiContainerKind::Row,
                    1 => GuiContainerKind::Column,
                    2 => GuiContainerKind::Stack,
                    3 => GuiContainerKind::Padding,
                    4 => GuiContainerKind::Align,
                    5 => GuiContainerKind::SizedBox,
                    6 => GuiContainerKind::ScrollView,
                    _ => return Err(ProtocolError::Malformed("GUI container kind")),
                };
                Ok(GuiNodeContent::Container(kind))
            }
            2 => Ok(GuiNodeContent::Text(self.string()?)),
            3 => Ok(GuiNodeContent::Drawing),
            4 => Ok(GuiNodeContent::Image {
                size: self.gui_vector()?,
            }),
            5 => Ok(GuiNodeContent::Button {
                label: self.string()?,
            }),
            6 => Ok(GuiNodeContent::Checkbox {
                checked: self.boolean()?,
            }),
            7 => {
                let value = self.f32()?;
                let min = self.f32()?;
                let max = self.f32()?;
                let step = self.f32()?;
                Ok(GuiNodeContent::Slider {
                    value,
                    min,
                    max,
                    step,
                })
            }
            8 => {
                let text = self.string()?;
                let placeholder = self.string()?;
                Ok(GuiNodeContent::TextInput {
                    text,
                    placeholder,
                })
            }
            _ => Err(ProtocolError::Malformed("GUI content tag")),
        }
    }

    fn gui_style(&mut self) -> Result<GuiNodeStyle, ProtocolError> {
        let mask = self.u16()?;
        let width = if mask & (1 << 0) != 0 {
            Some(self.f32()?)
        } else {
            None
        };
        let height = if mask & (1 << 1) != 0 {
            Some(self.f32()?)
        } else {
            None
        };
        let min_width = if mask & (1 << 2) != 0 {
            Some(self.f32()?)
        } else {
            None
        };
        let min_height = if mask & (1 << 3) != 0 {
            Some(self.f32()?)
        } else {
            None
        };
        let max_width = if mask & (1 << 4) != 0 {
            Some(self.f32()?)
        } else {
            None
        };
        let max_height = if mask & (1 << 5) != 0 {
            Some(self.f32()?)
        } else {
            None
        };
        let padding = if mask & (1 << 6) != 0 {
            Some(self.gui_vector()?)
        } else {
            None
        };
        let margin = if mask & (1 << 7) != 0 {
            Some(self.gui_vector()?)
        } else {
            None
        };
        let flex = if mask & (1 << 8) != 0 {
            Some(self.f32()?)
        } else {
            None
        };
        let align_x = if mask & (1 << 9) != 0 {
            Some(self.f32()?)
        } else {
            None
        };
        let align_y = if mask & (1 << 10) != 0 {
            Some(self.f32()?)
        } else {
            None
        };
        let color = self.gui_vector()?;
        let background_color = if mask & (1 << 11) != 0 {
            Some(self.gui_vector()?)
        } else {
            None
        };
        let opacity = self.f32()?;
        let font_size = self.f32()?;
        let asset = if mask & (1 << 12) != 0 {
            Some(AssetSource {
                kind: AssetTypeId(self.u16()?),
                variant: self.u32()?,
                uri: self.string()?,
            })
        } else {
            None
        };
        // The lane is a plain bool: absent bits default to interactive.
        let enabled = if mask & (1 << 13) != 0 {
            self.boolean()?
        } else {
            true
        };

        Ok(GuiNodeStyle {
            enabled,
            width,
            height,
            min_width,
            min_height,
            max_width,
            max_height,
            padding,
            margin,
            flex,
            align_x,
            align_y,
            color,
            background_color,
            opacity,
            font_size,
            asset,
        })
    }
}

impl Writer {
    pub(super) fn gui_inspect_response(
        &mut self,
        response: &GuiInspectResponse,
    ) -> Result<(), ProtocolError> {
        let mut buf = vec![1u8];
        buf.extend(response.root_entity.to_bits().to_le_bytes());
        buf.extend(response.root_incarnation.to_le_bytes());
        buf.extend((response.nodes.len() as u32).to_le_bytes());
        for node in &response.nodes {
            buf.extend(node.id.0.to_le_bytes());
            buf.extend(node.parent.map(|p| p.0).unwrap_or(0).to_le_bytes());
            buf.extend(node.lifetime.to_le_bytes());
            buf.extend(node.control_revision.to_le_bytes());
            buf.extend((node.children.len() as u32).to_le_bytes());
            for child in &node.children {
                buf.extend(child.0.to_le_bytes());
            }

            encode_node_content_bytes(&mut buf, &node.content)?;
            encode_control_value_bytes(&mut buf, &node.control_value)?;
            encode_node_style_bytes(&mut buf, &node.style)?;
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

fn encode_node_content_bytes(
    buf: &mut Vec<u8>,
    content: &GuiNodeContent,
) -> Result<(), ProtocolError> {
    match content {
        GuiNodeContent::Container(kind) => {
            buf.push(1);
            buf.push(*kind as u8);
        }
        GuiNodeContent::Text(text) => {
            buf.push(2);
            encode_gui_text_bytes(buf, text)?;
        }
        GuiNodeContent::Drawing => {
            buf.push(3);
        }
        GuiNodeContent::Image {
            size,
        } => {
            buf.push(4);
            for v in size {
                buf.extend(v.to_le_bytes());
            }
        }
        GuiNodeContent::Button {
            label,
        } => {
            buf.push(5);
            encode_gui_text_bytes(buf, label)?;
        }
        GuiNodeContent::Checkbox {
            checked,
        } => {
            buf.push(6);
            buf.push(u8::from(*checked));
        }
        GuiNodeContent::Slider {
            value,
            min,
            max,
            step,
        } => {
            buf.push(7);
            buf.extend(value.to_le_bytes());
            buf.extend(min.to_le_bytes());
            buf.extend(max.to_le_bytes());
            buf.extend(step.to_le_bytes());
        }
        GuiNodeContent::TextInput {
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

fn encode_node_style_bytes(buf: &mut Vec<u8>, style: &GuiNodeStyle) -> Result<(), ProtocolError> {
    let mut mask: u16 = 0;
    if style.width.is_some() {
        mask |= 1 << 0;
    }
    if style.height.is_some() {
        mask |= 1 << 1;
    }
    if style.min_width.is_some() {
        mask |= 1 << 2;
    }
    if style.min_height.is_some() {
        mask |= 1 << 3;
    }
    if style.max_width.is_some() {
        mask |= 1 << 4;
    }
    if style.max_height.is_some() {
        mask |= 1 << 5;
    }
    if style.padding.is_some() {
        mask |= 1 << 6;
    }
    if style.margin.is_some() {
        mask |= 1 << 7;
    }
    if style.flex.is_some() {
        mask |= 1 << 8;
    }
    if style.align_x.is_some() {
        mask |= 1 << 9;
    }
    if style.align_y.is_some() {
        mask |= 1 << 10;
    }
    if style.background_color.is_some() {
        mask |= 1 << 11;
    }
    if style.asset.is_some() {
        mask |= 1 << 12;
    }
    // `enabled` is a plain bool, always present on retained styles.
    mask |= 1 << 13;

    buf.extend(mask.to_le_bytes());
    if let Some(v) = style.width {
        buf.extend(v.to_le_bytes());
    }
    if let Some(v) = style.height {
        buf.extend(v.to_le_bytes());
    }
    if let Some(v) = style.min_width {
        buf.extend(v.to_le_bytes());
    }
    if let Some(v) = style.min_height {
        buf.extend(v.to_le_bytes());
    }
    if let Some(v) = style.max_width {
        buf.extend(v.to_le_bytes());
    }
    if let Some(v) = style.max_height {
        buf.extend(v.to_le_bytes());
    }
    if let Some(v) = style.padding {
        for f in v {
            buf.extend(f.to_le_bytes());
        }
    }
    if let Some(v) = style.margin {
        for f in v {
            buf.extend(f.to_le_bytes());
        }
    }
    if let Some(v) = style.flex {
        buf.extend(v.to_le_bytes());
    }
    if let Some(v) = style.align_x {
        buf.extend(v.to_le_bytes());
    }
    if let Some(v) = style.align_y {
        buf.extend(v.to_le_bytes());
    }
    for f in style.color {
        buf.extend(f.to_le_bytes());
    }
    if let Some(v) = style.background_color {
        for f in v {
            buf.extend(f.to_le_bytes());
        }
    }
    buf.extend(style.opacity.to_le_bytes());
    buf.extend(style.font_size.to_le_bytes());
    if let Some(asset) = &style.asset {
        buf.extend(asset.kind.0.to_le_bytes());
        buf.extend(asset.variant.to_le_bytes());
        encode_gui_text_bytes(buf, &asset.uri)?;
    }
    buf.push(u8::from(style.enabled));
    Ok(())
}
