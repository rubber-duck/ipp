//! Byte layouts of retained GUI records, shared by both GL devices.
//!
//! Each layout is computed from its `#[repr(C)]` record with `offset_of!` and the
//! field array lengths, then validated at compile time: attributes use consecutive
//! shader locations and tile the whole record in declaration order. Changing a record
//! field therefore fails the build until its table, and the shader it describes,
//! agree. Every attribute advances once per instance: a record is one quad, whose
//! corners the vertex shader takes from `gl_VertexID`. GLES configures attribute
//! pointers from these tables and the WebGL bridge reads the same `#[repr(C)]` tables
//! from WASM memory, so neither restates a layout.

use std::mem::{offset_of, size_of};

use super::super::gui_records::{GuiGlyphRecord, GuiRecordKind, GuiShapeRecord};

/// One `f32` vector attribute of a retained record.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RetainedRecordAttribute {
    /// Shader input location.
    pub location: u32,
    /// Consecutive `f32` lanes read from `offset`.
    pub components: u32,
    /// Byte offset within one record.
    pub offset: u32,
}

/// Stride and attributes of one retained record type.
///
/// The WebGL bridge reads this table as consecutive `u32` words: stride, attribute
/// count, then location, components and offset for each attribute.
#[repr(C)]
#[derive(Debug)]
pub(crate) struct RetainedRecordLayout<const N: usize> {
    /// Bytes per record.
    pub stride: u32,
    /// Number of attributes that follow.
    pub attribute_count: u32,
    /// Attributes in location order.
    pub attributes: [RetainedRecordAttribute; N],
}

/// Count the lanes of one `[f32; L]` record field; other field types do not compile.
const fn f32_lanes<V, const L: usize>(_field: fn(&V) -> &[f32; L]) -> u32 {
    L as u32
}

/// Declare one attribute from a record field name and its shader location.
macro_rules! retained_attribute {
    ($record:ty, $field:ident, $location:expr) => {
        RetainedRecordAttribute {
            location: $location,
            components: f32_lanes::<$record, _>(|record| &record.$field),
            offset: offset_of!($record, $field) as u32,
        }
    };
}

/// Validate that `attributes` tile a `stride`-byte record in location order.
const fn validated_layout<const N: usize>(
    stride: usize,
    attributes: [RetainedRecordAttribute; N],
) -> RetainedRecordLayout<N> {
    let mut end = 0;
    let mut index = 0;
    while index < N {
        let attribute = attributes[index];
        assert!(
            attribute.location as usize == index,
            "retained record locations must be consecutive from zero"
        );
        assert!(
            attribute.offset as usize == end,
            "retained record attributes must follow field order without padding"
        );
        end += attribute.components as usize * size_of::<f32>();
        index += 1;
    }

    assert!(
        end == stride,
        "retained record attributes must cover the whole record"
    );
    RetainedRecordLayout {
        stride: stride as u32,
        attribute_count: N as u32,
        attributes,
    }
}

/// [`GuiShapeRecord`] inputs of `surface_gui.vert`.
pub(crate) const GUI_SHAPE_LAYOUT: RetainedRecordLayout<12> = validated_layout(
    size_of::<GuiShapeRecord>(),
    [
        retained_attribute!(GuiShapeRecord, rect, 0),
        retained_attribute!(GuiShapeRecord, placement, 1),
        retained_attribute!(GuiShapeRecord, shape, 2),
        retained_attribute!(GuiShapeRecord, corner_cut, 3),
        retained_attribute!(GuiShapeRecord, corner_accent, 4),
        retained_attribute!(GuiShapeRecord, color0, 5),
        retained_attribute!(GuiShapeRecord, color1, 6),
        retained_attribute!(GuiShapeRecord, border_color, 7),
        retained_attribute!(GuiShapeRecord, gradient_coords, 8),
        retained_attribute!(GuiShapeRecord, material_params, 9),
        retained_attribute!(GuiShapeRecord, glow_color, 10),
        retained_attribute!(GuiShapeRecord, clip, 11),
    ],
);

/// [`GuiGlyphRecord`] inputs of `surface_glyph.vert`.
pub(crate) const GUI_GLYPH_LAYOUT: RetainedRecordLayout<4> = validated_layout(
    size_of::<GuiGlyphRecord>(),
    [
        retained_attribute!(GuiGlyphRecord, rect, 0),
        retained_attribute!(GuiGlyphRecord, uv, 1),
        retained_attribute!(GuiGlyphRecord, color, 2),
        retained_attribute!(GuiGlyphRecord, clip, 3),
    ],
);

// Pinned strides: resizing a record changes GPU memory accounting and every
// consumer's stride, so it must be a deliberate edit here as well. Twelve
// attributes stay within the 16 that WebGL 2 and GLES 3 guarantee.
const _: () = assert!(GUI_SHAPE_LAYOUT.stride == 192);
const _: () = assert!(GUI_GLYPH_LAYOUT.stride == 64);

/// Stride and attributes of `kind`'s records.
pub(crate) fn gui_record_layout(kind: GuiRecordKind) -> (u32, &'static [RetainedRecordAttribute]) {
    match kind {
        GuiRecordKind::Shape => (GUI_SHAPE_LAYOUT.stride, &GUI_SHAPE_LAYOUT.attributes),
        GuiRecordKind::Glyph => (GUI_GLYPH_LAYOUT.stride, &GUI_GLYPH_LAYOUT.attributes),
    }
}

/// The `#[repr(C)]` table of `kind`'s records as the WebGL bridge reads it.
#[cfg(target_arch = "wasm32")]
pub(crate) fn gui_record_table(kind: GuiRecordKind) -> *const u32 {
    match kind {
        GuiRecordKind::Shape => std::ptr::from_ref(&GUI_SHAPE_LAYOUT).cast(),
        GuiRecordKind::Glyph => std::ptr::from_ref(&GUI_GLYPH_LAYOUT).cast(),
    }
}

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

    fn layout_inputs<const N: usize>(layout: &RetainedRecordLayout<N>) -> Vec<(u32, u32)> {
        layout
            .attributes
            .iter()
            .map(|attribute| (attribute.location, attribute.components))
            .collect()
    }

    /// Value of `const float NAME = value;` declared in `source`.
    fn shader_constant(source: &str, name: &str) -> f32 {
        let prefix = format!("const float {name} = ");
        source
            .lines()
            .find_map(|line| line.trim().strip_prefix(&prefix)?.strip_suffix(';'))
            .unwrap_or_else(|| panic!("shader declares {name}"))
            .parse()
            .unwrap()
    }

    const SHAPE_VERTEX: &str =
        crate::services::render::embedded_shader!("shaders/surface_gui.vert");

    const SHAPE_FRAGMENT: &str =
        crate::services::render::embedded_shader!("shaders/surface_gui.frag");

    const GLYPH_VERTEX: &str =
        crate::services::render::embedded_shader!("shaders/surface_glyph.vert");

    #[test]
    fn shader_inputs_match_the_retained_record_layouts() {
        assert_eq!(
            shader_inputs(SHAPE_VERTEX),
            layout_inputs(&GUI_SHAPE_LAYOUT)
        );
        assert_eq!(
            shader_inputs(GLYPH_VERTEX),
            layout_inputs(&GUI_GLYPH_LAYOUT)
        );
    }

    #[test]
    fn bridge_tables_are_consecutive_words() {
        assert_eq!(
            size_of::<RetainedRecordLayout<12>>(),
            (2 + 12 * 3) * size_of::<u32>()
        );
        assert_eq!(offset_of!(RetainedRecordLayout<12>, attributes), 8);
        assert_eq!(GUI_SHAPE_LAYOUT.attributes[10].offset, 160);
        assert_eq!(GUI_SHAPE_LAYOUT.attributes[11].offset, 176);
        assert_eq!(GUI_GLYPH_LAYOUT.attributes[3].offset, 48);
        assert_eq!(gui_record_layout(GuiRecordKind::Glyph).1.len(), 4);
    }

    #[test]
    fn shader_constants_match_generated_geometry() {
        assert_eq!(
            shader_constant(SHAPE_VERTEX, "GUI_BOX_ANTIALIAS_PAD"),
            crate::gui_batch::GUI_BOX_ANTIALIAS_PAD
        );
        assert_eq!(
            shader_constant(SHAPE_FRAGMENT, "GUI_PAINT_STROKE"),
            crate::gui_batch::GUI_PAINT_STROKE
        );
        assert_eq!(
            shader_constant(SHAPE_FRAGMENT, "GUI_PAINT_ARC"),
            crate::gui_batch::GUI_PAINT_ARC
        );
        assert_eq!(
            shader_constant(SHAPE_FRAGMENT, "GUI_PAINT_CHECKER"),
            crate::gui_batch::GUI_PAINT_CHECKER
        );
        assert_eq!(
            shader_constant(SHAPE_FRAGMENT, "GUI_FILL_HUE"),
            crate::gui_batch::GUI_FILL_HUE
        );
        assert_eq!(
            shader_constant(SHAPE_FRAGMENT, "GUI_FILL_SATURATION_VALUE"),
            crate::gui_batch::GUI_FILL_SATURATION_VALUE
        );
        assert_eq!(
            shader_constant(SHAPE_FRAGMENT, "GUI_FILL_PAINT"),
            crate::gui_batch::GUI_FILL_PAINT
        );
        assert_eq!(
            shader_constant(SHAPE_FRAGMENT, "GUI_PAINT_BLOCK_STRIDE"),
            crate::gui_batch::GUI_PAINT_BLOCK_STRIDE
        );
        // A whole ring's half sweep is exactly this value, which the shader's
        // whole-ring test compares against.
        assert_eq!(shader_constant(SHAPE_FRAGMENT, "PI"), std::f32::consts::PI);
    }
}
