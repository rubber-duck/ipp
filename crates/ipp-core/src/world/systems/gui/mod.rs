//! Ordinary-entity GUI: control components, layout, presentation and motion.
//!
//! GUI structure is the core entity tree. Controls are ordinary entities with
//! control components ([`local`]) whose values are ordinary fields;
//! [`layout`] places `GuiLayout` entities over core links and writes scroll
//! geometry; [`presentation`] resolves themes, skins and fonts into control
//! paint joined to Canvas output; [`motion`] drives skin transitions that
//! `AnimationSystem` samples; [`observations`] publishes momentary control
//! effects. [`GuiSystem`] owns focus, pointer interaction and native text
//! state, writes control values for actions and input, keeps eligibility
//! fields current and is the sole provider of GUI World operations. Physical
//! input routing lives in the composed router of the GUI input service.
//!
//! Components are re-exported through [`crate::components`]; the
//! [GUI architecture](../../../../../../docs/architecture/gui.md) owns the
//! design.

pub mod layout;
pub mod local;
pub mod motion;
pub mod observations;
pub mod presentation;
mod system;
#[cfg(test)]
pub(crate) mod test_support;

pub use layout::{GuiLayoutSystem, GuiLayoutSystemFactory, MAX_LAYOUT_DEPTH};
pub use local::MAX_GUI_TEXT_BYTES;
pub(crate) use local::slider::slider_rail;
pub use presentation::{
    FOCUS_BORDER_COLOR, FOCUS_BORDER_WIDTH, GUI_BASE_PARTS, GuiPartId, GuiPartProperty,
    GuiPartStyle, GuiPartVariant, GuiPrimitivePart, GuiSkinState,
};
pub use system::{GuiSystem, GuiSystemFactory};
