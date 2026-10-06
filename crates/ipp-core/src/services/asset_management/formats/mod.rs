//! Typed CPU asset formats with their decoders, encoders and shared building blocks.
//! Binary layouts are documented in the adjacent `*_FORMAT.md` and `*_FORMATS.md` files.

pub mod mesh;

pub mod quadratic;

pub mod font;

pub mod drawing;

pub mod expression;

pub mod mesh_metadata;

pub mod texture;

pub mod skeleton;

pub mod skin_binding;

/// Immutable backend-specific shader definitions.
pub mod shader;
