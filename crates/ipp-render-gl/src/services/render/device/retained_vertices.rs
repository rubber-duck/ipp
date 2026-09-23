//! Byte layouts of retained GUI vertices, shared by both GL devices.
//!
//! Each layout is computed from its `#[repr(C)]` vertex with `offset_of!` and the
//! field array lengths, then validated at compile time: attributes use consecutive
//! shader locations and tile the whole vertex in declaration order. Changing a vertex
//! field therefore fails the build until this table, and the shaders it describes,
//! agree. GLES configures attribute pointers from these tables and the WebGL bridge
//! reads the same `#[repr(C)]` table from WASM memory, so neither restates the layout.

use std::mem::{offset_of, size_of};

use super::{GlyphVertex, GuiBoxVertex};

/// One `f32` vector attribute of a retained vertex.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RetainedVertexAttribute {
    /// Shader input location.
    pub location: u32,
    /// Consecutive `f32` lanes read from `offset`.
    pub components: u32,
    /// Byte offset within one vertex.
    pub offset: u32,
}

/// Stride and attributes of one retained vertex type.
///
/// The WebGL bridge reads this table as consecutive `u32` words: stride, attribute
/// count, then location, components and offset for each attribute.
#[repr(C)]
#[derive(Debug)]
pub(crate) struct RetainedVertexLayout<const N: usize> {
    /// Bytes per vertex.
    pub stride: u32,
    /// Number of attributes that follow.
    pub attribute_count: u32,
    /// Attributes in location order.
    pub attributes: [RetainedVertexAttribute; N],
}

/// Count the lanes of one `[f32; L]` vertex field; other field types do not compile.
const fn f32_lanes<V, const L: usize>(_field: fn(&V) -> &[f32; L]) -> u32 {
    L as u32
}

/// Declare one attribute from a vertex field name and its shader location.
macro_rules! retained_attribute {
    ($vertex:ty, $field:ident, $location:expr) => {
        RetainedVertexAttribute {
            location: $location,
            components: f32_lanes::<$vertex, _>(|vertex| &vertex.$field),
            offset: offset_of!($vertex, $field) as u32,
        }
    };
}

/// Validate that `attributes` tile a `stride`-byte vertex in location order.
const fn validated_layout<const N: usize>(
    stride: usize,
    attributes: [RetainedVertexAttribute; N],
) -> RetainedVertexLayout<N> {
    let mut end = 0;
    let mut index = 0;
    while index < N {
        let attribute = attributes[index];
        assert!(
            attribute.location as usize == index,
            "retained vertex locations must be consecutive from zero"
        );
        assert!(
            attribute.offset as usize == end,
            "retained vertex attributes must follow field order without padding"
        );
        end += attribute.components as usize * size_of::<f32>();
        index += 1;
    }

    assert!(
        end == stride,
        "retained vertex attributes must cover the whole vertex"
    );
    RetainedVertexLayout {
        stride: stride as u32,
        attribute_count: N as u32,
        attributes,
    }
}

/// [`GuiBoxVertex`] inputs of `surface_box.vert`.
pub(crate) const GUI_BOX_VERTEX_LAYOUT: RetainedVertexLayout<9> = validated_layout(
    size_of::<GuiBoxVertex>(),
    [
        retained_attribute!(GuiBoxVertex, position, 0),
        retained_attribute!(GuiBoxVertex, placement, 1),
        retained_attribute!(GuiBoxVertex, shape, 2),
        retained_attribute!(GuiBoxVertex, color0, 3),
        retained_attribute!(GuiBoxVertex, color1, 4),
        retained_attribute!(GuiBoxVertex, border_color, 5),
        retained_attribute!(GuiBoxVertex, gradient_coords, 6),
        retained_attribute!(GuiBoxVertex, material_params, 7),
        retained_attribute!(GuiBoxVertex, glow_color, 8),
    ],
);

/// [`GlyphVertex`] inputs of `surface_text.vert`.
pub(crate) const GLYPH_VERTEX_LAYOUT: RetainedVertexLayout<3> = validated_layout(
    size_of::<GlyphVertex>(),
    [
        retained_attribute!(GlyphVertex, position, 0),
        retained_attribute!(GlyphVertex, uv, 1),
        retained_attribute!(GlyphVertex, color, 2),
    ],
);

// Pinned strides: resizing either vertex changes GPU memory accounting and every
// consumer's stride, so it must be a deliberate edit here as well.
const _: () = assert!(GUI_BOX_VERTEX_LAYOUT.stride == 136);
const _: () = assert!(GLYPH_VERTEX_LAYOUT.stride == 32);

#[cfg(test)]
mod tests {
    use super::*;

    /// `(location, components)` of every `layout(location = N) in vecK` input.
    fn shader_inputs(source: &str) -> Vec<(u32, u32)> {
        source
            .lines()
            .filter_map(|line| {
                let declaration = line.trim().strip_prefix("layout(location = ")?;
                let (location, rest) = declaration.split_once(')')?;
                let components = rest.trim().strip_prefix("in vec")?.get(..1)?;
                Some((location.parse().ok()?, components.parse().ok()?))
            })
            .collect()
    }

    fn layout_inputs<const N: usize>(layout: &RetainedVertexLayout<N>) -> Vec<(u32, u32)> {
        layout
            .attributes
            .iter()
            .map(|attribute| (attribute.location, attribute.components))
            .collect()
    }

    #[test]
    fn shader_inputs_match_retained_vertex_layouts() {
        assert_eq!(
            shader_inputs(crate::services::render::embedded_shader!(
                "shaders/surface_box.vert"
            )),
            layout_inputs(&GUI_BOX_VERTEX_LAYOUT)
        );
        assert_eq!(
            shader_inputs(crate::services::render::embedded_shader!(
                "shaders/surface_text.vert"
            )),
            layout_inputs(&GLYPH_VERTEX_LAYOUT)
        );
    }

    #[test]
    fn bridge_table_is_consecutive_words() {
        assert_eq!(
            size_of::<RetainedVertexLayout<9>>(),
            (2 + 9 * 3) * size_of::<u32>()
        );
        assert_eq!(offset_of!(RetainedVertexLayout<9>, attributes), 8);
        assert_eq!(GUI_BOX_VERTEX_LAYOUT.attributes[8].offset, 120);
        assert_eq!(GLYPH_VERTEX_LAYOUT.attributes[2].offset, 16);
    }

    #[test]
    fn box_shader_pad_matches_generated_geometry() {
        let declared = crate::services::render::embedded_shader!("shaders/surface_box.vert")
            .lines()
            .find_map(|line| {
                line.trim()
                    .strip_prefix("const float GUI_BOX_ANTIALIAS_PAD = ")?
                    .strip_suffix(';')
            })
            .expect("surface_box.vert declares GUI_BOX_ANTIALIAS_PAD");
        assert_eq!(
            declared.parse::<f32>().unwrap(),
            crate::gui_batch::GUI_BOX_ANTIALIAS_PAD
        );
    }
}
