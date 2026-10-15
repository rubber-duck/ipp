pub(super) struct WireContract {
    pub(super) conventions: Vec<(String, String)>,
    /// Named numeric bounds that no layout field carries.
    pub(super) limits: Vec<(String, u32)>,
    pub(super) layouts: Vec<WireLayout>,
    pub(super) tags: Vec<WireTag>,
    pub(super) asset_formats: Vec<AssetFormat>,
}

pub(super) struct WireTag {
    pub(super) name: String,
    pub(super) space: u8,
    pub(super) value: u8,
    pub(super) layout: String,
}

pub(super) struct WireLayout {
    pub(super) name: String,
    pub(super) fields: Vec<WireField>,
}

pub(super) struct WireField {
    pub(super) name: String,
    pub(super) encoding: WireEncoding,
    pub(super) limit: u32,
    pub(super) target: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum WireEncoding {
    U16,
    U32,
    U64,
    FiniteF32,
    NonnegativeFiniteF64,
    Utf8,
    Bytes,
    Named,
    List,
    Option,
    Variant,
    Union,
    Bool,
    Masked,
    U8CountedList,
}

pub(super) struct AssetFormat {
    pub(super) name: String,
    pub(super) type_id: u16,
    pub(super) format: String,
}

pub(super) struct Component {
    pub(super) dynamic_properties: bool,
    pub(super) creatable: bool,
    pub(super) id: u16,
    pub(super) name: String,
    pub(super) size: u32,
    pub(super) align: u32,
    pub(super) fields: Vec<Field>,
}

pub(super) struct Field {
    pub(super) name: String,
    pub(super) offset: u32,
    pub(super) field_size: u32,
    pub(super) field_align: u32,
    pub(super) kind: u8,
    pub(super) default: String,
    /// Present exactly for schema rows fields (kind 8).
    pub(super) rows: Option<RowsLayout>,
}

/// Region base and ordered property layout of one schema rows field.
pub(super) struct RowsLayout {
    pub(super) region_base: u32,
    pub(super) properties: Vec<RowProperty>,
}

pub(super) struct RowProperty {
    pub(super) name: String,
    /// `DynamicPropertyKind` tag.
    pub(super) kind: u8,
    pub(super) optional: bool,
    pub(super) rotation: bool,
    /// UTF-8 byte bound, present exactly for text properties (kind 13).
    pub(super) max_bytes: Option<u32>,
}

/// Rows addressing and size bounds declared by the core registry export.
pub(super) struct RowLimits {
    /// Offset span of one rows region; region `k` starts at `(k + 1)` spans.
    pub(super) region_span: u32,
    /// Rows fields one component may declare.
    pub(super) fields: u8,
    /// Properties one row type may declare.
    pub(super) properties: u16,
    /// Largest UTF-8 byte bound of a text row property.
    pub(super) text_bytes: u32,
}

pub(super) struct Export {
    pub(super) expected: u64,
    pub(super) arch: String,
    pub(super) os: String,
    pub(super) pointer: u8,
    pub(super) components: Vec<Component>,
    pub(super) paint_keys: Vec<GuiPaintKey>,
    pub(super) row_limits: RowLimits,
    pub(super) wire: WireContract,
}

pub(super) struct GuiPaintKey {
    pub(super) index: u32,
    pub(super) part: String,
    pub(super) state: String,
    pub(super) variant: String,
}
