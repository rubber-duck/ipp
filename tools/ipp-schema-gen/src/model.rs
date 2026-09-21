pub(super) struct WireContract {
    pub(super) capabilities: Capabilities,
    pub(super) conventions: Vec<(String, String)>,
    pub(super) layouts: Vec<WireLayout>,
    pub(super) tags: Vec<WireTag>,
    pub(super) asset_formats: Vec<AssetFormat>,
}

pub(super) struct WireTag {
    pub(super) name: String,
    pub(super) space: u8,
    pub(super) capability: u8,
    pub(super) value: u8,
    pub(super) layout: String,
}

pub(super) struct WireLayout {
    pub(super) name: String,
    pub(super) capability: u8,
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
}

pub(super) struct AssetFormat {
    pub(super) name: String,
    pub(super) capability: u8,
    pub(super) type_id: u16,
    pub(super) format: String,
}

pub(super) struct TargetFeature {
    pub(super) id: u8,
    pub(super) name: String,
    pub(super) enabled: bool,
}

#[derive(Clone, Copy, Default)]
pub(super) struct Capabilities {
    pub(super) surfaces: bool,
    pub(super) gui: bool,
    pub(super) animation: bool,
    pub(super) skeletal_animation: bool,
    pub(super) assets: bool,
    pub(super) state_overlays: bool,
    pub(super) spatial: bool,
    pub(super) textures: bool,
    pub(super) builtin_assets: bool,
    pub(super) picking: bool,
    pub(super) debug_geometry: bool,
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
}

pub(super) struct Export {
    pub(super) expected: u64,
    pub(super) arch: String,
    pub(super) os: String,
    pub(super) pointer: u8,
    pub(super) features: Vec<TargetFeature>,
    pub(super) components: Vec<Component>,
    pub(super) wire: WireContract,
}
