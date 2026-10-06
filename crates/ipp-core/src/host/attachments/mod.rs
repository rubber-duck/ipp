//! Host attachment graph: the derived live topology, exact producer-write receipts and
//! explicit root output bindings.

mod root_binding;
mod tokens;
pub(crate) mod topology;

pub use root_binding::{RootBindingGeneration, RootOutputBinding};
pub use tokens::{WorldAttachmentEffect, WorldAttachmentRetirement, WorldAttachmentToken};
