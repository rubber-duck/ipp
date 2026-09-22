//! Canonical, machine-readable wire contract for codecs and SDK generation.

use ipp_core::components::schema::{ContractSink, write_string};

pub(crate) const HOST_REQUEST_MAGIC_HEX: &str = "4950504801000000";
pub(crate) const HOST_RESPONSE_MAGIC_HEX: &str = "4950504101000000";

pub(crate) const fn host_magic(hex: &str) -> [u8; 8] {
    const fn digit(value: u8) -> u8 {
        match value {
            b'0'..=b'9' => value - b'0',
            b'a'..=b'f' => value - b'a' + 10,
            _ => panic!("invalid declared magic"),
        }
    }

    let bytes = hex.as_bytes();
    assert!(bytes.len() == 16);
    let mut output = [0; 8];
    let mut i = 0;
    while i < 8 {
        output[i] = digit(bytes[i * 2]) * 16 + digit(bytes[i * 2 + 1]);
        i += 1;
    }
    output
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Capability {
    Base = 0,
    StateOverlays = 1,
    Spatial = 2,
    Textures = 3,
    BuiltinAssets = 4,
    Picking = 5,
    DebugGeometry = 6,
    Animation = 7,
    Assets = 8,
}

impl Capability {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::StateOverlays => "state-overlays",
            Self::Spatial => "spatial",
            Self::Textures => "textures",
            Self::BuiltinAssets => "builtin-assets",
            Self::Picking => "picking",
            Self::DebugGeometry => "debug-geometry",
            Self::Animation => "animation",
            Self::Assets => "assets",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
#[allow(dead_code)]
pub(crate) enum TagSpace {
    Value = 1,
    Request = 2,
    Command = 3,
    Reference = 4,
    Response = 5,
    Outcome = 6,
    Option = 8,
    EntityOverlayMode = 9,
    ComponentOverlayMode = 10,
    StateOverlayHandleKind = 11,
    StateOverlayLifecycleReason = 12,
    AssetResourceStatus = 13,
    SnapshotValue = 15,
    SnapshotReference = 16,
    CameraMotion = 17,
    GeometryPickOutcome = 18,
    LifecycleObservation = 19,
    BatchErrorScope = 20,
    RuntimeFailureScope = 21,
    InspectionCollection = 22,
    HostRequest = 23,
    HostResponse = 24,
    WorldSelector = 25,
    AnimationTarget = 26,
    PlaybackControl = 27,
    PlaybackState = 28,
    PlaybackEvent = 29,
    AnimationTransitionEasing = 30,
    AnimationTransitionStartTime = 31,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum FieldEncoding {
    U16 = 2,
    U32 = 3,
    U64 = 4,
    FiniteF32 = 5,
    NonnegativeFiniteF64 = 6,
    Utf8 = 7,
    Bytes = 8,
    Named = 9,
    List = 10,
    Option = 11,
    Variant = 12,
    Union = 13,
    Bool = 14,
    Masked = 15,
}

/// Byte fields bounded only by the complete message budget, declared once from
/// [`crate::MAX_MESSAGE_BYTES`] so manifests, encoders and generated codecs agree.
pub(crate) const MESSAGE_BYTES: u32 = crate::MAX_MESSAGE_BYTES as u32;
const _: () = assert!(crate::MAX_MESSAGE_BYTES == MESSAGE_BYTES as usize);

#[derive(Clone, Copy, Debug)]
pub(crate) struct WireField {
    pub(crate) name: &'static str,
    pub(crate) encoding: FieldEncoding,
    /// Maximum byte/count value for bounded encodings; zero means not applicable.
    pub(crate) limit: u32,
    /// Referenced layout or tag-space name for composite encodings.
    pub(crate) target: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WireLayout {
    pub(crate) name: &'static str,
    pub(crate) capability: Capability,
    pub(crate) fields: &'static [WireField],
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WireTag {
    pub(crate) name: &'static str,
    pub(crate) space: TagSpace,
    pub(crate) capability: Capability,
    pub(crate) value: u8,
    pub(crate) layout: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AssetFormat {
    pub(crate) name: &'static str,
    pub(crate) capability: Capability,
    pub(crate) type_id: u16,
    pub(crate) format: &'static str,
}

macro_rules! field {
    ($name:literal, $encoding:ident) => {
        WireField {
            name: $name,
            encoding: FieldEncoding::$encoding,
            limit: 0,
            target: "",
        }
    };
    ($name:literal, $encoding:ident, $limit:expr) => {
        WireField {
            name: $name,
            encoding: FieldEncoding::$encoding,
            limit: $limit,
            target: "",
        }
    };
    ($name:literal, $encoding:ident => $target:literal) => {
        WireField {
            name: $name,
            encoding: FieldEncoding::$encoding,
            limit: 0,
            target: $target,
        }
    };
    ($name:literal, $encoding:ident, $limit:expr => $target:literal) => {
        WireField {
            name: $name,
            encoding: FieldEncoding::$encoding,
            limit: $limit,
            target: $target,
        }
    };
}

macro_rules! layouts {
    ($( $(#[$meta:meta])* $name:literal [$capability:ident] => [$($field:expr),* $(,)?];)+) => {
        pub(crate) const LAYOUTS: &[WireLayout] = &[
            $(
                $(#[$meta])*
                WireLayout {
                    name: $name,
                    capability: Capability::$capability,
                    fields: &[$($field),*],
                },
            )+
        ];
    };
}

layouts! {
    #[cfg(feature = "surfaces")]
    "request-surface" [Base] => [field!("session", U64), field!("request_id", U64), field!("tag", Variant => "request"), field!("edit", Bytes, 65536)];
    #[cfg(feature = "surfaces")]
    "response-surface" [Base] => [field!("session", U64), field!("request_id", U64), field!("tick", U64), field!("tag", Variant => "response")];
    #[cfg(feature = "gui")]
    "request-gui" [Base] => [field!("session", U64), field!("request_id", U64), field!("tag", Variant => "request"), field!("batch_id", Option => "u64"), field!("edits", Bytes, MESSAGE_BYTES)];
    #[cfg(feature = "gui")]
    "response-gui" [Base] => [field!("session", U64), field!("request_id", U64), field!("tick", U64), field!("tag", Variant => "response"), field!("applied", U32), field!("error", Option => "utf8-65536")];
    #[cfg(feature = "gui")]
    "request-gui-inspect" [Base] => [field!("session", U64), field!("request_id", U64), field!("tag", Variant => "request"), field!("query", Bytes, 65536)];
    #[cfg(feature = "gui")]
    "request-gui-input" [Base] => [field!("session", U64), field!("request_id", U64), field!("tag", Variant => "request"), field!("input", Bytes, MESSAGE_BYTES)];
    #[cfg(feature = "gui")]
    "gui-input-routing" [Base] => [field!("tick", U64), field!("reason", U16), field!("blocker", Option => "u64")];
    #[cfg(feature = "gui")]
    "response-gui-input" [Base] => [field!("session", U64), field!("request_id", U64), field!("tick", U64), field!("tag", Variant => "response"), field!("routing", Named => "gui-input-routing")];
    #[cfg(feature = "gui")]
    "response-gui-inspect" [Base] => [field!("session", U64), field!("request_id", U64), field!("tick", U64), field!("tag", Variant => "response"), field!("payload", Bytes, MESSAGE_BYTES)];
    #[cfg(feature = "gui")]
    "response-gui-observations" [Base] => [field!("session", U64), field!("request_id", U64), field!("tick", U64), field!("tag", Variant => "response"), field!("observations", Bytes, MESSAGE_BYTES)];
    #[cfg(feature = "gui")]
    "response-gui-unhandled" [Base] => [field!("session", U64), field!("request_id", U64), field!("tick", U64), field!("tag", Variant => "response"), field!("unhandled", Bytes, MESSAGE_BYTES)];
    #[cfg(feature = "gui")]
    "request-gui-semantic-snapshot" [Base] => [field!("session", U64), field!("request_id", U64), field!("tag", Variant => "request"), field!("query", Bytes, 65536)];
    #[cfg(feature = "gui")]
    "request-gui-semantic-action" [Base] => [field!("session", U64), field!("request_id", U64), field!("tag", Variant => "request"), field!("action", Bytes, MESSAGE_BYTES)];
    #[cfg(feature = "gui")]
    "response-gui-semantic-snapshot" [Base] => [field!("session", U64), field!("request_id", U64), field!("tick", U64), field!("tag", Variant => "response"), field!("snapshot", Bytes, MESSAGE_BYTES)];
    "host-request-list-worlds" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("after", U64)];
    "host-request-create-world" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("symbolic_id", Utf8, 65536), field!("hints", Named => "host-hints-patch"), field!("temporary", Bool)];
    "host-request-attach-world" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("world", Union => "world-selector")];
    "host-request-rename-world" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("world", Union => "world-selector"), field!("symbolic_id", Utf8, 65536)];
    "host-request-destroy-world" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("world", Union => "world-selector")];
    "host-request-detach-world" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request")];
    "host-request-set-capacity-hints" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("hints", Named => "host-hints-patch")];
    "host-request-save-world" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request")];
    "host-request-read-world-save" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("job", U64), field!("offset", U64)];
    "host-request-begin-world-load" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("bytes", U64), field!("symbolic_id", Option => "utf8-65536"), field!("hints", Named => "host-hints-patch")];
    "host-request-write-world-load" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("job", U64), field!("offset", U64), field!("bytes", Bytes, 65536)];
    "host-request-finish-world-load" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("job", U64)];
    "host-request-cancel-world-transfer" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("job", U64)];
    "host-response-worlds" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("worlds", List, 32 => "host-world"), field!("next", U64)];
    "host-response-attached" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("world", Named => "host-world"), field!("session", U64)];
    "host-response-world" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("world", Named => "host-world")];
    "host-response-complete" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response")];
    "host-response-error" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("message", Utf8, 65536)];
    "host-response-detached" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("session", U64), field!("reason", Utf8, 65536)];
    "host-response-transfer" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("job", U64)];
    "host-response-save-chunk" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("job", U64), field!("offset", U64), field!("total", U64), field!("bytes", Bytes, 65536)];
    "host-world" [Base] => [field!("id", U64), field!("symbolic_id", Utf8, 65536), field!("persistent_id_low", U64), field!("persistent_id_high", U64), field!("entities", U32), field!("systems", List, 1024 => "host-system-hints")];
    "host-hints-patch" [Base] => [field!("entities", Option => "u32"), field!("systems", List, 1024 => "host-system-hints")];
    "host-system-hints" [Base] => [field!("system", Utf8, 65536), field!("values", List, 1024 => "host-capacity-hint")];
    "host-capacity-hint" [Base] => [field!("name", Utf8, 65536), field!("value", U32)];
    "world-selector-id" [Base] => [field!("tag", Variant => "world-selector"), field!("value", U64)];
    "world-selector-symbol" [Base] => [field!("tag", Variant => "world-selector"), field!("value", Utf8, 65536)];

    "value-dynamic" [Base] => [field!("tag", Variant => "value"), field!("value", Bytes, 65536)];
    "snapshot-value-dynamic" [Base] => [field!("tag", Variant => "snapshot-value"), field!("value", Bytes, 65536)];
    "command-set-dynamic-property" [Base] => [field!("tag", Variant => "command"), field!("entity", Union => "reference"), field!("component", U16), field!("name", Utf8, 65536), field!("value", Bytes, 65536)];
    "command-remove-dynamic-property" [Base] => [field!("tag", Variant => "command"), field!("entity", Union => "reference"), field!("component", U16), field!("name", Utf8, 65536)];
    "dynamic-property-name" [Base] => [field!("name", Utf8, 65536)];
    "dynamic-property-write" [Base] => [field!("name", Utf8, 65536), field!("value", Bytes, 65536)];
    "command-update-dynamic-component-state-overlay" [StateOverlays] => [field!("tag", Variant => "command"), field!("owner", Union => "reference"), field!("overlay", Union => "reference"), field!("properties", List, 65536 => "dynamic-property-write"), field!("clear", List, 65536 => "dynamic-property-name")];
    "request-lifecycle-unsubscribe" [Base] => [
        field!("session", U64), field!("request_id", U64), field!("tag", Variant => "request"), field!("subscription", U64),
    ];
    "response-lifecycle-subscription" [Base] => [
        field!("session", U64), field!("request_id", U64), field!("tick", U64), field!("tag", Variant => "response"),
    ];
    "response-lifecycle-events" [Base] => [
        field!("session", U64), field!("request_id", U64), field!("tick", U64), field!("tag", Variant => "response"),
        field!("events", List, 128 => "lifecycle-publication"),
    ];
    "response-lifecycle-overflow" [Base] => [
        field!("session", U64), field!("request_id", U64), field!("tick", U64), field!("tag", Variant => "response"), field!("dropped", U64),
    ];
    "lifecycle-publication" [Base] => [
        field!("subscription", U64), field!("sequence", U64), field!("tick", U64), field!("observation", Union => "lifecycle-observation"),
    ];
    "lifecycle-entity" [Base] => [field!("tag", Variant => "lifecycle-observation"), field!("entity", U64)];
    "lifecycle-component" [Base] => [
        field!("tag", Variant => "lifecycle-observation"), field!("entity", U64), field!("component", U16),
        field!("previous_incarnation", U64), field!("incarnation", U64),
    ];
    "lifecycle-asset" [Assets] => [field!("tag", Variant => "lifecycle-observation"), field!("resource", Named => "resource")];
    "request-lifecycle-subscribe" [Base] => [
        field!("session", U64), field!("request_id", U64), field!("tag", Variant => "request"),
        field!("subscription", U64), field!("domains", U16), field!("entity", U64), field!("component", U16), field!("asset", U64),
    ];
    "value-bool" [Base] => [
        field!("tag", Variant => "value"),
        field!("value", Bool),
    ];
    "snapshot-value-bool" [Base] => [
        field!("tag", Variant => "snapshot-value"),
        field!("value", Bool),
    ];
    "render-state-patch" [Spatial] => [
        field!("mask", U16),
        field!("showAllDebugGeometries", Masked, 1 => "bool"),
        field!("debugGeometryColor", Masked, 2 => "linear-rgb"),
        field!("ambientLight", Masked, 4 => "linear-rgb"),
    ];
    "linear-rgb" [Spatial] => [
        field!("r", FiniteF32),
        field!("g", FiniteF32),
        field!("b", FiniteF32),
    ];
    "request-render-state-update" [Spatial] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("changes", Named => "render-state-patch"),
    ];
    "response-render-state-updated" [Spatial] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("changes", Named => "render-state-patch"),
    ];
    "request-camera-activate" [Spatial] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("entity", U64),
    ];
    "request-camera-navigate" [Spatial] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("motion", Union => "camera-motion"),
    ];
    "camera-motion-rotate" [Spatial] => [
        field!("tag", Variant => "camera-motion"),
        field!("yaw", FiniteF32),
        field!("pitch", FiniteF32),
    ];
    "camera-motion-pan" [Spatial] => [
        field!("tag", Variant => "camera-motion"),
        field!("x", FiniteF32),
        field!("y", FiniteF32),
        field!("width", U32),
        field!("height", U32),
    ];
    "camera-motion-zoom" [Spatial] => [
        field!("tag", Variant => "camera-motion"),
        field!("amount", FiniteF32),
    ];
    "request-geometry-pick" [Picking] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("x", FiniteF32),
        field!("y", FiniteF32),
        field!("width", U32),
        field!("height", U32),
        field!("include_view_plane", Bool),
    ];
    "request-camera-project" [Picking] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("x", FiniteF32),
        field!("y", FiniteF32),
        field!("width", U32),
        field!("height", U32),
        field!("plane", Named => "pick-view-plane"),
    ];
    "response-camera-project" [Picking] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("camera", Option => "u64"),
        field!("ok", Bool),
        field!("position", Option => "world-point"),
        field!("error", Option => "utf8-65536"),
    ];
    "world-point" [Picking] => [
        field!("x", FiniteF32),
        field!("y", FiniteF32),
        field!("z", FiniteF32),
    ];
    "response-camera-state-changed" [Spatial] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("changes", Named => "camera-state-patch"),
    ];
    "camera-state-patch" [Spatial] => [
        field!("mask", U16),
        field!("activeCamera", Masked, 1 => "camera-entity"),
    ];
    "camera-entity" [Spatial] => [field!("id", U64)];
    "response-geometry-pick" [Picking] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("camera", Option => "u64"),
        field!("result", Union => "geometry-pick-outcome"),
    ];
    "pick-result-miss" [Picking] => [field!("tag", Variant => "geometry-pick-outcome")];
    "pick-result-hit" [Picking] => [
        field!("tag", Variant => "geometry-pick-outcome"),
        field!("entity", U64),
        field!("position_x", FiniteF32),
        field!("position_y", FiniteF32),
        field!("position_z", FiniteF32),
        field!("distance", FiniteF32),
        field!("part", U32),
        field!("view_plane", Option => "pick-view-plane"),
    ];
    "pick-view-plane" [Picking] => [
        field!("point_x", FiniteF32),
        field!("point_y", FiniteF32),
        field!("point_z", FiniteF32),
        field!("normal_x", FiniteF32),
        field!("normal_y", FiniteF32),
        field!("normal_z", FiniteF32),
    ];
    "pick-result-failure" [Picking] => [
        field!("tag", Variant => "geometry-pick-outcome"),
        field!("reason", Utf8, 65536),
    ];

    "metadata" [Base] => [
        field!("symbolic_id", Option => "utf8-65536"),
        field!("classes", List, 256 => "utf8-65536"),
    ];
    "field" [Base] => [
        field!("offset", U32),
        field!("value", Union => "value"),
    ];
    "component" [Base] => [
        field!("type_id", U16),
        field!("fields", List, 65536 => "snapshot-field"),
    ];
    "snapshot-field" [Base] => [
        field!("offset", U32),
        field!("value", Union => "snapshot-value"),
    ];
    "entity" [Base] => [
        field!("id", U64),
        field!("metadata", Named => "metadata"),
        field!("base", List, 256 => "component"),
        field!("effective", List, 256 => "component"),
    ];
    "alias-handle" [Base] => [
        field!("alias", U32),
        field!("handle", U64),
    ];
    "state-overlay-alias" [StateOverlays] => [
        field!("alias", U32),
        field!("id", U64),
        field!("kind", Variant => "state-overlay-handle-kind"),
        field!("entity", Option => "u64"),
    ];
    "resource" [Assets] => [
        field!("id", U64),
        field!("kind", U16),
        field!("source", Utf8, 65536),
        field!("variant", U32),
        field!("status", Union => "resource-status"),
        field!("representation", Named => "asset-representation"),
    ];
    "asset-representation" [Assets] => [
        field!("decoded", Bool), field!("graphics_ready", Option => "bool"),
        field!("source_bytes", U64), field!("resident_bytes", U64), field!("graphics_bytes", Option => "u64"),
    ];
    "render-diagnostic" [Spatial] => [
        field!("entity", U64),
        field!("reason", Utf8, 65536),
    ];
    "state-overlay-lifecycle-diagnostic" [StateOverlays] => [
        field!("owner", U64),
        field!("stateOverlay", U64),
        field!("entity", U64),
        field!("component", Option => "u16"),
        field!("reason", Variant => "state-overlay-lifecycle-reason"),
    ];

    "value-f32" [Base] => [
        field!("tag", Variant => "value"),
        field!("value", FiniteF32),
    ];
    "value-entity" [Base] => [
        field!("tag", Variant => "value"),
        field!("value", Union => "reference"),
    ];
    "value-u32" [Base] => [
        field!("tag", Variant => "value"),
        field!("value", U32),
    ];
    "value-u64" [Base] => [
        field!("tag", Variant => "value"),
        field!("value", U64),
    ];
    "value-string" [Base] => [
        field!("tag", Variant => "value"),
        field!("value", Utf8, 65536),
    ];
    "value-bytes" [Base] => [
        field!("tag", Variant => "value"),
        field!("value", Bytes, 65536),
    ];
    "snapshot-value-f32" [Base] => [
        field!("tag", Variant => "snapshot-value"),
        field!("value", FiniteF32),
    ];
    "snapshot-value-entity" [Base] => [
        field!("tag", Variant => "snapshot-value"),
        field!("reference_tag", Variant => "snapshot-reference"),
        field!("value", U64),
    ];
    "snapshot-value-u32" [Base] => [
        field!("tag", Variant => "snapshot-value"),
        field!("value", U32),
    ];
    "snapshot-value-u64" [Base] => [
        field!("tag", Variant => "snapshot-value"),
        field!("value", U64),
    ];
    "snapshot-value-string" [Base] => [
        field!("tag", Variant => "snapshot-value"),
        field!("value", Utf8, 65536),
    ];
    "snapshot-value-bytes" [Base] => [
        field!("tag", Variant => "snapshot-value"),
        field!("value", Bytes, MESSAGE_BYTES),
    ];
    // An effective component repeats its base descriptor table by reference.
    "snapshot-value-base-descriptors" [Base] => [
        field!("tag", Variant => "snapshot-value"),
    ];

    "request-begin-batch" [Base] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
    ];
    "request-end-batch" [Base] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("batch_id", U64),
    ];
    "request-batch" [Base] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("batch_id", U64),
        field!("operations", List, 256 => "command"),
    ];
    "request-inspect" [Base] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("collection", Variant => "inspection-collection"),
        field!("after", U64),
        field!("target", U64),
        field!("limit", U16),
    ];

    "command-create" [Base] => [
        field!("tag", Variant => "command"),
        field!("alias", U32),
        field!("metadata", Named => "metadata"),
    ];
    "command-delete" [Base] => [
        field!("tag", Variant => "command"),
        field!("entity", Union => "reference"),
    ];
    "command-metadata" [Base] => [
        field!("tag", Variant => "command"),
        field!("entity", Union => "reference"),
        field!("metadata", Named => "metadata"),
    ];
    "command-insert" [Base] => [
        field!("tag", Variant => "command"),
        field!("entity", Union => "reference"),
        field!("component", U16),
        field!("fields", List, 256 => "field"),
    ];
    "command-set" [Base] => [
        field!("tag", Variant => "command"),
        field!("entity", Union => "reference"),
        field!("component", U16),
        field!("field", Named => "field"),
    ];
    "command-remove" [Base] => [
        field!("tag", Variant => "command"),
        field!("entity", Union => "reference"),
        field!("component", U16),
    ];
    "command-create-state-overlay-owner" [StateOverlays] => [
        field!("tag", Variant => "command"),
        field!("alias", U32),
    ];
    "command-release-state-overlay-owner" [StateOverlays] => [
        field!("tag", Variant => "command"),
        field!("owner", Union => "reference"),
    ];
    "command-attach-entity-overlay-binding" [StateOverlays] => [
        field!("tag", Variant => "command"),
        field!("owner", Union => "reference"),
        field!("alias", U32),
        field!("symbolic_id", Utf8, 65536),
        field!("mode", Variant => "entity-overlay-mode"),
    ];
    "command-release-entity-overlay-binding" [StateOverlays] => [
        field!("tag", Variant => "command"),
        field!("owner", Union => "reference"),
        field!("binding", Union => "reference"),
    ];
    "command-attach-component-state-overlay" [StateOverlays] => [
        field!("tag", Variant => "command"),
        field!("owner", Union => "reference"),
        field!("binding", Union => "reference"),
        field!("alias", U32),
        field!("component", U16),
        field!("mode", Variant => "component-overlay-mode"),
        field!("fields", List, 256 => "field"),
    ];
    "command-update-component-state-overlay" [StateOverlays] => [
        field!("tag", Variant => "command"),
        field!("owner", Union => "reference"),
        field!("overlay", Union => "reference"),
        field!("fields", List, 256 => "field"),
        field!("clear", List, 256 => "u32"),
    ];
    "command-release-component-state-overlay" [StateOverlays] => [
        field!("tag", Variant => "command"),
        field!("owner", Union => "reference"),
        field!("overlay", Union => "reference"),
    ];

    "reference-handle" [Base] => [
        field!("tag", Variant => "reference"),
        field!("handle", U64),
    ];
    "reference-alias" [Base] => [
        field!("tag", Variant => "reference"),
        field!("alias", U32),
    ];

    "response-batch-identity" [Base] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("batch_id", U64),
    ];
    "response-batch-aborted" [Base] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("batch_id", U64),
        field!("message", Utf8, 2048),
    ];
    "response-batch" [Base] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("outcome", Union => "outcome"),
    ];
    "response-runtime-failure" [Base] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("scope", Variant => "runtime-failure-scope"),
        field!("faulted", Bool),
        field!("message", Utf8, 2048),
    ];
    "response-frame" [Base] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("time", NonnegativeFiniteF64),
    ];
    "response-inspect" [Base] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("time", NonnegativeFiniteF64),
        field!("next", U64),
        field!("entities", List, 256 => "entity"),
        field!("resources", List, 256 => "resource"),
        field!("render_diagnostics", List, 256 => "render-diagnostic"),
        field!("controllers", List, 256 => "animation-controller"),
    ];
    "animation-controller" [Animation] => [
        field!("state", Named => "controller-state"),
        field!("description", Named => "controller-description"),
        field!("transition", Option => "controller-transition-state"),
    ];
    "controller-transition-state" [Animation] => [
        field!("duration", NonnegativeFiniteF64),
        field!("elapsed", NonnegativeFiniteF64),
        field!("easing", U32),
        field!("pending", Bool),
    ];
    "controller-description" [Animation] => [
        field!("speed", FiniteF32),
        field!("looping", Bool),
        field!("drivers", List, u32::MAX => "animation-driver"),
    ];
    "animation-driver" [Animation] => [
        field!("source", Utf8, 65536),
        field!("variant", U32),
        field!("track", U32),
        field!("target", U64),
        field!("property", Named => "animation-property"),
        field!("weight", FiniteF32),
        field!("additive", Bool),
        field!("reference_time", FiniteF32),
        field!("repeat", Bool),
    ];
    "animation-property" [Animation] => [
        field!("kind", U32),
        field!("component", U16),
        field!("indices", List, 4096 => "animation-index"),
        field!("name", Utf8, 65536),
    ];
    "animation-index" [Animation] => [
        field!("value", U32),
    ];
    "request-controller-create" [Animation] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("description", Named => "controller-description"),
    ];
    "request-controller-update" [Animation] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("id", U64),
        field!("description", Named => "controller-description"),
    ];
    "request-controller-delete" [Animation] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("id", U64),
    ];
    "request-controller-control" [Animation] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("id", U64),
        field!("control", U32),
        field!("time", NonnegativeFiniteF64),
        field!("speed", FiniteF32),
    ];
    "request-controller-transition" [Animation] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("id", U64),
        field!("description", Named => "controller-description"),
        field!("duration", NonnegativeFiniteF64),
        field!("easing", U32),
        field!("start_time", U32),
        field!("seek_time", NonnegativeFiniteF64),
    ];
    "response-controller" [Animation] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("id", U64),
    ];
    "controller-state" [Animation] => [
        field!("id", U64),
        field!("state", U32),
        field!("time", NonnegativeFiniteF64),
    ];
    "playback-event" [Animation] => [
        field!("controller", Named => "controller-state"),
        field!("kind", U32),
        field!("reason", Utf8, 65536),
    ];
    "request-playback" [Animation] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("controller", U64),
        field!("control", U32),
        field!("time", NonnegativeFiniteF64),
        field!("speed", FiniteF32),
    ];
    "response-playback" [Animation] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("events", List, 1024 => "playback-event"),
    ];
    "response-state-overlay-lifecycle" [StateOverlays] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("diagnostics", List, 16384 => "state-overlay-lifecycle-diagnostic"),
    ];
    "response-resources" [Assets] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("resources", List, 128 => "resource"),
    ];
    "response-error" [Base] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("code", U16),
        field!("message", Utf8, 65536),
    ];

    "outcome-success" [StateOverlays] => [
        field!("batch_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "outcome"),
        field!("aliases", List, 4096 => "alias-handle"),
        field!("stateOverlays", List, 4096 => "state-overlay-alias"),
    ];
    "outcome-failure" [StateOverlays] => [
        field!("batch_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "outcome"),
        field!("scope", Variant => "batch-error-scope"),
        field!("operation", Option => "u32"),
        field!("reason", Utf8, 65536),
        field!("aliases", List, 4096 => "alias-handle"),
        field!("stateOverlays", List, 4096 => "state-overlay-alias"),
    ];

    "resource-status-unloaded" [Assets] => [field!("tag", Variant => "resource-status")];
    "resource-status-start" [Assets] => [field!("tag", Variant => "resource-status")];
    "resource-status-progress" [Assets] => [
        field!("tag", Variant => "resource-status"),
        field!("completed", U64),
        field!("total", Option => "u64"),
    ];
    "resource-status-loaded" [Assets] => [field!("tag", Variant => "resource-status")];
    "resource-status-failed" [Assets] => [
        field!("tag", Variant => "resource-status"),
        field!("error", Utf8, ipp_core::services::asset_management::MAX_ASSET_ERROR_BYTES as u32),
    ];
}

macro_rules! tags {
    ($( $(#[$meta:meta])* $space:ident [$capability:ident] $name:ident = $value:expr => $layout:literal;)+) => {
        $(
            $(#[$meta])*
            #[allow(dead_code)]
            pub(crate) const $name: u8 = $value;
        )+

        pub(crate) const TAGS: &[WireTag] = &[
            $(
                $(#[$meta])*
                WireTag {
                    name: stringify!($name),
                    space: TagSpace::$space,
                    capability: Capability::$capability,
                    value: $value,
                    layout: $layout,
                },
            )+
        ];
    };
}

tags! {
    #[cfg(feature = "surfaces")]
    Request [Base] REQUEST_SURFACE = 26 => "request-surface";
    #[cfg(feature = "surfaces")]
    Response [Base] RESPONSE_SURFACE = 26 => "response-surface";
    #[cfg(feature = "gui")]
    Request [Base] REQUEST_GUI = 28 => "request-gui";
    #[cfg(feature = "gui")]
    Response [Base] RESPONSE_GUI = 28 => "response-gui";
    #[cfg(feature = "gui")]
    Request [Base] REQUEST_GUI_INSPECT = 29 => "request-gui-inspect";
    #[cfg(feature = "gui")]
    Response [Base] RESPONSE_GUI_INSPECT = 29 => "response-gui-inspect";
    #[cfg(feature = "gui")]
    Request [Base] REQUEST_GUI_INPUT = 30 => "request-gui-input";
    #[cfg(feature = "gui")]
    Response [Base] RESPONSE_GUI_INPUT = 30 => "response-gui-input";
    #[cfg(feature = "gui")]
    Response [Base] RESPONSE_GUI_OBSERVATIONS = 31 => "response-gui-observations";
    #[cfg(feature = "gui")]
    Response [Base] RESPONSE_GUI_UNHANDLED = 32 => "response-gui-unhandled";
    #[cfg(feature = "gui")]
    Request [Base] REQUEST_GUI_SEMANTIC_SNAPSHOT = 31 => "request-gui-semantic-snapshot";
    #[cfg(feature = "gui")]
    Request [Base] REQUEST_GUI_SEMANTIC_ACTION = 32 => "request-gui-semantic-action";
    #[cfg(feature = "gui")]
    Response [Base] RESPONSE_GUI_SEMANTIC_SNAPSHOT = 33 => "response-gui-semantic-snapshot";
    HostRequest [Base] HOST_REQUEST_LIST_WORLDS = 1 => "host-request-list-worlds";
    HostRequest [Base] HOST_REQUEST_CREATE_WORLD = 2 => "host-request-create-world";
    HostRequest [Base] HOST_REQUEST_ATTACH_WORLD = 3 => "host-request-attach-world";
    HostRequest [Base] HOST_REQUEST_RENAME_WORLD = 4 => "host-request-rename-world";
    HostRequest [Base] HOST_REQUEST_DESTROY_WORLD = 5 => "host-request-destroy-world";
    HostRequest [Base] HOST_REQUEST_DETACH_WORLD = 6 => "host-request-detach-world";
    HostRequest [Base] HOST_REQUEST_SET_CAPACITY_HINTS = 7 => "host-request-set-capacity-hints";
    HostRequest [Base] HOST_REQUEST_SAVE_WORLD = 8 => "host-request-save-world";
    HostRequest [Base] HOST_REQUEST_READ_WORLD_SAVE = 9 => "host-request-read-world-save";
    HostRequest [Base] HOST_REQUEST_BEGIN_WORLD_LOAD = 10 => "host-request-begin-world-load";
    HostRequest [Base] HOST_REQUEST_WRITE_WORLD_LOAD = 11 => "host-request-write-world-load";
    HostRequest [Base] HOST_REQUEST_FINISH_WORLD_LOAD = 12 => "host-request-finish-world-load";
    HostRequest [Base] HOST_REQUEST_CANCEL_WORLD_TRANSFER = 13 => "host-request-cancel-world-transfer";
    HostResponse [Base] HOST_RESPONSE_WORLDS = 1 => "host-response-worlds";
    HostResponse [Base] HOST_RESPONSE_ATTACHED = 2 => "host-response-attached";
    HostResponse [Base] HOST_RESPONSE_WORLD = 3 => "host-response-world";
    HostResponse [Base] HOST_RESPONSE_COMPLETE = 4 => "host-response-complete";
    HostResponse [Base] HOST_RESPONSE_ERROR = 5 => "host-response-error";
    HostResponse [Base] HOST_RESPONSE_DETACHED = 6 => "host-response-detached";
    HostResponse [Base] HOST_RESPONSE_TRANSFER = 7 => "host-response-transfer";
    HostResponse [Base] HOST_RESPONSE_SAVE_CHUNK = 9 => "host-response-save-chunk";
    WorldSelector [Base] WORLD_SELECTOR_ID = 0 => "world-selector-id";
    WorldSelector [Base] WORLD_SELECTOR_SYMBOL = 1 => "world-selector-symbol";
    AnimationTarget [Animation] ANIMATION_TARGET_PROPERTY = 0 => "empty";
    AnimationTarget [Animation] ANIMATION_TARGET_JOINTS = 1 => "empty";
    AnimationTarget [Animation] ANIMATION_TARGET_DYNAMIC = 2 => "empty";
    PlaybackControl [Animation] PLAYBACK_CONTROL_PLAY = 0 => "empty";
    PlaybackControl [Animation] PLAYBACK_CONTROL_PAUSE = 1 => "empty";
    PlaybackControl [Animation] PLAYBACK_CONTROL_STOP = 2 => "empty";
    PlaybackControl [Animation] PLAYBACK_CONTROL_SEEK = 3 => "empty";
    PlaybackControl [Animation] PLAYBACK_CONTROL_RESTART = 4 => "empty";
    PlaybackState [Animation] PLAYBACK_STATE_STOPPED = ipp_core::systems::animation::AnimationPlaybackStatus::Stopped as u8 => "empty";
    PlaybackState [Animation] PLAYBACK_STATE_PLAYING = ipp_core::systems::animation::AnimationPlaybackStatus::Playing as u8 => "empty";
    PlaybackState [Animation] PLAYBACK_STATE_PAUSED = ipp_core::systems::animation::AnimationPlaybackStatus::Paused as u8 => "empty";
    PlaybackState [Animation] PLAYBACK_STATE_COMPLETED = ipp_core::systems::animation::AnimationPlaybackStatus::Completed as u8 => "empty";
    PlaybackEvent [Animation] PLAYBACK_EVENT_STARTED = ipp_core::systems::animation::AnimationPlaybackEventKind::Started as u8 => "empty";
    PlaybackEvent [Animation] PLAYBACK_EVENT_PAUSED = ipp_core::systems::animation::AnimationPlaybackEventKind::Paused as u8 => "empty";
    PlaybackEvent [Animation] PLAYBACK_EVENT_STOPPED = ipp_core::systems::animation::AnimationPlaybackEventKind::Stopped as u8 => "empty";
    PlaybackEvent [Animation] PLAYBACK_EVENT_COMPLETED = ipp_core::systems::animation::AnimationPlaybackEventKind::Completed as u8 => "empty";
    PlaybackEvent [Animation] PLAYBACK_EVENT_INVALIDATED = ipp_core::systems::animation::AnimationPlaybackEventKind::Invalidated as u8 => "empty";
    PlaybackEvent [Animation] PLAYBACK_EVENT_FAILED = ipp_core::systems::animation::AnimationPlaybackEventKind::Failed as u8 => "empty";

    InspectionCollection [Base] INSPECT_SUMMARY = 0 => "empty";
    InspectionCollection [Base] INSPECT_ENTITIES = 1 => "empty";
    InspectionCollection [Base] INSPECT_RESOURCES = 2 => "empty";
    InspectionCollection [Base] INSPECT_CONTROLLERS = 3 => "empty";
    InspectionCollection [Base] INSPECT_RENDER_DIAGNOSTICS = 4 => "empty";
    Request [Base] REQUEST_LIFECYCLE_SUBSCRIBE = 19 => "request-lifecycle-subscribe";
    Request [Base] REQUEST_LIFECYCLE_UNSUBSCRIBE = 20 => "request-lifecycle-unsubscribe";
    Response [Base] RESPONSE_LIFECYCLE_SUBSCRIPTION = 16 => "response-lifecycle-subscription";
    Response [Base] RESPONSE_LIFECYCLE_EVENTS = 17 => "response-lifecycle-events";
    Response [Base] RESPONSE_LIFECYCLE_OVERFLOW = 18 => "response-lifecycle-overflow";
    LifecycleObservation [Base] LIFECYCLE_ENTITY_CREATED = 1 => "lifecycle-entity";
    LifecycleObservation [Base] LIFECYCLE_ENTITY_METADATA_CHANGED = 2 => "lifecycle-entity";
    LifecycleObservation [Base] LIFECYCLE_ENTITY_DELETED = 3 => "lifecycle-entity";
    LifecycleObservation [Base] LIFECYCLE_COMPONENT_INSERTED = 4 => "lifecycle-component";
    LifecycleObservation [Base] LIFECYCLE_COMPONENT_UPDATED = 5 => "lifecycle-component";
    LifecycleObservation [Base] LIFECYCLE_COMPONENT_REPLACED = 6 => "lifecycle-component";
    LifecycleObservation [Base] LIFECYCLE_COMPONENT_REMOVED = 7 => "lifecycle-component";
    LifecycleObservation [Assets] LIFECYCLE_ASSET_STATUS_CHANGED = 8 => "lifecycle-asset";
    LifecycleObservation [Assets] LIFECYCLE_ASSET_REMOVED = 9 => "lifecycle-asset";
    LifecycleObservation [Assets] LIFECYCLE_ASSET_GRAPHICS_INVALIDATED = 10 => "lifecycle-asset";
    Value [Base] VALUE_BOOL = ipp_core::components::schema::FieldKind::Bool as u8 => "value-bool";
    SnapshotValue [Base] SNAPSHOT_VALUE_BOOL = ipp_core::components::schema::FieldKind::Bool as u8 => "snapshot-value-bool";
    Request [Spatial] REQUEST_RENDER_STATE_UPDATE = 10 => "request-render-state-update";
    Response [Spatial] RESPONSE_RENDER_STATE_UPDATED = 12 => "response-render-state-updated";
    Value [Base] VALUE_F32 = ipp_core::components::schema::FieldKind::F32 as u8 => "value-f32";
    Value [Base] VALUE_ENTITY = ipp_core::components::schema::FieldKind::Entity as u8 => "value-entity";
    Value [Base] VALUE_U32 = ipp_core::components::schema::FieldKind::U32 as u8 => "value-u32";
    Value [Base] VALUE_U64 = ipp_core::components::schema::FieldKind::U64 as u8 => "value-u64";
    Value [Base] VALUE_STRING = ipp_core::components::schema::FieldKind::String as u8 => "value-string";
    Value [Base] VALUE_DYNAMIC = ipp_core::components::schema::FieldKind::Dynamic as u8 => "value-dynamic";
    SnapshotValue [Base] SNAPSHOT_VALUE_DYNAMIC = ipp_core::components::schema::FieldKind::Dynamic as u8 => "snapshot-value-dynamic";
    Value [Base] VALUE_BYTES = ipp_core::components::schema::FieldKind::Bytes as u8 => "value-bytes";
    SnapshotValue [Base] SNAPSHOT_VALUE_F32 = ipp_core::components::schema::FieldKind::F32 as u8 => "snapshot-value-f32";
    SnapshotValue [Base] SNAPSHOT_VALUE_ENTITY = ipp_core::components::schema::FieldKind::Entity as u8 => "snapshot-value-entity";
    SnapshotValue [Base] SNAPSHOT_VALUE_U32 = ipp_core::components::schema::FieldKind::U32 as u8 => "snapshot-value-u32";
    SnapshotValue [Base] SNAPSHOT_VALUE_U64 = ipp_core::components::schema::FieldKind::U64 as u8 => "snapshot-value-u64";
    SnapshotValue [Base] SNAPSHOT_VALUE_STRING = ipp_core::components::schema::FieldKind::String as u8 => "snapshot-value-string";
    SnapshotValue [Base] SNAPSHOT_VALUE_BYTES = ipp_core::components::schema::FieldKind::Bytes as u8 => "snapshot-value-bytes";
    SnapshotValue [Base] SNAPSHOT_VALUE_BASE_DESCRIPTORS = 12 => "snapshot-value-base-descriptors";
    SnapshotReference [Base] SNAPSHOT_REF_HANDLE = REF_HANDLE => "empty";

    Request [Animation] REQUEST_CONTROLLER_CREATE = 15 => "request-controller-create";
    Request [Animation] REQUEST_CONTROLLER_UPDATE = 16 => "request-controller-update";
    Request [Animation] REQUEST_CONTROLLER_DELETE = 17 => "request-controller-delete";
    Request [Animation] REQUEST_CONTROLLER_CONTROL = 18 => "request-controller-control";
    Request [Animation] REQUEST_CONTROLLER_TRANSITION = 27 => "request-controller-transition";
    Response [Animation] RESPONSE_CONTROLLER = 15 => "response-controller";
    Request [Animation] REQUEST_PLAYBACK = 13 => "request-playback";
    Response [Animation] RESPONSE_PLAYBACK = 14 => "response-playback";
    AnimationTransitionEasing [Animation] ANIMATION_TRANSITION_LINEAR = 0 => "empty";
    AnimationTransitionEasing [Animation] ANIMATION_TRANSITION_SMOOTHSTEP = 1 => "empty";
    AnimationTransitionStartTime [Animation] ANIMATION_TRANSITION_RESTART = 0 => "empty";
    AnimationTransitionStartTime [Animation] ANIMATION_TRANSITION_PRESERVE = 1 => "empty";
    AnimationTransitionStartTime [Animation] ANIMATION_TRANSITION_MATCH_PHASE = 2 => "empty";
    AnimationTransitionStartTime [Animation] ANIMATION_TRANSITION_SEEK = 3 => "empty";
    PlaybackControl [Animation] PLAYBACK_CONTROL_PLAY_AT_SPEED = 5 => "empty";

    Request [Base] REQUEST_END_BATCH = 25 => "request-end-batch";
    Response [Base] RESPONSE_BATCH_FINISHED = 25 => "response-batch-identity";
    Request [Base] REQUEST_BEGIN_BATCH = 23 => "request-begin-batch";
    Request [Base] REQUEST_BATCH_CHUNK = 24 => "request-batch";
    Response [Base] RESPONSE_BATCH_STARTED = 23 => "response-batch-identity";
    Response [Base] RESPONSE_BATCH_ABORTED = 24 => "response-batch-aborted";
    Request [Base] REQUEST_BATCH = 1 => "request-batch";
    Request [Base] REQUEST_INSPECT = 3 => "request-inspect";
    Request [Spatial] REQUEST_CAMERA_ACTIVATE = 8 => "request-camera-activate";
    Request [Picking] REQUEST_GEOMETRY_PICK = 9 => "request-geometry-pick";
    Request [Picking] REQUEST_CAMERA_PROJECT = 12 => "request-camera-project";

    Command [Base] COMMAND_CREATE = 1 => "command-create";
    Command [Base] COMMAND_DELETE = 2 => "command-delete";
    Command [Base] COMMAND_METADATA = 3 => "command-metadata";
    Command [Base] COMMAND_INSERT = 4 => "command-insert";
    Command [Base] COMMAND_SET = 5 => "command-set";
    Command [Base] COMMAND_SET_DYNAMIC_PROPERTY = 14 => "command-set-dynamic-property";
    Command [Base] COMMAND_REMOVE_DYNAMIC_PROPERTY = 15 => "command-remove-dynamic-property";
    Command [StateOverlays] COMMAND_UPDATE_DYNAMIC_COMPONENT_STATE_OVERLAY = 16 => "command-update-dynamic-component-state-overlay";
    Command [Base] COMMAND_REMOVE = 6 => "command-remove";
    Command [StateOverlays] COMMAND_CREATE_STATE_OVERLAY_OWNER = 7 => "command-create-state-overlay-owner";
    Command [StateOverlays] COMMAND_RELEASE_STATE_OVERLAY_OWNER = 8 => "command-release-state-overlay-owner";
    Command [StateOverlays] COMMAND_ATTACH_ENTITY_OVERLAY_BINDING = 9 => "command-attach-entity-overlay-binding";
    Command [StateOverlays] COMMAND_RELEASE_ENTITY_OVERLAY_BINDING = 10 => "command-release-entity-overlay-binding";
    Command [StateOverlays] COMMAND_ATTACH_COMPONENT_STATE_OVERLAY = 11 => "command-attach-component-state-overlay";
    Command [StateOverlays] COMMAND_UPDATE_COMPONENT_STATE_OVERLAY = 12 => "command-update-component-state-overlay";
    Command [StateOverlays] COMMAND_RELEASE_COMPONENT_STATE_OVERLAY = 13 => "command-release-component-state-overlay";

    Reference [Base] REF_HANDLE = 0 => "reference-handle";
    Reference [Base] REF_ALIAS = 1 => "reference-alias";

    Response [Base] RESPONSE_BATCH = 1 => "response-batch";
    Response [Base] RESPONSE_INSPECT = 3 => "response-inspect";
    Response [Base] RESPONSE_FRAME = 4 => "response-frame";
    Response [StateOverlays] RESPONSE_STATE_OVERLAY_LIFECYCLE = 5 => "response-state-overlay-lifecycle";
    Response [Assets] RESPONSE_RESOURCES = 9 => "response-resources";
    Response [Spatial] RESPONSE_CAMERA_STATE_CHANGED = 10 => "response-camera-state-changed";
    Response [Picking] RESPONSE_GEOMETRY_PICK = 11 => "response-geometry-pick";
    Response [Picking] RESPONSE_CAMERA_PROJECT = 13 => "response-camera-project";
    Response [Base] RESPONSE_RUNTIME_FAILURE = 22 => "response-runtime-failure";
    RuntimeFailureScope [Base] FAILURE_DRAW = crate::RuntimeFailureScope::Draw as u8 => "empty";
    RuntimeFailureScope [Base] FAILURE_RESOURCE = crate::RuntimeFailureScope::Resource as u8 => "empty";
    RuntimeFailureScope [Base] FAILURE_CONTEXT = crate::RuntimeFailureScope::Context as u8 => "empty";
    RuntimeFailureScope [Base] FAILURE_WORLD = crate::RuntimeFailureScope::World as u8 => "empty";
    Response [Base] RESPONSE_ERROR = 255 => "response-error";

    Request [Spatial] REQUEST_CAMERA_NAVIGATE = 11 => "request-camera-navigate";
    CameraMotion [Spatial] CAMERA_MOTION_ROTATE = 0 => "camera-motion-rotate";
    CameraMotion [Spatial] CAMERA_MOTION_PAN = 1 => "camera-motion-pan";
    CameraMotion [Spatial] CAMERA_MOTION_ZOOM = 2 => "camera-motion-zoom";
    GeometryPickOutcome [Picking] PICK_OUTCOME_MISS = 0 => "pick-result-miss";
    GeometryPickOutcome [Picking] PICK_OUTCOME_HIT = 1 => "pick-result-hit";
    GeometryPickOutcome [Picking] PICK_OUTCOME_FAILURE = 3 => "pick-result-failure";

    Outcome [StateOverlays] OUTCOME_SUCCESS = 0 => "outcome-success";
    Outcome [StateOverlays] OUTCOME_FAILURE = 1 => "outcome-failure";

    BatchErrorScope [Base] BATCH_ERROR_OPERATION = 0 => "empty";
    BatchErrorScope [Base] BATCH_ERROR_COMMIT = 1 => "empty";
    Option [Base] OPTION_NONE = 0 => "empty";
    Option [Base] OPTION_SOME = 1 => "present";
    EntityOverlayMode [StateOverlays] ENTITY_OVERLAY_MODE_OWNED = 0 => "empty";
    EntityOverlayMode [StateOverlays] ENTITY_OVERLAY_MODE_BOUND = 1 => "empty";
    ComponentOverlayMode [StateOverlays] COMPONENT_OVERLAY_MODE_AUTO = 0 => "empty";
    ComponentOverlayMode [StateOverlays] COMPONENT_OVERLAY_MODE_BOUND = 1 => "empty";
    ComponentOverlayMode [StateOverlays] COMPONENT_OVERLAY_MODE_OWNED = 2 => "empty";
    StateOverlayHandleKind [StateOverlays] STATE_OVERLAY_KIND_OWNER = 0 => "empty";
    StateOverlayHandleKind [StateOverlays] STATE_OVERLAY_KIND_ENTITY_BINDING = 1 => "empty";
    StateOverlayHandleKind [StateOverlays] STATE_OVERLAY_KIND_COMPONENT = 2 => "empty";
    StateOverlayLifecycleReason [StateOverlays] STATE_OVERLAY_ENTITY_DELETED = 0 => "empty";
    StateOverlayLifecycleReason [StateOverlays] STATE_OVERLAY_COMPONENT_REPLACED = 1 => "empty";
    StateOverlayLifecycleReason [StateOverlays] STATE_OVERLAY_COMPONENT_REMOVED = 2 => "empty";
    AssetResourceStatus [Assets] RESOURCE_UNLOADED = 0 => "resource-status-unloaded";
    AssetResourceStatus [Assets] RESOURCE_START = 1 => "resource-status-start";
    AssetResourceStatus [Assets] RESOURCE_PROGRESS = 2 => "resource-status-progress";
    AssetResourceStatus [Assets] RESOURCE_LOADED = 3 => "resource-status-loaded";
    AssetResourceStatus [Assets] RESOURCE_FAILED = 4 => "resource-status-failed";
}

pub(crate) const CAPABILITIES: &[(Capability, bool)] = &[
    (Capability::Animation, true),
    (Capability::Assets, true),
    (Capability::StateOverlays, true),
    (Capability::Spatial, true),
    (Capability::Textures, true),
    (Capability::BuiltinAssets, cfg!(feature = "builtin-assets")),
    (Capability::Picking, true),
    (Capability::DebugGeometry, true),
];

pub(crate) const CONVENTIONS: &[(&str, &str)] = &[
    #[cfg(feature = "surfaces")]
    (
        "surface-edit",
        "version-u8=1;action-u8:insert1|update2|remove3|move4;entity-u64,item-u32;insert=index-u32,content,style;update=mask-u8,content?,position?,scale?,color?,opacity?,font-size?,asset?;move=index-u32;content=tag-u8:label1-utf8|glyphs2-count-u32-(id-u32,xy-f32x2,color?-bool-f32x4)|drawing3|bitmap4-size-f32x2;style=position-f32x2,scale-f32x2,color-f32x4,opacity-f32,font-size-f32,asset?-bool-(type-u16,variant-u32,source-utf8);finite-values;positive-bitmap-and-font-size;nonnegative-scale;unit-color-and-opacity;max65536;ordered-correlated;stable-item-identities;no-reuse",
    ),
    #[cfg(feature = "surfaces")]
    (
        "surface-items",
        "version-u8=1;next-id-u32,count-u32,(id-u32,content)*;content=surface-edit-content;ids-unique-nonzero-below-next-id;monotonic-next-id;properties=item_{id}_{position|scale|color|opacity|font_size|asset};local-xy-metres;centered-clip-rectangle;painter-order",
    ),
    #[cfg(feature = "gui")]
    (
        "gui-input",
        "version-u8=1;action-u8:pointer-down1|pointer-up2|pointer-move3|pointer-cancel4|scroll5|key6|text7|focus8|blur9|set-text-selection10|update-composition11|commit-composition12|cancel-composition13;pointer-u32;panel?-bool-entity-u64;position-f32x2;button-u8:primary0|secondary1|auxiliary2;blockers-count-u32-max1024-(entity-u64,distance-f32);panel-distance?-bool-f32;delta-f32x2;key-u8:tab0|enter1|space2|escape3|backspace4|delete5|left6|right7|up8|down9|home10|end11;pressed-bool;text-utf8-max65536;focus=session-u64,entity-u64,root-incarnation-u64,node-u32,lifetime-u32;selection-u32x2;composition=text-utf8,caret-u32x2;finite-values;ordered-correlated;session-fenced;nonzero-request;overlay-nearest-without-panel",
    ),
    #[cfg(feature = "gui")]
    (
        "gui-observations",
        "version-u8=3;unsolicited-request-id-0;chunked-like-resources-max128-per-message;observations=effects-count-u32,effect*,conflicts-count-u32,conflict*,cancellations-count-u32,cancel*,text-focus-count-u32-max1,text-focus-update?;text-focus-update=tag-u8:cleared0|focused1,session-u64,context-generation-u64,focus-generation-u64,focused-(entity-u64,root-incarnation-u64,node-u32,lifetime-u32,revision-u32,text-utf8-max65536,selection-u32x2,composition?-bool-(text-utf8-max65536,caret-u32x2));unhandled=count-u32,input*;effect=kind-u8:button0|control1,session-u64,source-tick-u64,effect-tick-u64,entity-u64,root-incarnation-u64,node-u32,lifetime-u32,path-count-u32-max65536,path-u32*,revision-u32?,value-tag-u8:bool1|scalar2|text3,text-utf8-max65536;conflict-cancel=session-u64,source-tick-u64,effect-tick-u64,target?-bool-(entity-u64,root-incarnation-u64,node-u32,lifetime-u32),reason-u8;conflict-reason:revision-mismatch0-expected-u32-found-u32|admission-failed1-reason-utf8|touch-arbitration2-owner-u32;cancel-reason:target-removed0|target-hidden1|session-replaced2|gesture-cancelled3;unhandled=session-u64,tick-u64,gui-input,reason-u8:no-panel-hit0|blocked1-entity-u64|stale-target2|no-focus3|no-capture4|not-focusable5|not-owner6;gui-input=version-u8=1-action-u8-see-gui-input;text-utf8-max65536;broadcast-effects-and-conflicts,text-focus-and-unhandled-supplier-only",
    ),
    #[cfg(feature = "gui")]
    (
        "gui-semantics",
        "snapshot-version-u8=1;action-version-u8=2;correlated-nonzero-request;bounded-maxdepth-32-limit-256;snapshot-query=entity-u64,max-depth-u32,limit-u32;action=entity-u64,root-incarnation-u64,node-u32-nonzero,lifetime-u32,expected-revision-u32,kind-u8:press0|toggle1|set-scalar2-value-f32|set-text3-value-utf8-max65536|focus4;snapshot=entity-u64,incarnation-u64,tick-u64,count-u32-max256,node*,focus?-bool-(id-u32,lifetime-u32);node=id-u32-nonzero,parent-u32-0-none,lifetime-u32,role-u8:container0|text1|drawing2|image3|button4|checkbox5|slider6|textinput7,name?-bool-utf8-max65536,value-tag-u8:none0|bool1|scalar2|text3-utf8-max65536,revision-u32,bounds-f32x4,enabled-bool,visible-bool,available-bool,actions-u8-list:press0|toggle1|set-scalar2|set-text3|focus4;revision-gated-set-control-value;stale-root-lifetime-revision-unknown-unsupported-refuse-as-host-error;oversized-text-rejected-whole;transient-only-focus-visible",
    ),
    ("host-request-magic", HOST_REQUEST_MAGIC_HEX),
    (
        "asset-source-data-plane",
        "revision=1;request=IPAS,session-u64,request-u64,tag-u8;response=IPAR,session-u64,request-u64,result-u8:ok0|error1-string;begin0=source,length-u64;chunk1=transfer-u64,offset-u64,bytes-u32-length;finish2=transfer-u64;cancel3=transfer-u64;release4=source;source=kind-u32,uri-utf8,variant-u32;chunk-limit=65536;frame-limit=65664;begin-request-is-transfer-id;session-fenced;ordered-chunks;complete-only-registration;no-world-tick;independent-of-command-batches",
    ),
    ("host-response-magic", HOST_RESPONSE_MAGIC_HEX),
    #[cfg(feature = "skeletal-animation")]
    (
        "mesh-skin-streams",
        "IPPM-v3;semantic4-format4=u8x4;semantic5-format5=f32x4;paired;indices=0..31;weights=finite-0..1-positive-sum-normalized;rigid-omits-streams",
    ),
    #[cfg(feature = "skeletal-animation")]
    (
        "skeleton-overrides",
        "ascending-unique-joint-u32,local-trs-f32x10;empty=source-pose-or-rest;max32;quaternion-xyzw-normalized;positive-scale",
    ),
    ("endianness", "little"),
    ("bool", "u8;false=0;true=1;other-values-reject"),
    (
        "render-state-patch",
        "mask-u16;masked-field-limit-is-presence-bit;unknown-bits-reject;omitted-unchanged;empty-noop-no-event;linear-rgb=finite-0..1;stage0-ordered;atomic;session-default-reset",
    ),
    ("bootstrap", "magic4,version-u32,schema-hash-u64"),
    (
        "host-control",
        "revision=1;request=IPPH-1-0-0-0,connection-u64,request-u64,tag-u8;response=IPPA-1-0-0-0,connection-u64,request-u64,tag-u8;requests=1-list,2-create,3-attach,4-rename,5-destroy,6-detach,7-hints;responses=1-worlds,2-attached,3-world,4-complete,5-error,6-detached;descriptor=id-u64,symbol-string,persistent-u128,hints;selector=tag-u8:0-id-u64,1-symbol-string;hints=optional-entities-u32,system-string-to-string-u32-map;unattached-before-create-load-attach;ordered-with-world-ingress;world-list-page=32;session-fresh;temporary-explicit;world-clock-host-owned",
    ),
    (
        "world-persistence",
        "IPPW-version=2;underlying-components-and-controllers;unchanged-asset-references;no-asset-table;no-source-reads;target-contract-required;header=magic4,version-u32,contract-u64,length-u64,fnv1a64-body;host-requests=8-save,9-read,10-begin-load,11-write,12-finish,13-cancel;host-responses=7-transfer,9-chunk;job-offset-length=u64;chunk=bytes-u32-max65536;max-file=67108864;new-world-only;private-validation-before-publication",
    ),
    (
        "request-id",
        "nonzero-rpc-and-query;zero-command-and-unsolicited-event;commands-no-reply",
    ),
    (
        "camera-navigation",
        "stage0-ordered;fire-and-forget;base-camera-transform;rotate-local-yaw-pitch-radians-fixed-pivot;pan-normalized-right-down-viewport;zoom-positive-log-out;atomic;no-system-event",
    ),
    (
        "system-state-events",
        "zero-id;sparse-committed-values;no-success-or-failure;no-command-correlation;nonempty-mask",
    ),
    ("length-prefix", "u32"),
    ("utf8", "strict"),
    ("max-message-bytes", "1048576"),
    ("command-page-bytes", "131072"),
    ("unsupported", "hierarchy"),
    (
        "playback",
        "control-u32:play=0,pause=1,stop=2,seek=3,restart=4;time-f64:nonnegative;nonseek-time=0;state-u32:stopped=0,playing=1,paused=2,completed=3;event-u32:started=0,paused=1,stopped=2,completed=3,invalidated=4,failed=5;reason-empty=none;stage0-ordered;world-owned-controller;multi-entity-drivers;seek-no-advance;stop-withdraws;completion-holds;loops-no-completion",
    ),
    (
        "camera-activation",
        "stage0-ordered;fire-and-forget;sparse-committed-active-camera-change;noops-no-event;active-lifetime-protected",
    ),
    (
        "geometry-query",
        "normalized-top-left;nonzero-viewport;render-surface-match;final-camera-and-pose;camera-clipping;world-distance;entity-then-part-ties",
    ),
    (
        "dynamic-property-values",
        "f32:1,i32:2,u32:3,bool:4,vec2:5,vec3:6,vec4:7,mat2:8,mat3:9,mat4:10,asset:12;asset=type-u16,variant-u32,source-utf8;retired-tag11-rejected;asset-type-is-value;consumer-validates-requirements",
    ),
];

pub(crate) const ASSET_FORMATS: &[AssetFormat] = &[
    #[cfg(feature = "surfaces")]
    AssetFormat {
        name: "ASSET_FONT",
        capability: Capability::Base,
        type_id: 17,
        format: "IPPF;version=1;quadratic-contours;original-glyph-identities;cmap;metrics;pair-kerning;cpu-layout;renderer-owned-acceleration",
    },
    #[cfg(feature = "surfaces")]
    AssetFormat {
        name: "ASSET_DRAWING",
        capability: Capability::Base,
        type_id: 18,
        format: "IPPD;version=1;quadratic-contours;paint-order;solid-srgb-rgba;nonzero-or-evenodd;source-bounds;tolerance;renderer-owned-acceleration",
    },
    #[cfg(feature = "particles")]
    AssetFormat {
        name: "ASSET_PARTICLE_CACHE",
        capability: Capability::Spatial,
        type_id: ipp_core::systems::particles::PARTICLE_CACHE_TYPE.0,
        format: "IPPC;version-u32=1;space-u32=local0|world1;frames-u32;directory=time-f32,offset-u32,count-u32;sample=id-u64,birth-f32,death-f32,position-f32x3,velocity-f32x3,rotation-f32x4,size-f32;60-byte-samples;ordered-times-and-identities;exact-payload",
    },
    #[cfg(feature = "particles")]
    AssetFormat {
        name: "ASSET_PARTICLE_SURFACE",
        capability: Capability::Spatial,
        type_id: ipp_core::systems::particles::PARTICLE_SURFACE_TYPE.0,
        format: "IPPM;version=1|2|3;decoded-mesh-metadata;triangle-area-weighted-emission;cpu-only",
    },
    AssetFormat {
        name: "ASSET_SHADER",
        capability: Capability::Base,
        type_id: ipp_core::services::asset_management::shader::SHADER_TYPE.0,
        format: "IPPH;version-u32=2;recipe-flags-u32;backend-utf8;attributes-u32;parameters-count-u32:name-utf8,kind-u8;backends-count-u32:name-utf8,vertex-utf8,fragment-utf8;immutable;interface=glsl-es-300-v1;shader-parameter-kinds=f32:1,i32:2,u32:3,bool:4,vec2:5,vec3:6,vec4:7,mat2:8,mat3:9,mat4:10,texture2D:11",
    },
    AssetFormat {
        name: "ASSET_GEOMETRY",
        capability: Capability::Spatial,
        type_id: ipp_core::systems::geometry::GEOMETRY_TYPE.0,
        format: "IPPG;version-u32=1;count-u32>=1;part=tag-u32,parameters-f32x7,trs-f32x10,joints-u32x2;box0=min3,max3,zero;sphere1=center3,radius,zero3;pill2=start3,end3,radius;joints-none=4294967295,4294967295;joint-pair-requires-skeleton:0..31-and-zero-endpoints;positive-radius;finite-values;exact-payload;union-leaf-order;geometry-skeleton-zero=own-skin-or-self",
    },
    #[cfg(not(feature = "skeletal-animation"))]
    AssetFormat {
        name: "ASSET_ANIMATION",
        capability: Capability::Animation,
        type_id: ipp_core::systems::animation::ANIMATION_TYPE.0,
        format: "IPPA;version-u32=1|3;v3-track=target-u8:property0|dynamic2;dynamic=component-u16,name-utf8;dynamic-value-kind11=length-u32,typed-value;dynamic-asset-tag12=type-u16,variant-u32,source-utf8;duration-f64;tracks-u32>=1;track=component-u16,offset-count-u8:1|4,offsets-u32,keys-u32;key=time-f64,value,curve-u8;value=field-kind-u8,payload;quaternion-kind=8:xyzw-f32;entity=u64;owned=length-u32,bytes;curve=step:0,linear:1,bezier:2;bezier=time1-f64,value1,time2-f64,value2;monotone-times;last-key-step;exact-payload",
    },
    #[cfg(feature = "skeletal-animation")]
    AssetFormat {
        name: "ASSET_ANIMATION",
        capability: Capability::Animation,
        type_id: ipp_core::systems::animation::ANIMATION_TYPE.0,
        format: "IPPA;version-u32=1|2|3;v3-dynamic-target2=component-u16,name-utf8;dynamic-value-kind11=length-u32,typed-value;dynamic-asset-tag12=type-u16,variant-u32,source-utf8;v2-track=target-u8:property0|joints1;joints=count-u32:1..32,ordinals-u32-ascending;pose-kind9=count-u32,trs-f32x10;pose-count-matches-joints;local-trs-blending;duration-f64;tracks-u32>=1;track=component-u16,offset-count-u8:1|4,offsets-u32,keys-u32;key=time-f64,value,curve-u8;value=field-kind-u8,payload;quaternion-kind=8:xyzw-f32;entity=u64;owned=length-u32,bytes;curve=step:0,linear:1,bezier:2;bezier=time1-f64,value1,time2-f64,value2;monotone-times;last-key-step;exact-payload",
    },
    #[cfg(feature = "skeletal-animation")]
    AssetFormat {
        name: "ASSET_SKELETON",
        capability: Capability::Spatial,
        type_id: ipp_core::SKELETON_TYPE.0,
        format: "IPPS;version=1;count-u32=1..32;parent-u32-root=4294967295;parent-precedes-child;trs-f32x10;exact-payload",
    },
    #[cfg(feature = "skeletal-animation")]
    AssetFormat {
        name: "ASSET_POSE",
        capability: Capability::Spatial,
        type_id: ipp_core::POSE_TYPE.0,
        format: "IPPP;version=1;count-u32=1..32;trs-f32x10;exact-payload",
    },
    #[cfg(feature = "skeletal-animation")]
    AssetFormat {
        name: "ASSET_SKIN",
        capability: Capability::Spatial,
        type_id: ipp_core::SKIN_TYPE.0,
        format: "IPPB;version=1;count-u32=1..32;joint-u32;inverse-bind-f32x16-column-major-affine-invertible;exact-payload",
    },
    AssetFormat {
        name: "ASSET_MESH",
        capability: Capability::Textures,
        type_id: ipp_core::MESH_TYPE.0,
        format: "IPPM;version=1|2|3;v1=position-f32x3,color-f32x3,index-u16;v2=position-f32x3,color-f32x3,uv-f32x2,index-u16;v3=ordered-attribute-descriptors-and-packed-streams,normal-f32x3-semantic4-finite-nonzero;finite-values;loaded-means-usable",
    },
    AssetFormat {
        name: "ASSET_TEXTURE",
        capability: Capability::Textures,
        type_id: ipp_core::TEXTURE_TYPE.0,
        format: "IPPT;version=3;width-u32;height-u32;rgba8-srgb-linear-alpha;top-left-row-first;exact-payload;loaded-means-usable",
    },
];

macro_rules! reasons {
    ($( $(#[$meta:meta])* $reason:ident),+ $(,)?) => {
        pub(crate) const REASONS: &[&str] = &[$( $(#[$meta])* stringify!($reason)),+];

        pub(crate) fn error_name(reason: ipp_core::ErrorReason) -> &'static str {
            match reason {
                $( $(#[$meta])* ipp_core::ErrorReason::$reason => stringify!($reason)),+
            }
        }
    };
}

reasons! {
    NonConvergentCommit,
    InvalidEntity,
    UnknownAlias,
    DuplicateAlias,
    DuplicateSymbolicId,
    UnknownComponent,
    MissingComponent,
    MissingCreationContract,
    InvalidField,
    InvalidValue,
    Capacity,
    UnsupportedDependency,
    InvalidStateOverlay,
    StateOverlayOwnershipMismatch,
    MissingSymbolicId,
    ComponentExists,
    InvalidAsset,
    MissingAsset,
    DuplicateAsset,
    NoActiveCamera,
    ActiveCamera,
    InvalidViewport,
    GeometryUnavailable,
    InvalidGeometry,
}

pub(crate) fn write_contract(sink: &mut impl ContractSink) {
    sink.write(&1u16.to_le_bytes());

    sink.write(&(CAPABILITIES.len() as u8).to_le_bytes());
    for (capability, enabled) in CAPABILITIES {
        sink.write(&[*capability as u8, u8::from(*enabled)]);
        write_string(sink, capability.name());
    }

    sink.write(&(CONVENTIONS.len() as u16).to_le_bytes());
    for (name, value) in CONVENTIONS {
        write_string(sink, name);
        write_string(sink, value);
    }

    sink.write(&(LAYOUTS.len() as u16).to_le_bytes());
    for layout in LAYOUTS {
        write_layout(sink, layout);
    }

    sink.write(&(TAGS.len() as u16).to_le_bytes());
    for tag in TAGS {
        write_string(sink, tag.name);
        sink.write(&[tag.space as u8, tag.capability as u8, tag.value]);
        write_string(sink, tag.layout);
    }

    sink.write(&(ASSET_FORMATS.len() as u16).to_le_bytes());
    for format in ASSET_FORMATS {
        write_string(sink, format.name);
        sink.write(&[format.capability as u8]);
        sink.write(&format.type_id.to_le_bytes());
        write_string(sink, format.format);
    }

    sink.write(&(REASONS.len() as u16).to_le_bytes());
    for reason in REASONS {
        write_string(sink, reason);
    }
}

fn write_layout(sink: &mut impl ContractSink, layout: &WireLayout) {
    write_string(sink, layout.name);
    sink.write(&[layout.capability as u8]);
    sink.write(&(layout.fields.len() as u16).to_le_bytes());
    for field in layout.fields {
        write_string(sink, field.name);
        sink.write(&[field.encoding as u8]);
        sink.write(&field.limit.to_le_bytes());
        write_string(sink, field.target);
    }
}

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;
