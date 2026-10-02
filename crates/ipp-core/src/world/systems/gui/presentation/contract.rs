use super::looks::{gui_skin_looks, gui_skin_tokens};
use super::{GuiPartId, GuiPartProperty, GuiPartVariant, GuiSkinState};
use crate::DynamicValue;
use crate::components::rows::SchemaRow;
use crate::components::schema::{ContractSink, write_string};
use crate::systems::gui::motion::GuiMotionPart;

/// Stream the compiled paint part keys, then every built-in skin look as data:
/// its name, its `em`, its appearance rows and its motion rows, each a part key
/// index and the properties it sets as name, lane count and `f32` lanes; then
/// the design language's tokens, each a name, lane count and `f32` lanes.
pub(crate) fn write_paint_contract(sink: &mut impl ContractSink) {
    sink.write(&(GuiPartId::COUNT as u16).to_le_bytes());
    for index in 0..GuiPartId::COUNT {
        let key = GuiPartId::from_index(index).expect("compiled paint key");
        sink.write(&key.index().expect("compiled paint index").to_le_bytes());
        write_string(sink, key.part.as_str());
        write_string(
            sink,
            match key.state {
                None => "",
                Some(GuiSkinState::Idle) => "idle",
                Some(GuiSkinState::Hovered) => "hovered",
                Some(GuiSkinState::Pressed) => "pressed",
                Some(GuiSkinState::Disabled) => "disabled",
            },
        );
        write_string(
            sink,
            match key.variant {
                None => "",
                Some(GuiPartVariant::Checked) => "checked",
                Some(GuiPartVariant::Unchecked) => "unchecked",
            },
        );
    }

    let looks = gui_skin_looks();
    sink.write(&(looks.len() as u16).to_le_bytes());
    for look in looks {
        write_string(sink, look.name);
        sink.write(&look.em.to_le_bytes());
        sink.write(&(look.parts.len() as u16).to_le_bytes());
        for row in &look.parts {
            sink.write(&row.part.to_le_bytes());
            let properties: Vec<_> = GuiPartProperty::ALL
                .into_iter()
                .filter_map(|property| {
                    let value = row.property(property.index()).expect("appearance lane")?;
                    Some((property.name(), value))
                })
                .collect();
            sink.write(&[properties.len() as u8]);
            for (name, value) in properties {
                write_string(sink, name);
                let lanes: &[f32] = match &value {
                    DynamicValue::F32(value) => std::slice::from_ref(value),
                    DynamicValue::Vec2(value) => value,
                    DynamicValue::Vec4(value) => value,
                    other => unreachable!("built-in looks hold numeric lanes, not {other:?}"),
                };
                sink.write(&[lanes.len() as u8]);
                for lane in lanes {
                    sink.write(&lane.to_le_bytes());
                }
            }
        }

        sink.write(&(look.motion.len() as u16).to_le_bytes());
        for row in &look.motion {
            sink.write(&row.part.to_le_bytes());
            let properties: Vec<_> = GuiMotionPart::LAYOUT
                .properties
                .iter()
                .enumerate()
                .filter(|(_, property)| property.name != "part")
                .filter_map(|(index, property)| {
                    let value = row.property(index as u32).expect("motion lane")?;
                    Some((property.name, value))
                })
                .collect();
            sink.write(&[properties.len() as u8]);
            for (name, value) in properties {
                write_string(sink, name);
                let lane = match value {
                    DynamicValue::F32(value) => value,
                    DynamicValue::U32(value) => value as f32,
                    other => unreachable!("motion rows hold scalars, not {other:?}"),
                };
                sink.write(&[1]);
                sink.write(&lane.to_le_bytes());
            }
        }
    }

    let tokens = gui_skin_tokens();
    sink.write(&(tokens.len() as u16).to_le_bytes());
    for token in tokens {
        write_string(sink, token.name);
        let lanes = token.value.lanes();
        sink.write(&[lanes.len() as u8]);
        for lane in lanes {
            sink.write(&lane.to_le_bytes());
        }
    }
}
