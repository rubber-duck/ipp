use super::{GuiPartId, GuiPartVariant, GuiPrimitivePart, GuiSkinState};
use crate::components::schema::{ContractSink, write_string};

pub(crate) fn write_paint_contract(sink: &mut impl ContractSink) {
    sink.write(&(GuiPartId::COUNT as u16).to_le_bytes());
    for index in 0..GuiPartId::COUNT {
        let key = GuiPartId::from_index(index).expect("compiled paint key");
        sink.write(&key.index().expect("compiled paint index").to_le_bytes());
        write_string(
            sink,
            match key.part {
                GuiPrimitivePart::Background => "background",
                GuiPrimitivePart::Fill => "fill",
                GuiPrimitivePart::Label => "label",
                GuiPrimitivePart::Icon => "icon",
                GuiPrimitivePart::FocusRing => "focusRing",
                GuiPrimitivePart::ScrollTrackX => "scrollTrackX",
                GuiPrimitivePart::ScrollThumbX => "scrollThumbX",
                GuiPrimitivePart::ScrollTrackY => "scrollTrackY",
                GuiPrimitivePart::ScrollThumbY => "scrollThumbY",
                GuiPrimitivePart::Caret
                | GuiPrimitivePart::Selection
                | GuiPrimitivePart::Composition => unreachable!("not a skin part"),
            },
        );
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
}
