//! Ordinary attachment component and its selected producer admission owner.

mod component;
pub use component::WorldAttachment;

mod publication;
mod system;
pub use system::{WorldAttachmentSystem, WorldAttachmentSystemFactory};
