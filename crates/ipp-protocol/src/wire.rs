//! Canonical, machine-readable wire contract for codecs and SDK generation.

use ipp_core::components::schema::{ContractSink, write_string};

pub(crate) const HOST_REQUEST_MAGIC_HEX: &str = "4950504802000000";
pub(crate) const HOST_RESPONSE_MAGIC_HEX: &str = "4950504102000000";

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
    // Capability 1 is retired; retired capabilities are never reused.
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
    // Tag spaces 9-12 are retired; retired tag spaces are never reused.
    AssetResourceStatus = 13,
    SnapshotValue = 15,
    SnapshotReference = 16,
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
    OutputKind = 32,
    ViewTarget = 33,
    OperationEffect = 34,
    AttachmentReceiptState = 35,
    PresentationRequest = 36,
    PresentationResponse = 37,
    PresentationError = 38,
    LifecycleWatchChange = 39,
    LifecycleWatchTarget = 40,
    LifecycleWatchRecord = 41,
    LifecycleMembershipResult = 42,
    LifecycleTargetLifetime = 43,
    LifecycleMembershipRejection = 44,
    LifecycleWatchKinds = 45,
    GuiPhysicalRequest = 46,
    GuiPhysicalEvent = 47,
    GuiPhysicalResponse = 48,
    GuiPhysicalButton = 49,
    GuiPhysicalKey = 50,
    GuiNativeEdit = 51,
    GuiPhysicalDisposition = 52,
    #[cfg_attr(not(any(feature = "gui", test)), expect(dead_code))]
    GuiAction = 53,
    OutputTarget = 54,
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
    #[cfg_attr(not(any(feature = "gui", test)), expect(dead_code))]
    U8CountedList = 16,
}

/// Byte fields bounded only by the complete message budget, declared once from
/// [`crate::MAX_MESSAGE_BYTES`] so manifests, encoders and generated codecs agree.
pub(crate) const MESSAGE_BYTES: u32 = crate::MAX_MESSAGE_BYTES as u32;
pub(crate) const MAX_ANIMATION_ENTITY_BINDINGS: u32 = MESSAGE_BYTES / 8;
/// Ordinary text and byte fields, declared once from [`crate::MAX_FIELD_BYTES`].
pub(crate) const FIELD_BYTES: u32 = crate::MAX_FIELD_BYTES as u32;
/// Inspection and entity-tree page records, from [`crate::INSPECTION_PAGE_RECORDS`].
const PAGE_RECORDS: u32 = crate::INSPECTION_PAGE_RECORDS as u32;
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
    #[cfg(feature = "gui")]
    "gui-target" [Base] => [field!("world", Named => "world-reference"), field!("entity", U64), field!("component", U16), field!("incarnation", U64)];
    // GUI System query records, read through inspection collections 6 and 7.
    #[cfg(feature = "gui")]
    "gui-focus" [Base] => [field!("target", Named => "gui-target"), field!("visible", Bool)];
    #[cfg(feature = "gui")]
    "gui-pointer" [Base] => [
        field!("target", Named => "gui-target"), field!("pointer", U64),
        field!("hovered", Bool), field!("pressed", Bool), field!("captured", Bool),
    ];
    #[cfg(feature = "gui")]
    "request-gui-observation" [Base] => [field!("session", U64), field!("request_id", U64), field!("tag", Variant => "request"), field!("control", Bytes, 64)];
    #[cfg(feature = "gui")]
    "response-gui-observation" [Base] => [field!("session", U64), field!("request_id", U64), field!("tick", U64), field!("tag", Variant => "response"), field!("record", Bytes, MESSAGE_BYTES)];
    "host-request-resolve-world" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("world", Union => "world-selector")];
    "host-request-bind-output" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("world", Named => "world-reference"), field!("entity", U64), field!("kind", Union => "output-kind")];
    "host-request-resolve-output" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("output", Named => "output-reference")];
    "host-request-set-root-output" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("output", Named => "output-reference"), field!("width", U32), field!("height", U32), field!("device_pixel_ratio", NonnegativeFiniteF64)];
    "host-request-clear-root-output" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("binding", Named => "root-binding")];
    "presentation-identity" [Base] => [field!("host", U64), field!("serial", U64)];
    "root-binding" [Base] => [field!("output", Named => "output-reference"), field!("width", U32), field!("height", U32), field!("device_pixel_ratio", NonnegativeFiniteF64), field!("generation", Named => "presentation-identity")];
    "presentation-surface" [Base] => [field!("id", U64), field!("context", U64), field!("max_width", U32), field!("max_height", U32)];
    "presentation-view" [Base] => [field!("surface", Named => "presentation-surface"), field!("selection", U64), field!("binding", Named => "root-binding")];
    "presented-source" [Base] => [field!("output", Named => "output-reference"), field!("minimum_tick", U64), field!("publication", Named => "presentation-identity"), field!("tick", U64)];
    "presented-frame" [Base] => [field!("view", Named => "presentation-view"), field!("sequence", U64), field!("publication", Named => "presentation-identity"), field!("draw_calls", U32), field!("triangles", U32), field!("failed_draw_calls", U32), field!("sources", List, crate::presentation::MAX_PRESENTATION_SOURCES as u32 => "presented-source")];
    "presentation-request-surface" [Base] => [field!("tag", Variant => "presentation-request")];
    "presentation-request-select" [Base] => [field!("tag", Variant => "presentation-request"), field!("surface", Named => "presentation-surface"), field!("binding", Named => "root-binding")];
    "presentation-request-clear" [Base] => [field!("tag", Variant => "presentation-request"), field!("view", Named => "presentation-view")];
    "presentation-request-frame" [Base] => [field!("tag", Variant => "presentation-request"), field!("view", Named => "presentation-view"), field!("after_sequence", Option => "u64"), field!("publication", Option => "presentation-identity"), field!("capture", Bool), field!("after_outputs", List, crate::presentation::MAX_PRESENTATION_SOURCES as u32 => "output-reference")];
    "presentation-request-read-capture" [Base] => [field!("tag", Variant => "presentation-request"), field!("capture", U64), field!("offset", U64)];
    "presentation-request-release-capture" [Base] => [field!("tag", Variant => "presentation-request"), field!("capture", U64)];
    "presentation-request-cancel-frame" [Base] => [field!("tag", Variant => "presentation-request"), field!("request", U64)];
    "presentation-response-surface" [Base] => [field!("tag", Variant => "presentation-response"), field!("surface", Named => "presentation-surface")];
    "presentation-response-view" [Base] => [field!("tag", Variant => "presentation-response"), field!("view", Named => "presentation-view")];
    "presentation-response-frame" [Base] => [field!("tag", Variant => "presentation-response"), field!("frame", Named => "presented-frame")];
    "presentation-response-capture" [Base] => [field!("tag", Variant => "presentation-response"), field!("frame", Named => "presented-frame"), field!("capture", U64), field!("bytes", U64)];
    "presentation-response-chunk" [Base] => [field!("tag", Variant => "presentation-response"), field!("capture", U64), field!("offset", U64), field!("bytes", Bytes, FIELD_BYTES)];
    "presentation-response-complete" [Base] => [field!("tag", Variant => "presentation-response")];
    "presentation-response-error" [Base] => [field!("tag", Variant => "presentation-response"), field!("error", Union => "presentation-error")];
    "presentation-error-unsupported" [Base] => [field!("tag", Variant => "presentation-error")];
    "presentation-error-unavailable" [Base] => [field!("tag", Variant => "presentation-error")];
    "presentation-error-stale-view" [Base] => [field!("tag", Variant => "presentation-error")];
    "presentation-error-invalid-viewport" [Base] => [field!("tag", Variant => "presentation-error")];
    "presentation-error-obsolete-publication" [Base] => [field!("tag", Variant => "presentation-error")];
    "presentation-error-capacity" [Base] => [field!("tag", Variant => "presentation-error")];
    "presentation-error-timeout" [Base] => [field!("tag", Variant => "presentation-error")];
    "presentation-error-draw-failed" [Base] => [field!("tag", Variant => "presentation-error")];
    "host-request-presentation" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("body", Union => "presentation-request")];
    #[cfg(feature = "gui")]
    "gui-physical-open" [Base] => [field!("tag", Variant => "gui-physical-request"), field!("view", Named => "presentation-view"), field!("blockers", List, MESSAGE_BYTES / 32 => "gui-picking-blocker")];
    #[cfg(feature = "gui")]
    "gui-physical-close" [Base] => [field!("tag", Variant => "gui-physical-request"), field!("context", U64)];
    #[cfg(feature = "gui")]
    "gui-physical-event" [Base] => [field!("tag", Variant => "gui-physical-request"), field!("context", U64), field!("input", Union => "gui-physical-event")];
    #[cfg(feature = "gui")]
    "gui-physical-text" [Base] => [field!("tag", Variant => "gui-physical-request"), field!("context", U64), field!("target", Named => "gui-physical-target"), field!("generation", U64), field!("edit", Union => "gui-native-edit")];
    #[cfg(feature = "gui")]
    "gui-physical-pointer-down" [Base] => [field!("tag", Variant => "gui-physical-event"), field!("pointer", U64), field!("point", Named => "gui-input-vector"), field!("button", Variant => "gui-physical-button")];
    #[cfg(feature = "gui")]
    "gui-physical-pointer-move" [Base] => [field!("tag", Variant => "gui-physical-event"), field!("pointer", U64), field!("point", Named => "gui-input-vector")];
    #[cfg(feature = "gui")]
    "gui-physical-pointer-up" [Base] => [field!("tag", Variant => "gui-physical-event"), field!("pointer", U64), field!("point", Named => "gui-input-vector"), field!("button", Variant => "gui-physical-button")];
    #[cfg(feature = "gui")]
    "gui-physical-pointer-cancel" [Base] => [field!("tag", Variant => "gui-physical-event"), field!("pointer", U64)];
    #[cfg(feature = "gui")]
    "gui-physical-wheel" [Base] => [field!("tag", Variant => "gui-physical-event"), field!("point", Named => "gui-input-vector"), field!("delta", Named => "gui-input-vector")];
    #[cfg(feature = "gui")]
    "gui-physical-key" [Base] => [field!("tag", Variant => "gui-physical-event"), field!("key", Variant => "gui-physical-key")];
    #[cfg(feature = "gui")]
    "gui-physical-blur" [Base] => [field!("tag", Variant => "gui-physical-event")];
    #[cfg(feature = "gui")]
    "gui-physical-opened" [Base] => [field!("tag", Variant => "gui-physical-response"), field!("context", U64)];
    #[cfg(feature = "gui")]
    "gui-physical-closed" [Base] => [field!("tag", Variant => "gui-physical-response")];
    #[cfg(feature = "gui")]
    "gui-physical-routed" [Base] => [field!("tag", Variant => "gui-physical-response"), field!("disposition", Variant => "gui-physical-disposition"), field!("applied", U32), field!("rejected", U32), field!("cancelled", U32), field!("error", Option => "utf8-65536"), field!("remaining", Option => "gui-input-vector"), field!("native", Option => "gui-native-buffer")];
    #[cfg(feature = "gui")]
    "gui-physical-rejected" [Base] => [field!("tag", Variant => "gui-physical-response"), field!("reason", Utf8, FIELD_BYTES)];
    #[cfg(feature = "gui")]
    "gui-physical-revoked" [Base] => [field!("tag", Variant => "gui-physical-response"), field!("context", U64)];
    #[cfg(feature = "gui")]
    "gui-physical-cancelled" [Base] => [field!("tag", Variant => "gui-physical-response"), field!("context", U64), field!("pointers", U8CountedList, ipp_core::services::gui_input::GUI_INPUT_MAX_POINTERS as u32 => "u64"), field!("focus", Bool)];
    #[cfg(feature = "gui")]
    "gui-physical-native" [Base] => [field!("tag", Variant => "gui-physical-response"), field!("context", U64), field!("state", Option => "gui-native-buffer")];
    #[cfg(feature = "gui")]
    "gui-physical-button-primary" [Base] => [field!("tag", Variant => "gui-physical-button")];
    #[cfg(feature = "gui")]
    "gui-physical-button-secondary" [Base] => [field!("tag", Variant => "gui-physical-button")];
    #[cfg(feature = "gui")]
    "gui-physical-button-auxiliary" [Base] => [field!("tag", Variant => "gui-physical-button")];
    #[cfg(feature = "gui")]
    "gui-physical-key-tab" [Base] => [field!("tag", Variant => "gui-physical-key")];
    #[cfg(feature = "gui")]
    "gui-physical-key-back-tab" [Base] => [field!("tag", Variant => "gui-physical-key")];
    #[cfg(feature = "gui")]
    "gui-physical-key-enter" [Base] => [field!("tag", Variant => "gui-physical-key")];
    #[cfg(feature = "gui")]
    "gui-physical-key-space" [Base] => [field!("tag", Variant => "gui-physical-key")];
    #[cfg(feature = "gui")]
    "gui-physical-key-escape" [Base] => [field!("tag", Variant => "gui-physical-key")];
    #[cfg(feature = "gui")]
    "gui-physical-key-left" [Base] => [field!("tag", Variant => "gui-physical-key")];
    #[cfg(feature = "gui")]
    "gui-physical-key-right" [Base] => [field!("tag", Variant => "gui-physical-key")];
    #[cfg(feature = "gui")]
    "gui-physical-key-up" [Base] => [field!("tag", Variant => "gui-physical-key")];
    #[cfg(feature = "gui")]
    "gui-physical-key-down" [Base] => [field!("tag", Variant => "gui-physical-key")];
    #[cfg(feature = "gui")]
    "gui-physical-key-home" [Base] => [field!("tag", Variant => "gui-physical-key")];
    #[cfg(feature = "gui")]
    "gui-physical-key-end" [Base] => [field!("tag", Variant => "gui-physical-key")];
    #[cfg(feature = "gui")]
    "gui-native-insert" [Base] => [field!("tag", Variant => "gui-native-edit"), field!("text", Utf8, FIELD_BYTES)];
    #[cfg(feature = "gui")]
    "gui-native-selection" [Base] => [field!("tag", Variant => "gui-native-edit"), field!("start", U32), field!("end", U32)];
    #[cfg(feature = "gui")]
    "gui-native-compose" [Base] => [field!("tag", Variant => "gui-native-edit"), field!("composition", Named => "gui-native-composition")];
    #[cfg(feature = "gui")]
    "gui-native-commit-composition" [Base] => [field!("tag", Variant => "gui-native-edit")];
    #[cfg(feature = "gui")]
    "gui-native-cancel-composition" [Base] => [field!("tag", Variant => "gui-native-edit")];
    #[cfg(feature = "gui")]
    "gui-native-backspace" [Base] => [field!("tag", Variant => "gui-native-edit")];
    #[cfg(feature = "gui")]
    "gui-native-delete" [Base] => [field!("tag", Variant => "gui-native-edit")];
    #[cfg(feature = "gui")]
    "gui-native-left" [Base] => [field!("tag", Variant => "gui-native-edit")];
    #[cfg(feature = "gui")]
    "gui-native-right" [Base] => [field!("tag", Variant => "gui-native-edit")];
    #[cfg(feature = "gui")]
    "gui-native-home" [Base] => [field!("tag", Variant => "gui-native-edit")];
    #[cfg(feature = "gui")]
    "gui-native-end" [Base] => [field!("tag", Variant => "gui-native-edit")];
    #[cfg(feature = "gui")]
    "gui-native-select-all" [Base] => [field!("tag", Variant => "gui-native-edit")];
    #[cfg(feature = "gui")]
    "gui-native-submit" [Base] => [field!("tag", Variant => "gui-native-edit")];
    #[cfg(feature = "gui")]
    "gui-physical-disposition-routed" [Base] => [field!("tag", Variant => "gui-physical-disposition")];
    #[cfg(feature = "gui")]
    "gui-physical-disposition-miss" [Base] => [field!("tag", Variant => "gui-physical-disposition")];
    #[cfg(feature = "gui")]
    "gui-physical-disposition-blocked" [Base] => [field!("tag", Variant => "gui-physical-disposition")];
    #[cfg(feature = "gui")]
    "gui-physical-disposition-unhandled" [Base] => [field!("tag", Variant => "gui-physical-disposition")];
    #[cfg(feature = "gui")]
    "gui-input-vector" [Base] => [field!("x", FiniteF32), field!("y", FiniteF32)];
    #[cfg(feature = "gui")]
    "gui-picking-blocker" [Base] => [field!("world", Named => "world-reference"), field!("entity", U64), field!("incarnation", U64)];
    #[cfg(feature = "gui")]
    "gui-physical-target" [Base] => [field!("world", Named => "world-reference"), field!("entity", U64), field!("component", U16), field!("incarnation", U64)];
    #[cfg(feature = "gui")]
    "gui-native-buffer" [Base] => [field!("byte_length", U32), field!("state", Named => "gui-native-state")];
    #[cfg(feature = "gui")]
    "gui-native-composition" [Base] => [field!("text", Utf8, FIELD_BYTES), field!("selection_start", U32), field!("selection_end", U32)];
    #[cfg(feature = "gui")]
    "gui-native-state" [Base] => [field!("target", Named => "gui-physical-target"), field!("generation", U64), field!("text", Utf8, FIELD_BYTES), field!("selection_start", U32), field!("selection_end", U32), field!("composition", Option => "gui-native-composition")];
    #[cfg(feature = "gui")]
    "host-request-gui-input" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("body_bytes", U32), field!("body", Union => "gui-physical-request")];
    #[cfg(feature = "gui")]
    "host-response-gui-input" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("body_bytes", U32), field!("body", Union => "gui-physical-response")];
    "host-request-get-root-output-binding" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("world", Named => "world-reference")];
    "host-response-presentation" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("body", Union => "presentation-response")];
    "host-response-root-binding" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("binding", Option => "root-binding")];
    "host-response-world-reference" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("reference", Named => "world-reference")];
    "host-response-output-reference" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("reference", Named => "output-reference")];
    "host-request-list-worlds" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("after", U64)];
    "host-request-create-world" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("symbolic_id", Utf8, FIELD_BYTES), field!("hints", Named => "host-hints-patch"), field!("selected_systems", Option => "host-system-selection"), field!("canvas", Option => "canvas-state"), field!("temporary", Bool)];
    "host-request-open-world" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("world", Named => "world-reference")];
    "host-request-rename-world" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("world", Union => "world-selector"), field!("symbolic_id", Utf8, FIELD_BYTES)];
    "host-request-destroy-world" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("world", Named => "world-reference")];
    "host-request-detach-world" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("session", U64)];
    "host-request-set-capacity-hints" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("session", U64), field!("hints", Named => "host-hints-patch")];
    "host-request-save-world" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("session", U64)];
    "host-request-read-world-save" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("job", U64), field!("offset", U64)];
    "host-request-begin-world-load" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("bytes", U64)];
    "host-request-write-world-load" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("job", U64), field!("offset", U64), field!("bytes", Bytes, FIELD_BYTES)];
    "host-request-finish-world-load" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("job", U64), field!("symbolic_id", Option => "utf8-65536"), field!("hints", Named => "host-hints-patch")];
    "host-request-inspect-world-load" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("job", U64), field!("offset", U32)];
    "host-request-set-world-load-names" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("job", U64), field!("names", List, crate::host::MAX_GRAPH_METADATA_PAGE as u32 => "graph-node-name")];
    "host-request-read-world-load-bindings" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("job", U64), field!("offset", U32)];
    "host-request-acknowledge-world-load" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("job", U64)];
    "host-response-world-graph-page" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("job", U64), field!("root", U32), field!("total", U32), field!("offset", U32), field!("nodes", List, crate::host::MAX_GRAPH_METADATA_PAGE as u32 => "graph-node-metadata")];
    "host-response-world-graph-loaded" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("job", U64), field!("root", Named => "world-reference"), field!("total", U32)];
    "host-response-world-graph-bindings" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("job", U64), field!("offset", U32), field!("bindings", List, crate::host::MAX_GRAPH_BINDING_PAGE as u32 => "graph-node-binding")];
    "graph-node-name" [Base] => [field!("id", U32), field!("symbolic_id", Utf8, FIELD_BYTES)];
    "graph-node-metadata" [Base] => [field!("id", U32), field!("symbolic_id", Utf8, FIELD_BYTES), field!("persistent_id_low", U64), field!("persistent_id_high", U64)];
    "graph-node-binding" [Base] => [field!("id", U32), field!("world", Named => "world-reference")];
    "host-request-cancel-world-transfer" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("job", U64)];
    "host-response-worlds" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("worlds", List, 32 => "host-world"), field!("next", U64)];
    "host-response-created" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("world", Named => "host-world"), field!("reference", Named => "world-reference")];
    "host-response-attached" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("world", Named => "host-world"), field!("session", U64), field!("manifest", Named => "world-manifest"), field!("reference", Named => "world-reference")];
    "host-response-world" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("world", Named => "host-world")];
    "host-response-complete" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response")];
    "host-response-error" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("message", Utf8, FIELD_BYTES)];
    "host-response-detached" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("session", U64), field!("reason", Utf8, FIELD_BYTES)];
    "host-response-transfer" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("job", U64)];
    "host-response-save-chunk" [Base] => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("job", U64), field!("offset", U64), field!("total", U64), field!("bytes", Bytes, FIELD_BYTES)];
    "host-world" [Base] => [field!("id", U64), field!("symbolic_id", Utf8, FIELD_BYTES), field!("persistent_id_low", U64), field!("persistent_id_high", U64), field!("entities", U32), field!("systems", List, 1024 => "host-system-hints")];
    "host-hints-patch" [Base] => [field!("entities", Option => "u32"), field!("systems", List, 1024 => "host-system-hints")];
    "host-system-hints" [Base] => [field!("system", Utf8, FIELD_BYTES), field!("values", List, 1024 => "host-capacity-hint")];
    "host-system-selection" [Base] => [field!("systems", List, 1024 => "utf8-65536")];
    "world-manifest" [Base] => [field!("systems", List, 1024 => "utf8-65536"), field!("components", List, 1024 => "u16"), field!("operations", List, 32 => "u8")];
    "host-capacity-hint" [Base] => [field!("name", Utf8, FIELD_BYTES), field!("value", U32)];
    "world-selector-id" [Base] => [field!("tag", Variant => "world-selector"), field!("value", U64)];
    "world-selector-symbol" [Base] => [field!("tag", Variant => "world-selector"), field!("value", Utf8, FIELD_BYTES)];

    "value-dynamic" [Base] => [field!("tag", Variant => "value"), field!("value", Bytes, FIELD_BYTES)];
    "snapshot-value-dynamic" [Base] => [field!("tag", Variant => "snapshot-value"), field!("value", Bytes, FIELD_BYTES)];
    "command-set-dynamic-property" [Base] => [field!("tag", Variant => "command"), field!("entity", Union => "reference"), field!("component", U16), field!("name", Utf8, FIELD_BYTES), field!("value", Bytes, FIELD_BYTES)];
    "command-remove-dynamic-property" [Base] => [field!("tag", Variant => "command"), field!("entity", Union => "reference"), field!("component", U16), field!("name", Utf8, FIELD_BYTES)];
    "request-lifecycle-unsubscribe" [Base] => [
        field!("session", U64), field!("request_id", U64), field!("tag", Variant => "request"), field!("subscription", U64),
    ];
    "response-lifecycle-subscription" [Base] => [
        field!("session", U64), field!("request_id", U64), field!("tick", U64), field!("tag", Variant => "response"),
    ];
    "response-lifecycle-events" [Base] => [
        field!("session", U64), field!("request_id", U64), field!("tick", U64), field!("tag", Variant => "response"),
        field!("events", List, crate::MAX_LIFECYCLE_PUBLICATIONS as u32 => "lifecycle-publication"),
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
    "request-geometry-pick" [Picking] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("view", Union => "view-target"),
        field!("x", FiniteF32),
        field!("y", FiniteF32),
        field!("include_view_plane", Bool),
    ];
    "request-camera-project" [Picking] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("view", Union => "view-target"),
        field!("x", FiniteF32),
        field!("y", FiniteF32),
        field!("plane", Named => "pick-view-plane"),
    ];
    "response-camera-project" [Picking] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("view", Option => "view-descriptor"),
        field!("ok", Bool),
        field!("position", Option => "world-point"),
        field!("error", Option => "utf8-65536"),
    ];
    "world-point" [Picking] => [
        field!("x", FiniteF32),
        field!("y", FiniteF32),
        field!("z", FiniteF32),
    ];
    "response-geometry-pick" [Picking] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("result", Union => "geometry-pick-outcome"),
    ];
    "view-root" [Picking] => [
        field!("tag", Variant => "view-target"),
        field!("output", Named => "output-reference"),
        field!("expected_viewport", Named => "view-viewport"),
    ];
    "view-publication" [Picking] => [
        field!("tag", Variant => "view-target"),
        field!("output", Named => "output-reference"),
        field!("publication", Named => "publication-reference"),
        field!("viewport", Named => "view-viewport"),
    ];
    "view-bound" [Picking] => [field!("tag", Variant => "view-target"), field!("binding", Named => "root-binding"), field!("publication", Option => "publication-reference")];
    "request-camera-navigate" [Picking] => [field!("session", U64), field!("request_id", U64), field!("tag", Variant => "request"), field!("binding", Named => "root-binding"), field!("publication", Option => "publication-reference"), field!("kind", U32), field!("first", FiniteF32), field!("second", FiniteF32)];
    "response-camera-navigated" [Picking] => [field!("session", U64), field!("request_id", U64), field!("tick", U64), field!("tag", Variant => "response")];
    "view-viewport" [Picking] => [field!("width", U32), field!("height", U32), field!("device_pixel_ratio", NonnegativeFiniteF64)];
    "publication-reference" [Picking] => [field!("host", U64), field!("revision", U64)];
    "view-descriptor" [Picking] => [
        field!("output", Named => "output-reference"),
        field!("publication", Named => "publication-reference"),
        field!("viewport", Named => "view-viewport"),
    ];
    "view-path-entry" [Picking] => [field!("world", Named => "world-reference"), field!("anchor", U64)];
    "pick-result-miss" [Picking] => [field!("tag", Variant => "geometry-pick-outcome"), field!("view", Named => "view-descriptor")];
    "pick-result-hit" [Picking] => [
        field!("tag", Variant => "geometry-pick-outcome"),
        field!("view", Named => "view-descriptor"),
        field!("world", Named => "world-reference"),
        field!("publication", Named => "publication-reference"),
        field!("entity", U64),
        field!("incarnation", U64),
        field!("position_x", FiniteF32),
        field!("position_y", FiniteF32),
        field!("position_z", FiniteF32),
        field!("distance", NonnegativeFiniteF64),
        field!("part", U32),
        field!("path", List, (crate::MAX_MESSAGE_BYTES / 24) as u32 => "view-path-entry"),
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
        field!("reason", Utf8, FIELD_BYTES),
    ];

    "metadata" [Base] => [
        field!("symbolic_id", Option => "utf8-65536"),
        field!("classes", List, crate::MAX_METADATA_CLASSES as u32 => "utf8-65536"),
    ];
    "field" [Base] => [
        field!("offset", U32),
        field!("value", Union => "value"),
    ];
    "component" [Base] => [
        field!("type_id", U16),
        field!("fields", List, crate::MAX_INSPECTED_FIELDS as u32 => "snapshot-field"),
    ];
    "snapshot-field" [Base] => [
        field!("offset", U32),
        field!("value", Union => "snapshot-value"),
    ];
    "entity" [Base] => [
        field!("id", U64),
        field!("metadata", Named => "metadata"),
        field!("parent", U64),
        field!("order_low", U64),
        field!("order_high", U64),
        field!("components", List, crate::MAX_INSPECTED_COMPONENTS as u32 => "component"),
    ];
    "alias-handle" [Base] => [
        field!("alias", U32),
        field!("handle", U64),
    ];
    "symbol-handle" [Base] => [
        field!("symbol", Utf8, FIELD_BYTES),
        field!("handle", U64),
    ];
    "request-lifecycle-watch" [Base] => [
        field!("session", U64), field!("request_id", U64), field!("tag", Variant => "request"),
        field!("world", Named => "world-reference"),
        field!("change", Union => "lifecycle-watch-change"),
    ];
    "lifecycle-watch-add" [Base] => [
        field!("tag", Variant => "lifecycle-watch-change"),
        field!("members", List, crate::lifecycle_watch::MAX_LIFECYCLE_MEMBERS as u32 => "lifecycle-watch-selection"),
    ];
    "lifecycle-watch-remove" [Base] => [
        field!("tag", Variant => "lifecycle-watch-change"), field!("output", U64),
        field!("generations", List, crate::lifecycle_watch::MAX_LIFECYCLE_MEMBERS as u32 => "u64"),
    ];
    "lifecycle-watch-selection" [Base] => [
        field!("target", Union => "lifecycle-watch-target"), field!("kinds", Variant => "lifecycle-watch-kinds"),
    ];
    "lifecycle-watch-entity" [Base] => [
        field!("tag", Variant => "lifecycle-watch-target"), field!("entity", U64),
    ];
    "lifecycle-watch-component" [Base] => [
        field!("tag", Variant => "lifecycle-watch-target"), field!("entity", U64), field!("component", U16),
    ];
    // Schema field offsets of one component, strictly ascending; never rows or named properties.
    "lifecycle-watch-value" [Base] => [
        field!("tag", Variant => "lifecycle-watch-target"), field!("entity", U64), field!("component", U16),
        field!("fields", List, crate::lifecycle_watch::MAX_LIFECYCLE_VALUE_FIELDS as u32 => "u32"),
    ];
    #[cfg(feature = "diagnostics")]
    "request-lifecycle-diagnostics" [Base] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("world", Named => "world-reference"),
        field!("output", U64),
    ];
    #[cfg(feature = "diagnostics")]
    "response-lifecycle-diagnostics" [Base] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("world", Named => "world-reference"),
        field!("output", U64),
        field!("lookups", U64),
        field!("recipient_visits", U64),
        field!("work_saturated", Bool),
        field!("queued_events", U64),
        field!("queued_bytes", U64),
        field!("traffic_saturated", Bool),
    ];
    "response-lifecycle-watch" [Base] => [
        field!("session", U64), field!("request_id", U64), field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("world", Named => "world-reference"), field!("output", U64),
        field!("record", Union => "lifecycle-watch-record"),
    ];
    "lifecycle-watch-ack" [Base] => [
        field!("tag", Variant => "lifecycle-watch-record"), field!("action", Variant => "lifecycle-watch-change"),
        field!("cut", Option => "lifecycle-watch-cut"), field!("result", Union => "lifecycle-membership-result"),
    ];
    "lifecycle-watch-cut" [Base] => [field!("sequence", U64), field!("tick", U64)];
    "lifecycle-watch-applied" [Base] => [
        field!("tag", Variant => "lifecycle-membership-result"),
        field!("baselines", List, crate::lifecycle_watch::MAX_LIFECYCLE_MEMBERS as u32 => "lifecycle-watch-baseline"),
    ];
    "lifecycle-watch-rejected" [Base] => [
        field!("tag", Variant => "lifecycle-membership-result"), field!("reason", Variant => "lifecycle-membership-rejection"),
    ];
    "lifecycle-watch-cancelled" [Base] => [field!("tag", Variant => "lifecycle-membership-result")];
    "lifecycle-watch-baseline" [Base] => [
        field!("generation", U64), field!("target", Union => "lifecycle-watch-target"), field!("lifetime", Union => "lifecycle-target-lifetime"),
    ];
    "lifecycle-watch-event" [Base] => [
        field!("tag", Variant => "lifecycle-watch-record"), field!("generation", U64), field!("sequence", U64), field!("tick", U64),
        field!("observation", Union => "lifecycle-observation"),
    ];
    // Current values of a value member at the end of an evaluated frame; absent
    // while its entity or component is absent.
    "lifecycle-watch-value-record" [Base] => [
        field!("tag", Variant => "lifecycle-watch-record"), field!("generation", U64), field!("tick", U64),
        field!("values", Option => "lifecycle-watch-values"),
    ];
    // Snapshot-encoded field values in the target's field order.
    "lifecycle-watch-values" [Base] => [
        field!("fields", List, crate::lifecycle_watch::MAX_LIFECYCLE_VALUE_FIELDS as u32 => "snapshot-field"),
    ];
    "lifecycle-lifetime-entity" [Base] => [field!("tag", Variant => "lifecycle-target-lifetime"), field!("live", Bool)];
    "lifecycle-lifetime-component" [Base] => [
        field!("tag", Variant => "lifecycle-target-lifetime"), field!("entity_live", Bool), field!("incarnation", U64),
    ];
    "lifecycle-lifetime-removed" [Base] => [field!("tag", Variant => "lifecycle-target-lifetime")];
    "resource" [Assets] => [
        field!("id", U64),
        field!("kind", U16),
        field!("source", Utf8, FIELD_BYTES),
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
        field!("reason", Utf8, FIELD_BYTES),
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
        field!("value", Utf8, FIELD_BYTES),
    ];
    "value-bytes" [Base] => [
        field!("tag", Variant => "value"),
        field!("value", Bytes, FIELD_BYTES),
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
        field!("value", Utf8, FIELD_BYTES),
    ];
    "snapshot-value-bytes" [Base] => [
        field!("tag", Variant => "snapshot-value"),
        field!("value", Bytes, MESSAGE_BYTES),
    ];
    // A whole schema rows table in the component's row layout.
    "world-reference" [Base] => [
        field!("id", U64),
        field!("incarnation", U64),
    ];
    "output-reference" [Base] => [
        field!("world", Named => "world-reference"),
        field!("target", Union => "output-target"),
    ];
    "output-canvas" [Base] => [field!("tag", Variant => "output-kind")];
    "output-camera" [Base] => [field!("tag", Variant => "output-kind")];
    // The World-level canvas; its lifetime is the World's.
    "output-target-canvas" [Base] => [field!("tag", Variant => "output-target")];
    "output-target-camera" [Base] => [
        field!("tag", Variant => "output-target"),
        field!("entity", U64),
        field!("incarnation", U64),
    ];
    "canvas-extent" [Base] => [field!("width", FiniteF32), field!("height", FiniteF32)];
    "canvas-density" [Base] => [field!("units_per_metre", FiniteF32)];
    "canvas-state" [Base] => [
        field!("extent", Named => "canvas-extent"),
        field!("density", Named => "canvas-density"),
    ];
    #[cfg(feature = "surfaces")]
    "canvas-state-update" [Base] => [
        field!("mask", U16),
        field!("extent", Masked, 1 => "canvas-extent"),
        field!("density", Masked, 2 => "canvas-density"),
    ];
    #[cfg(feature = "surfaces")]
    "request-canvas-state-update" [Base] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("update", Named => "canvas-state-update"),
    ];
    // Canvas System query record, read through inspection collection 8.
    #[cfg(feature = "surfaces")]
    "canvas-evaluated-extent" [Base] => [
        field!("extent", Named => "canvas-extent"),
        field!("tick", U64),
    ];
    #[cfg(feature = "surfaces")]
    "canvas-state-record" [Base] => [
        field!("state", Named => "canvas-state"),
        field!("evaluated", Option => "canvas-evaluated-extent"),
    ];
    "value-world" [Base] => [
        field!("tag", Variant => "value"),
        field!("value", Option => "world-reference"),
    ];
    "value-output" [Base] => [
        field!("tag", Variant => "value"),
        field!("value", Option => "output-reference"),
    ];
    "snapshot-value-world" [Base] => [
        field!("tag", Variant => "snapshot-value"),
        field!("value", Option => "world-reference"),
    ];
    "snapshot-value-output" [Base] => [
        field!("tag", Variant => "snapshot-value"),
        field!("value", Option => "output-reference"),
    ];
    "value-rows" [Base] => [
        field!("tag", Variant => "value"),
        field!("value", Bytes, MESSAGE_BYTES),
    ];
    "snapshot-value-rows" [Base] => [
        field!("tag", Variant => "snapshot-value"),
        field!("value", Bytes, MESSAGE_BYTES),
    ];
    // Absence of an optional schema row property.
    "value-unset" [Base] => [
        field!("tag", Variant => "value"),
    ];
    "snapshot-value-unset" [Base] => [
        field!("tag", Variant => "snapshot-value"),
    ];

    "request-submit-batch" [Base] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("batch_id", U32),
        field!("last", Bool),
        field!("operations", List, crate::COMMAND_PAGE_COMMANDS as u32 => "command"),
    ];
    "request-inspect" [Base] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("collection", Variant => "inspection-collection"),
        field!("after", U64),
        field!("target", U64),
        field!("limit", U16),
        field!("max_depth", U16),
    ];

    "command-create" [Base] => [
        field!("tag", Variant => "command"),
        field!("alias", U32),
        field!("metadata", Named => "metadata"),
        field!("adopt", Bool),
    ];
    "command-delete" [Base] => [
        field!("tag", Variant => "command"),
        field!("entity", Union => "reference"),
    ];
    "entity-placement" [Base] => [
        field!("parent", Option => "reference-value"),
        field!("before", Option => "reference-value"),
    ];
    "reference-value" [Base] => [field!("value", Union => "reference")];
    "command-place-entity" [Base] => [
        field!("tag", Variant => "command"),
        field!("entity", Union => "reference"),
        field!("placement", Named => "entity-placement"),
    ];
    "command-delete-subtree" [Base] => [
        field!("tag", Variant => "command"),
        field!("root", Union => "reference"),
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
        field!("fields", List, crate::MAX_INSERT_FIELDS as u32 => "field"),
        field!("adopt", Bool),
    ];
    "command-set" [Base] => [
        field!("tag", Variant => "command"),
        field!("entity", Union => "reference"),
        field!("component", U16),
        field!("field", Named => "field"),
    ];
    // Compare-and-set: `expected` has the field's own type at the field's offset.
    "command-set-field-if" [Base] => [
        field!("tag", Variant => "command"),
        field!("entity", Union => "reference"),
        field!("component", U16),
        field!("field", Named => "field"),
        field!("expected", Union => "value"),
    ];
    // One semantic action on the control component at its exact incarnation.
    #[cfg(feature = "gui")]
    "command-gui-action" [Base] => [
        field!("tag", Variant => "command"),
        field!("entity", Union => "reference"),
        field!("component", U16),
        field!("incarnation", U64),
        field!("action", Union => "gui-action"),
    ];
    #[cfg(feature = "gui")]
    "gui-action-press" [Base] => [field!("tag", Variant => "gui-action")];
    #[cfg(feature = "gui")]
    "gui-action-toggle" [Base] => [field!("tag", Variant => "gui-action")];
    #[cfg(feature = "gui")]
    "gui-action-set-scalar" [Base] => [field!("tag", Variant => "gui-action"), field!("value", FiniteF32)];
    #[cfg(feature = "gui")]
    "gui-action-set-text" [Base] => [field!("tag", Variant => "gui-action"), field!("text", Utf8, FIELD_BYTES)];
    #[cfg(feature = "gui")]
    "gui-action-focus" [Base] => [field!("tag", Variant => "gui-action")];
    #[cfg(feature = "gui")]
    "gui-action-blur" [Base] => [field!("tag", Variant => "gui-action")];
    #[cfg(feature = "gui")]
    "gui-action-submit" [Base] => [field!("tag", Variant => "gui-action")];
    #[cfg(feature = "gui")]
    "gui-action-scroll-to" [Base] => [field!("tag", Variant => "gui-action"), field!("offset", Named => "gui-input-vector")];
    #[cfg(feature = "gui")]
    "gui-action-scroll-by" [Base] => [field!("tag", Variant => "gui-action"), field!("delta", Named => "gui-input-vector")];
    #[cfg(feature = "gui")]
    "gui-action-scroll-to-index" [Base] => [field!("tag", Variant => "gui-action"), field!("index", U32), field!("offset", FiniteF32)];
    "command-remove" [Base] => [
        field!("tag", Variant => "command"),
        field!("entity", Union => "reference"),
        field!("component", U16),
    ];

    "reference-handle" [Base] => [
        field!("tag", Variant => "reference"),
        field!("handle", U64),
    ];
    "reference-alias" [Base] => [
        field!("tag", Variant => "reference"),
        field!("alias", U32),
    ];
    // A live entity named by its symbolic identifier, resolved at the command's boundary.
    "reference-symbol" [Base] => [
        field!("tag", Variant => "reference"),
        field!("symbol", Utf8, FIELD_BYTES),
    ];

    "response-batch-aborted" [Base] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("batch_id", U64),
        field!("message", Utf8, crate::MAX_FAILURE_MESSAGE_BYTES as u32),
    ];
    "response-batch" [Base] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("outcome", Union => "outcome"),
        field!("effects", List, crate::BATCH_OUTCOME_EFFECTS as u32 => "applied-operation-effect"),
    ];
    // One applied effect at its zero-based operation index, in core order.
    "applied-operation-effect" [Base] => [
        field!("operation", U32),
        field!("effect", Union => "operation-effect"),
    ];
    "attachment-effect" [Base] => [
        field!("tag", Variant => "operation-effect"),
        field!("receipt", U64),
        field!("parent", Named => "world-reference"),
        field!("anchor", U64),
        field!("incarnation", U64),
        field!("revision", U64),
        field!("child", Option => "world-reference"),
    ];
    // An adopting create or insert found its entity or component already present.
    "operation-effect-adopted" [Base] => [field!("tag", Variant => "operation-effect")];
    "command-detach-attachment-receipt" [Base] => [
        field!("tag", Variant => "command"),
        field!("receipt", U64),
    ];
    "request-attachment-receipt" [Base] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("receipt", U64),
        field!("release", Bool),
    ];
    "response-attachment-receipt" [Base] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("receipt", U64),
        field!("state", Variant => "attachment-receipt-state"),
    ];
    "response-runtime-failure" [Base] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("scope", Variant => "runtime-failure-scope"),
        field!("faulted", Bool),
        field!("message", Utf8, crate::MAX_FAILURE_MESSAGE_BYTES as u32),
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
        field!("entities", List, PAGE_RECORDS => "entity"),
        field!("resources", List, PAGE_RECORDS => "resource"),
        field!("render_diagnostics", List, PAGE_RECORDS => "render-diagnostic"),
        field!("controllers", List, PAGE_RECORDS => "animation-controller"),
        #[cfg(feature = "gui")]
        field!("gui_focus", List, PAGE_RECORDS => "gui-focus"),
        #[cfg(feature = "gui")]
        field!("gui_pointers", List, PAGE_RECORDS => "gui-pointer"),
        #[cfg(feature = "surfaces")]
        field!("canvas", Option => "canvas-state-record"),
    ];
    "response-entity-tree" [Base] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("time", NonnegativeFiniteF64),
        field!("next", U64),
        field!("nodes", List, PAGE_RECORDS => "entity-tree-node"),
    ];
    "entity-tree-node" [Base] => [
        field!("id", U64),
        field!("parent", U64),
        field!("order_low", U64),
        field!("order_high", U64),
        field!("depth", U16),
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
        field!("source", Utf8, FIELD_BYTES),
        field!("variant", U32),
        field!("track", U32),
        field!("target", U64),
        field!("property", Named => "animation-property"),
        field!("entity_bindings", List, MAX_ANIMATION_ENTITY_BINDINGS => "u64"),
        field!("weight", FiniteF32),
        field!("additive", Bool),
        field!("reference_time", FiniteF32),
        field!("repeat", Bool),
    ];
    "animation-property" [Animation] => [
        field!("target", Union => "animation-target"),
    ];
    "animation-target-entity-link" [Animation] => [field!("kind", Variant => "animation-target")];
    "animation-target-property" [Animation] => [field!("kind", Variant => "animation-target"), field!("component", U16), field!("indices", List, crate::MAX_ANIMATION_TARGET_INDICES as u32 => "animation-index")];
    "animation-target-dynamic" [Animation] => [field!("kind", Variant => "animation-target"), field!("component", U16), field!("name", Utf8, FIELD_BYTES)];
    #[cfg(feature = "skeletal-animation")]
    "animation-target-joints" [Animation] => [field!("kind", Variant => "animation-target"), field!("indices", List, crate::MAX_ANIMATION_TARGET_INDICES as u32 => "animation-index")];
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
        field!("reason", Utf8, FIELD_BYTES),
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
        field!("events", List, crate::MAX_PLAYBACK_EVENTS as u32 => "playback-event"),
    ];
    "response-resources" [Assets] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("resources", List, crate::MAX_RESOURCE_EVENT_RECORDS as u32 => "resource"),
    ];
    "response-error" [Base] => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("code", U16),
        field!("message", Utf8, FIELD_BYTES),
    ];

    "outcome-success" [Base] => [
        field!("batch_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "outcome"),
        field!("aliases", List, crate::BATCH_OUTCOME_ALIASES as u32 => "alias-handle"),
        field!("symbols", List, crate::BATCH_OUTCOME_ALIASES as u32 => "symbol-handle"),
    ];
    "outcome-failure" [Base] => [
        field!("batch_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "outcome"),
        field!("scope", Variant => "batch-error-scope"),
        field!("operation", Option => "u32"),
        field!("reason", Utf8, FIELD_BYTES),
        field!("aliases", List, crate::BATCH_OUTCOME_ALIASES as u32 => "alias-handle"),
        field!("symbols", List, crate::BATCH_OUTCOME_ALIASES as u32 => "symbol-handle"),
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
    #[cfg(feature = "gui")]
    GuiPhysicalRequest [Base] GUI_PHYSICAL_REQUEST_OPEN = 0 => "gui-physical-open";
    #[cfg(feature = "gui")]
    GuiPhysicalRequest [Base] GUI_PHYSICAL_REQUEST_CLOSE = 1 => "gui-physical-close";
    #[cfg(feature = "gui")]
    GuiPhysicalRequest [Base] GUI_PHYSICAL_REQUEST_EVENT = 2 => "gui-physical-event";
    #[cfg(feature = "gui")]
    GuiPhysicalRequest [Base] GUI_PHYSICAL_REQUEST_TEXT = 3 => "gui-physical-text";
    #[cfg(feature = "gui")]
    GuiPhysicalEvent [Base] GUI_PHYSICAL_EVENT_POINTER_DOWN = 0 => "gui-physical-pointer-down";
    #[cfg(feature = "gui")]
    GuiPhysicalEvent [Base] GUI_PHYSICAL_EVENT_POINTER_MOVE = 1 => "gui-physical-pointer-move";
    #[cfg(feature = "gui")]
    GuiPhysicalEvent [Base] GUI_PHYSICAL_EVENT_POINTER_UP = 2 => "gui-physical-pointer-up";
    #[cfg(feature = "gui")]
    GuiPhysicalEvent [Base] GUI_PHYSICAL_EVENT_POINTER_CANCEL = 3 => "gui-physical-pointer-cancel";
    #[cfg(feature = "gui")]
    GuiPhysicalEvent [Base] GUI_PHYSICAL_EVENT_WHEEL = 4 => "gui-physical-wheel";
    #[cfg(feature = "gui")]
    GuiPhysicalEvent [Base] GUI_PHYSICAL_EVENT_KEY = 5 => "gui-physical-key";
    #[cfg(feature = "gui")]
    GuiPhysicalEvent [Base] GUI_PHYSICAL_EVENT_BLUR = 6 => "gui-physical-blur";
    #[cfg(feature = "gui")]
    GuiPhysicalResponse [Base] GUI_PHYSICAL_RESPONSE_OPENED = 0 => "gui-physical-opened";
    #[cfg(feature = "gui")]
    GuiPhysicalResponse [Base] GUI_PHYSICAL_RESPONSE_CLOSED = 1 => "gui-physical-closed";
    #[cfg(feature = "gui")]
    GuiPhysicalResponse [Base] GUI_PHYSICAL_RESPONSE_ROUTED = 2 => "gui-physical-routed";
    #[cfg(feature = "gui")]
    GuiPhysicalResponse [Base] GUI_PHYSICAL_RESPONSE_REJECTED = 3 => "gui-physical-rejected";
    #[cfg(feature = "gui")]
    GuiPhysicalResponse [Base] GUI_PHYSICAL_RESPONSE_REVOKED = 4 => "gui-physical-revoked";
    #[cfg(feature = "gui")]
    GuiPhysicalResponse [Base] GUI_PHYSICAL_RESPONSE_CANCELLED = 5 => "gui-physical-cancelled";
    #[cfg(feature = "gui")]
    GuiPhysicalResponse [Base] GUI_PHYSICAL_RESPONSE_NATIVE = 6 => "gui-physical-native";
    #[cfg(feature = "gui")]
    GuiPhysicalButton [Base] GUI_PHYSICAL_BUTTON_PRIMARY = 0 => "gui-physical-button-primary";
    #[cfg(feature = "gui")]
    GuiPhysicalButton [Base] GUI_PHYSICAL_BUTTON_SECONDARY = 1 => "gui-physical-button-secondary";
    #[cfg(feature = "gui")]
    GuiPhysicalButton [Base] GUI_PHYSICAL_BUTTON_AUXILIARY = 2 => "gui-physical-button-auxiliary";
    #[cfg(feature = "gui")]
    GuiPhysicalKey [Base] GUI_PHYSICAL_KEY_TAB = 0 => "gui-physical-key-tab";
    #[cfg(feature = "gui")]
    GuiPhysicalKey [Base] GUI_PHYSICAL_KEY_BACK_TAB = 1 => "gui-physical-key-back-tab";
    #[cfg(feature = "gui")]
    GuiPhysicalKey [Base] GUI_PHYSICAL_KEY_ENTER = 2 => "gui-physical-key-enter";
    #[cfg(feature = "gui")]
    GuiPhysicalKey [Base] GUI_PHYSICAL_KEY_SPACE = 3 => "gui-physical-key-space";
    #[cfg(feature = "gui")]
    GuiPhysicalKey [Base] GUI_PHYSICAL_KEY_ESCAPE = 4 => "gui-physical-key-escape";
    #[cfg(feature = "gui")]
    GuiPhysicalKey [Base] GUI_PHYSICAL_KEY_LEFT = 5 => "gui-physical-key-left";
    #[cfg(feature = "gui")]
    GuiPhysicalKey [Base] GUI_PHYSICAL_KEY_RIGHT = 6 => "gui-physical-key-right";
    #[cfg(feature = "gui")]
    GuiPhysicalKey [Base] GUI_PHYSICAL_KEY_UP = 7 => "gui-physical-key-up";
    #[cfg(feature = "gui")]
    GuiPhysicalKey [Base] GUI_PHYSICAL_KEY_DOWN = 8 => "gui-physical-key-down";
    #[cfg(feature = "gui")]
    GuiPhysicalKey [Base] GUI_PHYSICAL_KEY_HOME = 9 => "gui-physical-key-home";
    #[cfg(feature = "gui")]
    GuiPhysicalKey [Base] GUI_PHYSICAL_KEY_END = 10 => "gui-physical-key-end";
    #[cfg(feature = "gui")]
    GuiNativeEdit [Base] GUI_NATIVE_EDIT_INSERT = 0 => "gui-native-insert";
    #[cfg(feature = "gui")]
    GuiNativeEdit [Base] GUI_NATIVE_EDIT_SELECTION = 1 => "gui-native-selection";
    #[cfg(feature = "gui")]
    GuiNativeEdit [Base] GUI_NATIVE_EDIT_COMPOSE = 2 => "gui-native-compose";
    #[cfg(feature = "gui")]
    GuiNativeEdit [Base] GUI_NATIVE_EDIT_COMMIT_COMPOSITION = 3 => "gui-native-commit-composition";
    #[cfg(feature = "gui")]
    GuiNativeEdit [Base] GUI_NATIVE_EDIT_CANCEL_COMPOSITION = 4 => "gui-native-cancel-composition";
    #[cfg(feature = "gui")]
    GuiNativeEdit [Base] GUI_NATIVE_EDIT_BACKSPACE = 5 => "gui-native-backspace";
    #[cfg(feature = "gui")]
    GuiNativeEdit [Base] GUI_NATIVE_EDIT_DELETE = 6 => "gui-native-delete";
    #[cfg(feature = "gui")]
    GuiNativeEdit [Base] GUI_NATIVE_EDIT_LEFT = 7 => "gui-native-left";
    #[cfg(feature = "gui")]
    GuiNativeEdit [Base] GUI_NATIVE_EDIT_RIGHT = 8 => "gui-native-right";
    #[cfg(feature = "gui")]
    GuiNativeEdit [Base] GUI_NATIVE_EDIT_HOME = 9 => "gui-native-home";
    #[cfg(feature = "gui")]
    GuiNativeEdit [Base] GUI_NATIVE_EDIT_END = 10 => "gui-native-end";
    #[cfg(feature = "gui")]
    GuiNativeEdit [Base] GUI_NATIVE_EDIT_SELECT_ALL = 11 => "gui-native-select-all";
    #[cfg(feature = "gui")]
    GuiNativeEdit [Base] GUI_NATIVE_EDIT_SUBMIT = 12 => "gui-native-submit";
    #[cfg(feature = "gui")]
    GuiPhysicalDisposition [Base] GUI_PHYSICAL_DISPOSITION_ROUTED = 0 => "gui-physical-disposition-routed";
    #[cfg(feature = "gui")]
    GuiPhysicalDisposition [Base] GUI_PHYSICAL_DISPOSITION_MISS = 1 => "gui-physical-disposition-miss";
    #[cfg(feature = "gui")]
    GuiPhysicalDisposition [Base] GUI_PHYSICAL_DISPOSITION_BLOCKED = 2 => "gui-physical-disposition-blocked";
    #[cfg(feature = "gui")]
    GuiPhysicalDisposition [Base] GUI_PHYSICAL_DISPOSITION_UNHANDLED = 3 => "gui-physical-disposition-unhandled";
    Request [Base] REQUEST_LIFECYCLE_WATCH = 35 => "request-lifecycle-watch";
    Response [Base] RESPONSE_LIFECYCLE_WATCH = 37 => "response-lifecycle-watch";
    #[cfg(feature = "diagnostics")]
    Request [Base] REQUEST_LIFECYCLE_DIAGNOSTICS = 36 => "request-lifecycle-diagnostics";
    #[cfg(feature = "diagnostics")]
    Response [Base] RESPONSE_LIFECYCLE_DIAGNOSTICS = 38 => "response-lifecycle-diagnostics";
    LifecycleWatchChange [Base] LIFECYCLE_WATCH_ADD = 0 => "lifecycle-watch-add";
    LifecycleWatchChange [Base] LIFECYCLE_WATCH_REMOVE = 1 => "lifecycle-watch-remove";
    LifecycleWatchTarget [Base] LIFECYCLE_WATCH_ENTITY = 0 => "lifecycle-watch-entity";
    LifecycleWatchTarget [Base] LIFECYCLE_WATCH_COMPONENT = 1 => "lifecycle-watch-component";
    LifecycleWatchTarget [Base] LIFECYCLE_WATCH_VALUE = 2 => "lifecycle-watch-value";
    LifecycleWatchRecord [Base] LIFECYCLE_WATCH_ACK = 0 => "lifecycle-watch-ack";
    LifecycleWatchRecord [Base] LIFECYCLE_WATCH_EVENT = 1 => "lifecycle-watch-event";
    LifecycleWatchRecord [Base] LIFECYCLE_WATCH_VALUE_RECORD = 3 => "lifecycle-watch-value-record";
    LifecycleMembershipResult [Base] LIFECYCLE_MEMBERSHIP_APPLIED = 0 => "lifecycle-watch-applied";
    LifecycleMembershipResult [Base] LIFECYCLE_MEMBERSHIP_REJECTED = 1 => "lifecycle-watch-rejected";
    LifecycleMembershipResult [Base] LIFECYCLE_MEMBERSHIP_CANCELLED = 2 => "lifecycle-watch-cancelled";
    LifecycleTargetLifetime [Base] LIFECYCLE_LIFETIME_ENTITY = 0 => "lifecycle-lifetime-entity";
    LifecycleTargetLifetime [Base] LIFECYCLE_LIFETIME_COMPONENT = 1 => "lifecycle-lifetime-component";
    LifecycleTargetLifetime [Base] LIFECYCLE_LIFETIME_REMOVED = 2 => "lifecycle-lifetime-removed";
    LifecycleMembershipRejection [Base] LIFECYCLE_MEMBERSHIP_STALE_WORLD = 0 => "empty";
    LifecycleMembershipRejection [Base] LIFECYCLE_MEMBERSHIP_STALE_SESSION = 1 => "empty";
    LifecycleMembershipRejection [Base] LIFECYCLE_MEMBERSHIP_STALE_MEMBER = 2 => "empty";
    LifecycleMembershipRejection [Base] LIFECYCLE_MEMBERSHIP_ALREADY_ACTIVE = 3 => "empty";
    LifecycleMembershipRejection [Base] LIFECYCLE_MEMBERSHIP_TRACKING_ENDED = 4 => "empty";
    LifecycleMembershipRejection [Base] LIFECYCLE_MEMBERSHIP_CAPACITY = 5 => "empty";
    LifecycleWatchKinds [Base] LIFECYCLE_WATCH_ENTITY_CREATED = 1 => "empty";
    LifecycleWatchKinds [Base] LIFECYCLE_WATCH_ENTITY_METADATA_CHANGED = 2 => "empty";
    LifecycleWatchKinds [Base] LIFECYCLE_WATCH_ENTITY_DELETED = 4 => "empty";
    LifecycleWatchKinds [Base] LIFECYCLE_WATCH_COMPONENT_INSERTED = 8 => "empty";
    LifecycleWatchKinds [Base] LIFECYCLE_WATCH_COMPONENT_UPDATED = 16 => "empty";
    LifecycleWatchKinds [Base] LIFECYCLE_WATCH_COMPONENT_REPLACED = 32 => "empty";
    LifecycleWatchKinds [Base] LIFECYCLE_WATCH_COMPONENT_REMOVED = 64 => "empty";
    LifecycleWatchKinds [Base] LIFECYCLE_WATCH_VALUE_CHANGED = 128 => "empty";
    // Request and response tag 26 carried Surface item edits; requests 31 and
    // 32 (GUI action, now a command) and responses 28 (GUI terminal) and 33 are
    // also retired. Retired tags are never reused.
    #[cfg(feature = "gui")]
    Request [Base] REQUEST_GUI_OBSERVATION = 34 => "request-gui-observation";
    #[cfg(feature = "gui")]
    Response [Base] RESPONSE_GUI_OBSERVATION = 36 => "response-gui-observation";
    HostRequest [Base] HOST_REQUEST_RESOLVE_WORLD = 14 => "host-request-resolve-world";
    PresentationRequest [Base] PRESENTATION_REQUEST_SURFACE = 1 => "presentation-request-surface";
    PresentationRequest [Base] PRESENTATION_REQUEST_SELECT = 2 => "presentation-request-select";
    PresentationRequest [Base] PRESENTATION_REQUEST_CLEAR = 3 => "presentation-request-clear";
    PresentationRequest [Base] PRESENTATION_REQUEST_FRAME = 4 => "presentation-request-frame";
    PresentationRequest [Base] PRESENTATION_REQUEST_READ_CAPTURE = 5 => "presentation-request-read-capture";
    PresentationRequest [Base] PRESENTATION_REQUEST_RELEASE_CAPTURE = 6 => "presentation-request-release-capture";
    PresentationRequest [Base] PRESENTATION_REQUEST_CANCEL_FRAME = 7 => "presentation-request-cancel-frame";
    PresentationResponse [Base] PRESENTATION_RESPONSE_SURFACE = 1 => "presentation-response-surface";
    PresentationResponse [Base] PRESENTATION_RESPONSE_VIEW = 2 => "presentation-response-view";
    PresentationResponse [Base] PRESENTATION_RESPONSE_FRAME = 3 => "presentation-response-frame";
    PresentationResponse [Base] PRESENTATION_RESPONSE_CAPTURE = 4 => "presentation-response-capture";
    PresentationResponse [Base] PRESENTATION_RESPONSE_CHUNK = 5 => "presentation-response-chunk";
    PresentationResponse [Base] PRESENTATION_RESPONSE_COMPLETE = 6 => "presentation-response-complete";
    PresentationResponse [Base] PRESENTATION_RESPONSE_ERROR = 7 => "presentation-response-error";
    PresentationError [Base] PRESENTATION_ERROR_UNSUPPORTED = 1 => "presentation-error-unsupported";
    PresentationError [Base] PRESENTATION_ERROR_UNAVAILABLE = 2 => "presentation-error-unavailable";
    PresentationError [Base] PRESENTATION_ERROR_STALE_VIEW = 3 => "presentation-error-stale-view";
    PresentationError [Base] PRESENTATION_ERROR_INVALID_VIEWPORT = 4 => "presentation-error-invalid-viewport";
    PresentationError [Base] PRESENTATION_ERROR_OBSOLETE_PUBLICATION = 5 => "presentation-error-obsolete-publication";
    PresentationError [Base] PRESENTATION_ERROR_CAPACITY = 6 => "presentation-error-capacity";
    PresentationError [Base] PRESENTATION_ERROR_TIMEOUT = 7 => "presentation-error-timeout";
    PresentationError [Base] PRESENTATION_ERROR_DRAW_FAILED = 8 => "presentation-error-draw-failed";
    HostRequest [Base] HOST_REQUEST_PRESENTATION = 23 => "host-request-presentation";
    HostRequest [Base] HOST_REQUEST_GET_ROOT_OUTPUT_BINDING = 24 => "host-request-get-root-output-binding";
    #[cfg(feature = "gui")]
    HostRequest [Base] HOST_REQUEST_GUI_INPUT = 25 => "host-request-gui-input";
    #[cfg(feature = "gui")]
    HostResponse [Base] HOST_RESPONSE_GUI_INPUT = 19 => "host-response-gui-input";
    HostResponse [Base] HOST_RESPONSE_PRESENTATION = 17 => "host-response-presentation";
    HostResponse [Base] HOST_RESPONSE_ROOT_BINDING = 18 => "host-response-root-binding";
    HostRequest [Base] HOST_REQUEST_BIND_OUTPUT = 15 => "host-request-bind-output";
    HostRequest [Base] HOST_REQUEST_RESOLVE_OUTPUT = 16 => "host-request-resolve-output";
    HostRequest [Base] HOST_REQUEST_SET_ROOT_OUTPUT = 17 => "host-request-set-root-output";
    HostRequest [Base] HOST_REQUEST_CLEAR_ROOT_OUTPUT = 18 => "host-request-clear-root-output";
    HostResponse [Base] HOST_RESPONSE_CREATED = 12 => "host-response-created";
    HostResponse [Base] HOST_RESPONSE_WORLD_REFERENCE = 10 => "host-response-world-reference";
    HostResponse [Base] HOST_RESPONSE_OUTPUT_REFERENCE = 11 => "host-response-output-reference";
    HostRequest [Base] HOST_REQUEST_LIST_WORLDS = 1 => "host-request-list-worlds";
    HostRequest [Base] HOST_REQUEST_CREATE_WORLD = 2 => "host-request-create-world";
    HostRequest [Base] HOST_REQUEST_OPEN_WORLD = 3 => "host-request-open-world";
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
    HostRequest [Base] HOST_REQUEST_INSPECT_WORLD_LOAD = 19 => "host-request-inspect-world-load";
    HostRequest [Base] HOST_REQUEST_SET_WORLD_LOAD_NAMES = 20 => "host-request-set-world-load-names";
    HostRequest [Base] HOST_REQUEST_READ_WORLD_LOAD_BINDINGS = 21 => "host-request-read-world-load-bindings";
    HostRequest [Base] HOST_REQUEST_ACKNOWLEDGE_WORLD_LOAD = 22 => "host-request-acknowledge-world-load";
    HostResponse [Base] HOST_RESPONSE_WORLD_GRAPH_PAGE = 14 => "host-response-world-graph-page";
    HostResponse [Base] HOST_RESPONSE_WORLD_GRAPH_LOADED = 15 => "host-response-world-graph-loaded";
    HostResponse [Base] HOST_RESPONSE_WORLD_GRAPH_BINDINGS = 16 => "host-response-world-graph-bindings";
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
    AnimationTarget [Animation] ANIMATION_TARGET_PROPERTY = 0 => "animation-target-property";
    #[cfg(feature = "skeletal-animation")]
    AnimationTarget [Animation] ANIMATION_TARGET_JOINTS = 1 => "animation-target-joints";
    AnimationTarget [Animation] ANIMATION_TARGET_DYNAMIC = 2 => "animation-target-dynamic";
    AnimationTarget [Animation] ANIMATION_TARGET_ENTITY_LINK = 3 => "animation-target-entity-link";
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
    InspectionCollection [Base] INSPECT_ENTITY_TREE = 5 => "empty";
    #[cfg(feature = "gui")]
    InspectionCollection [Base] INSPECT_GUI_FOCUS = 6 => "empty";
    #[cfg(feature = "gui")]
    InspectionCollection [Base] INSPECT_GUI_POINTERS = 7 => "empty";
    #[cfg(feature = "surfaces")]
    InspectionCollection [Base] INSPECT_CANVAS = 8 => "empty";
    Request [Base] REQUEST_LIFECYCLE_SUBSCRIBE = 19 => "request-lifecycle-subscribe";
    Request [Base] REQUEST_LIFECYCLE_UNSUBSCRIBE = 20 => "request-lifecycle-unsubscribe";
    Response [Base] RESPONSE_LIFECYCLE_SUBSCRIPTION = 16 => "response-lifecycle-subscription";
    Response [Base] RESPONSE_LIFECYCLE_EVENTS = 17 => "response-lifecycle-events";
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
    #[cfg(feature = "surfaces")]
    Request [Base] REQUEST_CANVAS_STATE_UPDATE = 41 => "request-canvas-state-update";
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
    // Snapshot value tag 14 referred to a base descriptor table; retired tags are never reused.
    Value [Base] VALUE_ROWS = ipp_core::components::schema::FieldKind::Rows as u8 => "value-rows";
    Value [Base] VALUE_WORLD = ipp_core::components::schema::FieldKind::World as u8 => "value-world";
    OutputKind [Base] OUTPUT_CANVAS = 0 => "output-canvas";
    OutputKind [Base] OUTPUT_CAMERA = 1 => "output-camera";
    OutputTarget [Base] OUTPUT_TARGET_CANVAS = 0 => "output-target-canvas";
    OutputTarget [Base] OUTPUT_TARGET_CAMERA = 1 => "output-target-camera";
    Value [Base] VALUE_OUTPUT = ipp_core::components::schema::FieldKind::Output as u8 => "value-output";
    SnapshotValue [Base] SNAPSHOT_VALUE_WORLD = ipp_core::components::schema::FieldKind::World as u8 => "snapshot-value-world";
    SnapshotValue [Base] SNAPSHOT_VALUE_OUTPUT = ipp_core::components::schema::FieldKind::Output as u8 => "snapshot-value-output";
    SnapshotValue [Base] SNAPSHOT_VALUE_ROWS = ipp_core::components::schema::FieldKind::Rows as u8 => "snapshot-value-rows";
    Value [Base] VALUE_UNSET = ipp_core::components::schema::FieldKind::Unset as u8 => "value-unset";
    SnapshotValue [Base] SNAPSHOT_VALUE_UNSET = ipp_core::components::schema::FieldKind::Unset as u8 => "snapshot-value-unset";
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

    Response [Base] RESPONSE_BATCH_ABORTED = 24 => "response-batch-aborted";
    Request [Base] REQUEST_SUBMIT_BATCH = 1 => "request-submit-batch";
    Request [Base] REQUEST_INSPECT = 3 => "request-inspect";
    Request [Base] REQUEST_ATTACHMENT_RECEIPT = 33 => "request-attachment-receipt";
    Response [Base] RESPONSE_ATTACHMENT_RECEIPT = 35 => "response-attachment-receipt";
    OperationEffect [Base] ATTACHMENT_WRITTEN = 0 => "attachment-effect";
    OperationEffect [Base] ATTACHMENT_DETACHED = 1 => "attachment-effect";
    OperationEffect [Base] ATTACHMENT_SUPERSEDED = 2 => "attachment-effect";
    OperationEffect [Base] OPERATION_ADOPTED = 3 => "operation-effect-adopted";
    AttachmentReceiptState [Base] RECEIPT_PENDING = 0 => "empty";
    AttachmentReceiptState [Base] RECEIPT_RETIRED = 1 => "empty";
    AttachmentReceiptState [Base] RECEIPT_RELEASED = 2 => "empty";
    Request [Picking] REQUEST_GEOMETRY_PICK = 9 => "request-geometry-pick";
    Request [Picking] REQUEST_CAMERA_PROJECT = 12 => "request-camera-project";
    Request [Picking] REQUEST_CAMERA_NAVIGATE = 40 => "request-camera-navigate";
    Response [Picking] RESPONSE_CAMERA_NAVIGATED = 40 => "response-camera-navigated";

    Command [Base] COMMAND_CREATE = 1 => "command-create";
    Command [Base] COMMAND_DELETE = 2 => "command-delete";
    Command [Base] COMMAND_PLACE_ENTITY = 17 => "command-place-entity";
    Command [Base] COMMAND_DELETE_SUBTREE = 18 => "command-delete-subtree";
    Command [Base] COMMAND_DETACH_ATTACHMENT_RECEIPT = 22 => "command-detach-attachment-receipt";
    Command [Base] COMMAND_METADATA = 3 => "command-metadata";
    Command [Base] COMMAND_INSERT = 4 => "command-insert";
    Command [Base] COMMAND_SET = 5 => "command-set";
    Command [Base] COMMAND_SET_FIELD_IF = 23 => "command-set-field-if";
    Command [Base] COMMAND_SET_DYNAMIC_PROPERTY = 14 => "command-set-dynamic-property";
    Command [Base] COMMAND_REMOVE_DYNAMIC_PROPERTY = 15 => "command-remove-dynamic-property";
    Command [Base] COMMAND_REMOVE = 6 => "command-remove";
    #[cfg(feature = "gui")]
    Command [Base] COMMAND_GUI_ACTION = 24 => "command-gui-action";
    // Command tags 7-13, 16 and 19-21 are retired; retired tags are never reused.
    // GUI action 5 (explicit value replacement) is retired and never reused.
    #[cfg(feature = "gui")]
    GuiAction [Base] GUI_ACTION_PRESS = 0 => "gui-action-press";
    #[cfg(feature = "gui")]
    GuiAction [Base] GUI_ACTION_TOGGLE = 1 => "gui-action-toggle";
    #[cfg(feature = "gui")]
    GuiAction [Base] GUI_ACTION_SET_SCALAR = 2 => "gui-action-set-scalar";
    #[cfg(feature = "gui")]
    GuiAction [Base] GUI_ACTION_SET_TEXT = 3 => "gui-action-set-text";
    #[cfg(feature = "gui")]
    GuiAction [Base] GUI_ACTION_FOCUS = 4 => "gui-action-focus";
    #[cfg(feature = "gui")]
    GuiAction [Base] GUI_ACTION_BLUR = 6 => "gui-action-blur";
    #[cfg(feature = "gui")]
    GuiAction [Base] GUI_ACTION_SUBMIT = 7 => "gui-action-submit";
    #[cfg(feature = "gui")]
    GuiAction [Base] GUI_ACTION_SCROLL_TO = 8 => "gui-action-scroll-to";
    #[cfg(feature = "gui")]
    GuiAction [Base] GUI_ACTION_SCROLL_BY = 9 => "gui-action-scroll-by";
    #[cfg(feature = "gui")]
    GuiAction [Base] GUI_ACTION_SCROLL_TO_INDEX = 10 => "gui-action-scroll-to-index";

    Reference [Base] REF_HANDLE = 0 => "reference-handle";
    Reference [Base] REF_ALIAS = 1 => "reference-alias";
    // Reference tag 2 is retired; retired tags are never reused.
    Reference [Base] REF_SYMBOL = 3 => "reference-symbol";

    Response [Base] RESPONSE_BATCH = 1 => "response-batch";
    Response [Base] RESPONSE_INSPECT = 3 => "response-inspect";
    Response [Base] RESPONSE_ENTITY_TREE = 34 => "response-entity-tree";
    Response [Base] RESPONSE_FRAME = 4 => "response-frame";
    // Response tag 5 is retired; retired tags are never reused.
    Response [Assets] RESPONSE_RESOURCES = 9 => "response-resources";
    Response [Picking] RESPONSE_GEOMETRY_PICK = 11 => "response-geometry-pick";
    Response [Picking] RESPONSE_CAMERA_PROJECT = 13 => "response-camera-project";
    Response [Base] RESPONSE_RUNTIME_FAILURE = 22 => "response-runtime-failure";
    RuntimeFailureScope [Base] FAILURE_DRAW = crate::RuntimeFailureScope::Draw as u8 => "empty";
    RuntimeFailureScope [Base] FAILURE_RESOURCE = crate::RuntimeFailureScope::Resource as u8 => "empty";
    RuntimeFailureScope [Base] FAILURE_CONTEXT = crate::RuntimeFailureScope::Context as u8 => "empty";
    RuntimeFailureScope [Base] FAILURE_WORLD = crate::RuntimeFailureScope::World as u8 => "empty";
    Response [Base] RESPONSE_ERROR = 255 => "response-error";

    GeometryPickOutcome [Picking] PICK_OUTCOME_MISS = 0 => "pick-result-miss";
    GeometryPickOutcome [Picking] PICK_OUTCOME_HIT = 1 => "pick-result-hit";
    GeometryPickOutcome [Picking] PICK_OUTCOME_FAILURE = 3 => "pick-result-failure";
    ViewTarget [Picking] VIEW_ROOT = 0 => "view-root";
    ViewTarget [Picking] VIEW_PUBLICATION = 1 => "view-publication";
    ViewTarget [Picking] VIEW_BOUND = 2 => "view-bound";

    Outcome [Base] OUTCOME_SUCCESS = 0 => "outcome-success";
    Outcome [Base] OUTCOME_FAILURE = 1 => "outcome-failure";

    BatchErrorScope [Base] BATCH_ERROR_OPERATION = 0 => "empty";
    BatchErrorScope [Base] BATCH_ERROR_COMMIT = 1 => "empty";
    Option [Base] OPTION_NONE = 0 => "empty";
    Option [Base] OPTION_SOME = 1 => "present";
    AssetResourceStatus [Assets] RESOURCE_UNLOADED = 0 => "resource-status-unloaded";
    AssetResourceStatus [Assets] RESOURCE_START = 1 => "resource-status-start";
    AssetResourceStatus [Assets] RESOURCE_PROGRESS = 2 => "resource-status-progress";
    AssetResourceStatus [Assets] RESOURCE_LOADED = 3 => "resource-status-loaded";
    AssetResourceStatus [Assets] RESOURCE_FAILED = 4 => "resource-status-failed";
}

pub(crate) const CAPABILITIES: &[(Capability, bool)] = &[
    (Capability::Animation, true),
    (Capability::Assets, true),
    (Capability::Spatial, true),
    (Capability::Textures, true),
    (Capability::BuiltinAssets, cfg!(feature = "builtin-assets")),
    (Capability::Picking, true),
    (Capability::DebugGeometry, true),
];

pub(crate) const CONVENTIONS: &[(&str, &str)] = &[
    (
        "attachment-receipts",
        "session-owned-bounded-registry;effects=ordered-operation-index,exact-parent,anchor,component-incarnation,nonreused-write-revision,child;conditional-detach-resolves-at-operation;partial-prefix-retained;pending-is-not-retired;release-invalidates-handle;runtime-only;reply-and-registry-capacity-before-callbacks",
    ),
    #[cfg(feature = "gui")]
    (
        "gui-local",
        "ordinary-world-entity-component-incarnation;payloads=length-u32-bytes;per-world-session-only;query=kind-u8:entity0-u64|tree1-root?-bool-u64,after?-bool-u64,limit-u16-1..256,maxdepth-u16-0..64;action=world-u64x2,entity-u64,component-u16,incarnation-u64,revision-u32,operation-u8:press0|toggle1|scalar2-f32|text3-utf8|focus4|replace5-value|blur6;value=tag-u8:none0|bool1-u8|scalar2-f32|text3-utf8-max65536;page=world-u64x2,next?-bool-u64,count-u32-max256,row*;row=entity-u64,parent?-bool-u64,order-u128-le,depth-u16,control?-bool-(target,role-u8:button0|checkbox1|slider2|text3,revision-u32,value,label-utf8,ancestry-count-u32-u64*,enabled-bool,visible-bool,available-bool,focused-bool,hovered-bool,pressed-bool,captured-bool);terminal=tag-u8:applied0-effect|rejected1-reason|cancelled2;effect=target,source-u8:semantic0|replacement1,tick-u64,ancestry-count-u32-u64*,kind-u8:pressed0|focus-changed1-focused-bool-changed-bool|committed2-revision-u32-value;reason=u8:session0|capacity1|duplicate2|unavailable3|context6|path7|cancelled8|delivery-session9|delivery-capacity10|target11|revision12-current-u32|local-unavailable13|unsupported14|value15|revision-exhausted16;reserve-slot-rejection-and-exact-applied-bytes-before-mutation;single-terminal-no-generic-command-ack;header-tick=applied-effect-tick|no-effect-zero;terminal-not-frame-observation;no-root-required;equal-replacement-advances-revision;visited-entity-pages-not-state-replication",
    ),
    #[cfg(feature = "gui")]
    (
        "gui-scroll",
        "ordinary-control-roles=scroll-view4|virtual-list5;actions=scroll-to8-f32x2|scroll-by9-f32x2|scroll-to-index10-u32-f32;value=scroll4-offset-f32x2-anchor-index-u32-anchor-offset-f32;snapshot-trailer=scroll?-bool-(revision-u32,viewport-f32x2,content-f32x2,capacity-f32x2,item-count?-bool-u32,first-u32,last-u32);effect-source=layout3;effect=scroll-changed5-committed-revision-u32-value-scroll-snapshot;application-class;single-authoritative-position;owner-normalizes-measurements-during-layout-before-completed-paint-and-semantics;shrink-keeps-clamped-surviving-anchor-no-regrowth-intent;runtime-measurements-excluded-from-persistence",
    ),
    (
        "host-presentation",
        "root-binding=output,viewport,host-u64,generation-u64;root-config-independent-of-surface-selection-and-authoring-sessions;clear-compares-exact-generation;one-surface-one-selected-view;selection-fresh-on-equal-rebind;context-loss-fences-pending-completion;actual-successful-draw-only;no-headless-noop-or-invalid-camera-clear-success;exact-publication-constrains-current-authorized-draw-never-history-replay;viewport-exact-no-silent-clamp;capture=top-left-rgba8;immutable-completed-capture-survives-rebind-and-context-loss;capture-connection-owned-release-expiry-disconnect;frame-deadline-monotonic-host-time;no-simulation-step",
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
    ("endianness", "little"),
    ("bool", "u8;false=0;true=1;other-values-reject"),
    (
        "render-state-patch",
        "mask-u16;masked-field-limit-is-presence-bit;unknown-bits-reject;omitted-unchanged;empty-noop-no-event;linear-rgb=finite-0..1;stage0-ordered;atomic;session-default-reset",
    ),
    ("bootstrap", "magic4,version-u32,schema-hash-u64"),
    (
        "host-control",
        "revision=2;request=IPPH-2-0-0-0,connection-u64,request-u64,tag-u8;response=IPPA-2-0-0-0,connection-u64,request-u64,tag-u8;requests=1-list,2-create,3-open,4-rename,5-destroy,6-detach,7-hints,14-resolve-world,15-bind-output,16-resolve-output,17-set-root,18-clear-root;responses=1-worlds,2-attached,3-world,4-complete,5-error,6-detached,10-world-reference,11-output-reference,12-created;create=world-options,selected-system-names-option,canvas-state-option=extent-f32x2,units-per-metre-f32-finite-positive,only-with-canvas-system;output-reference=world-reference,target-u8:canvas0-world-lifetime|camera1-entity-u64,incarnation-u64;attached=descriptor,session-u64,selected-world-manifest,world-reference;world-manifest=systems-string-list,components-u16-list,operations-u8-list;operations=entity-links0,animation2,joint-animation3,constraints4,look-at5,geometry6,rendering7,camera8,surface9,gui10,particles11,canvas12;descriptor=id-u64,symbol-string,persistent-u128,hints;selector=tag-u8:0-id-u64,1-symbol-string;hints=optional-entities-u32,system-string-to-string-u32-map;connection-bootstrap=compiled-hash-only;standalone-session-bootstrap=compiled-hash-and-world-manifest;creation-independent-of-opening;opening-independent-of-root;ordered-with-world-ingress;world-list-page=32;session-fresh;temporary-explicit;world-clock-host-owned",
    ),
    (
        "world-persistence",
        "IPPW-version=6;authored-graph;graph-local-node-ids;unchanged-asset-references;no-source-reads;target-contract-required;host-requests=8-save,9-read,10-begin-load,11-write,12-finish,13-cancel,19-inspect,20-names,21-bindings,22-ack;host-responses=7-transfer,9-chunk,13-typed-busy-world,14-metadata-page,15-graph-loaded,16-binding-page;job-offset-length=u64;page-offset=u32;chunk=bytes-u32-max65536;max-file=67108864;preview-and-renames-bound-to-same-transfer;load-revalidates;private-all-or-none-publication;all-created-exact-refs-journaled-before-reply;ack-after-all-binding-pages;cancel-timeout-disconnect-destroy-only-unacknowledged-transfer-owned-worlds-without-cascade;acknowledged-worlds-outlive-connection",
    ),
    (
        "request-id",
        "nonzero-rpc-and-query;zero-command-and-unsolicited-event;commands-no-reply",
    ),
    (
        "system-state-events",
        "zero-id;sparse-committed-values;no-success-or-failure;no-command-correlation;nonempty-mask",
    ),
    ("length-prefix", "u32"),
    ("utf8", "strict"),
    ("max-message-bytes", "1048576"),
    ("command-page-bytes", "262144"),
    ("batch-outcome-aliases", "32768"),
    ("lifecycle-preferred-page-members", "2728"),
    (
        "lifecycle-watch",
        "exact-world-session;single-owned-ack;apply-cut-not-frame;outer-tick-zero;sorted-unique-member-generations;no-history;connection-byte-budget;kind-bits-or-within-target-domain;baseline-absence-zero;strict-acquisition-not-baseline;value-target=schema-fields-ascending-max64,kinds=value-changed128,baseline=component-lifetime;value-record=frame-end-tick,current-first-then-changed,absent-none,superseded-until-delivered",
    ),
    ("unsupported", "hierarchy"),
    (
        "playback",
        "control-u32:play=0,pause=1,stop=2,seek=3,restart=4;time-f64:nonnegative;nonseek-time=0;state-u32:stopped=0,playing=1,paused=2,completed=3;event-u32:started=0,paused=1,stopped=2,completed=3,invalidated=4,failed=5;reason-empty=none;stage0-ordered;world-owned-controller;multi-entity-drivers;seek-no-advance;stop-withdraws;completion-holds;loops-no-completion",
    ),
    (
        "geometry-query",
        "normalized-top-left;nonzero-viewport;RootView=current-root-exact-output-and-viewport;PublicationView=available-retained-CPU-history-without-presentation-or-input-authority;completed-camera-and-geometry;World-qualified-hits-and-publication-identities;camera-clipping;world-distance;entity-then-part-ties;no-live-active-camera-fallback",
    ),
    (
        "dynamic-property-values",
        "f32:1,i32:2,u32:3,bool:4,vec2:5,vec3:6,vec4:7,mat2:8,mat3:9,mat4:10,asset:12;asset=type-u16,variant-u32,source-utf8;retired-tag11-rejected;asset-type-is-value;consumer-validates-requirements",
    ),
];

/// Numeric bounds generated clients enforce that no wire layout field carries, each
/// read from its Rust constant so generated codecs never repeat the value.
pub(crate) const LIMITS: &[(&str, u32)] = &[
    ("entity-tree-depth", crate::MAX_ENTITY_TREE_DEPTH as u32),
    ("mesh-vertices", ipp_core::MAX_MESH_VERTICES),
    (
        "skeleton-joints",
        ipp_core::services::asset_management::MAX_JOINTS as u32,
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
        format: "IPPA;version-u32=4;track=target-u8:property0|dynamic2|entity-link3;entity-link-key-kind10=parent-slot-u32,before-slot-u32;none-slot=4294967295;slots-resolve-per-driver;dynamic=component-u16,name-utf8;dynamic-value-kind11=length-u32,typed-value;duration-f64;tracks-u32>=1;property=component-u16,offset-count-u8:1|4,offsets-u32;key-count-u32;key=time-f64,value,curve-u8;value=field-kind-u8,payload;quaternion-kind=8:xyzw-f32;entity=u64;owned=length-u32,bytes;curve=step:0,linear:1,bezier:2;bezier=time1-f64,value1,time2-f64,value2;monotone-times;last-key-step;exact-payload",
    },
    #[cfg(feature = "skeletal-animation")]
    AssetFormat {
        name: "ASSET_ANIMATION",
        capability: Capability::Animation,
        type_id: ipp_core::systems::animation::ANIMATION_TYPE.0,
        format: "IPPA;version-u32=4;track=target-u8:property0|joints1|dynamic2|entity-link3;entity-link-key-kind10=parent-slot-u32,before-slot-u32;none-slot=4294967295;slots-resolve-per-driver;dynamic=component-u16,name-utf8;dynamic-value-kind11=length-u32,typed-value;joints=count-u32:1..32,ordinals-u32-ascending;pose-kind9=count-u32,trs-f32x10;pose-count-matches-joints;duration-f64;tracks-u32>=1;property=component-u16,offset-count-u8:1|4,offsets-u32;key-count-u32;key=time-f64,value,curve-u8;value=field-kind-u8,payload;quaternion-kind=8:xyzw-f32;entity=u64;owned=length-u32,bytes;curve=step:0,linear:1,bezier:2;bezier=time1-f64,value1,time2-f64,value2;monotone-times;last-key-step;exact-payload",
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
    MissingSymbolicId,
    ValueMismatch,
    StaleTarget,
    Unavailable,
    UnsupportedAction,
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
    sink.write(&2u16.to_le_bytes());

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

    sink.write(&(LIMITS.len() as u16).to_le_bytes());
    for (name, value) in LIMITS {
        write_string(sink, name);
        sink.write(&value.to_le_bytes());
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
