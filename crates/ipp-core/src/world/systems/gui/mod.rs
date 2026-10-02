//! Ordinary-entity GUI: control components, layout, presentation and motion.
//!
//! GUI structure is the core entity tree. Controls are ordinary entities with
//! control components ([`local`]) whose values are ordinary fields;
//! [`layout`] places `GuiLayout` entities over core links and writes scroll
//! geometry; [`presentation`] resolves themes, skins and fonts into control
//! paint, and skins into the Background of other skinned entities, joined to
//! Canvas output; [`motion`] prepares skin transitions that
//! `AnimationSystem` samples; [`observations`] publishes momentary control
//! effects. [`GuiSystem`] owns focus, pointer interaction, native text state
//! and the [`GuiPreferences`], writes control values for actions and input,
//! keeps eligibility fields current and is the sole provider of GUI World
//! operations. Physical input routing lives in the composed router of the GUI
//! input service.
//!
//! Components are re-exported through [`crate::components`]; the
//! [GUI architecture](../../../../../../docs/architecture/gui.md) owns the
//! design.

pub mod layout;
pub mod local;
pub mod motion;
pub mod observations;
mod preferences;
pub mod presentation;
mod system;
#[cfg(test)]
pub(crate) mod test_support;

pub use layout::{GuiLayoutSystem, GuiLayoutSystemFactory, MAX_LAYOUT_DEPTH};
pub use local::MAX_GUI_TEXT_BYTES;
pub(crate) use local::slider::slider_rail;
pub use preferences::{GuiPreferences, GuiPreferencesUpdate};
pub use presentation::{
    GUI_BASE_PARTS, GuiPartId, GuiPartProperty, GuiPartStyle, GuiPartVariant, GuiPrimitivePart,
    GuiSkinLook, GuiSkinState, gui_skin_looks,
};
pub use system::{GuiSystem, GuiSystemFactory};
