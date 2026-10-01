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
    pub(crate) fields: &'static [WireField],
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WireTag {
    pub(crate) name: &'static str,
    pub(crate) space: TagSpace,
    pub(crate) value: u8,
    pub(crate) layout: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AssetFormat {
    pub(crate) name: &'static str,
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
    ($( $(#[$meta:meta])* $name:literal => [$($field:expr),* $(,)?];)+) => {
        pub(crate) const LAYOUTS: &[WireLayout] = &[
            $(
                $(#[$meta])*
                WireLayout {
                    name: $name,
                    fields: &[$($field),*],
                },
            )+
        ];
    };
}

layouts! {
    "gui-target" => [field!("world", Named => "world-reference"), field!("entity", U64), field!("component", U16), field!("incarnation", U64)];
    // GUI System query records, read through inspection collections 6 and 7.
    "gui-focus" => [field!("target", Named => "gui-target"), field!("visible", Bool)];
    "gui-pointer" => [
        field!("target", Named => "gui-target"), field!("pointer", U64),
        field!("hovered", Bool), field!("pressed", Bool), field!("captured", Bool),
    ];
    "request-gui-observation" => [field!("session", U64), field!("request_id", U64), field!("tag", Variant => "request"), field!("control", Bytes, 64)];
    "response-gui-observation" => [field!("session", U64), field!("request_id", U64), field!("tick", U64), field!("tag", Variant => "response"), field!("record", Bytes, MESSAGE_BYTES)];
    "host-request-resolve-world" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("world", Union => "world-selector")];
    "host-request-bind-output" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("world", Named => "world-reference"), field!("entity", U64), field!("kind", Union => "output-kind")];
    "host-request-resolve-output" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("output", Named => "output-reference")];
    "host-request-set-root-output" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("output", Named => "output-reference"), field!("width", U32), field!("height", U32), field!("device_pixel_ratio", NonnegativeFiniteF64)];
    "host-request-clear-root-output" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("binding", Named => "root-binding")];
    "presentation-identity" => [field!("host", U64), field!("serial", U64)];
    "root-binding" => [field!("output", Named => "output-reference"), field!("width", U32), field!("height", U32), field!("device_pixel_ratio", NonnegativeFiniteF64), field!("generation", Named => "presentation-identity")];
    "presentation-surface" => [field!("id", U64), field!("context", U64), field!("max_width", U32), field!("max_height", U32)];
    "presentation-view" => [field!("surface", Named => "presentation-surface"), field!("selection", U64), field!("binding", Named => "root-binding")];
    "presented-source" => [field!("output", Named => "output-reference"), field!("minimum_tick", U64), field!("publication", Named => "presentation-identity"), field!("tick", U64)];
    "presented-frame" => [field!("view", Named => "presentation-view"), field!("sequence", U64), field!("publication", Named => "presentation-identity"), field!("draw_calls", U32), field!("triangles", U32), field!("failed_draw_calls", U32), field!("sources", List, crate::presentation::MAX_PRESENTATION_SOURCES as u32 => "presented-source")];
    "presentation-request-surface" => [field!("tag", Variant => "presentation-request")];
    "presentation-request-select" => [field!("tag", Variant => "presentation-request"), field!("surface", Named => "presentation-surface"), field!("binding", Named => "root-binding")];
    "presentation-request-clear" => [field!("tag", Variant => "presentation-request"), field!("view", Named => "presentation-view")];
    "presentation-request-frame" => [field!("tag", Variant => "presentation-request"), field!("view", Named => "presentation-view"), field!("after_sequence", Option => "u64"), field!("publication", Option => "presentation-identity"), field!("capture", Bool), field!("after_outputs", List, crate::presentation::MAX_PRESENTATION_SOURCES as u32 => "output-reference")];
    "presentation-request-read-capture" => [field!("tag", Variant => "presentation-request"), field!("capture", U64), field!("offset", U64)];
    "presentation-request-release-capture" => [field!("tag", Variant => "presentation-request"), field!("capture", U64)];
    "presentation-request-cancel-frame" => [field!("tag", Variant => "presentation-request"), field!("request", U64)];
    "presentation-response-surface" => [field!("tag", Variant => "presentation-response"), field!("surface", Named => "presentation-surface")];
    "presentation-response-view" => [field!("tag", Variant => "presentation-response"), field!("view", Named => "presentation-view")];
    "presentation-response-frame" => [field!("tag", Variant => "presentation-response"), field!("frame", Named => "presented-frame")];
    "presentation-response-capture" => [field!("tag", Variant => "presentation-response"), field!("frame", Named => "presented-frame"), field!("capture", U64), field!("bytes", U64)];
    "presentation-response-chunk" => [field!("tag", Variant => "presentation-response"), field!("capture", U64), field!("offset", U64), field!("bytes", Bytes, FIELD_BYTES)];
    "presentation-response-complete" => [field!("tag", Variant => "presentation-response")];
    "presentation-response-error" => [field!("tag", Variant => "presentation-response"), field!("error", Union => "presentation-error")];
    "presentation-error-unsupported" => [field!("tag", Variant => "presentation-error")];
    "presentation-error-unavailable" => [field!("tag", Variant => "presentation-error")];
    "presentation-error-stale-view" => [field!("tag", Variant => "presentation-error")];
    "presentation-error-invalid-viewport" => [field!("tag", Variant => "presentation-error")];
    "presentation-error-obsolete-publication" => [field!("tag", Variant => "presentation-error")];
    "presentation-error-capacity" => [field!("tag", Variant => "presentation-error")];
    "presentation-error-timeout" => [field!("tag", Variant => "presentation-error")];
    "presentation-error-draw-failed" => [field!("tag", Variant => "presentation-error")];
    "host-request-presentation" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("body", Union => "presentation-request")];
    "gui-physical-open" => [field!("tag", Variant => "gui-physical-request"), field!("view", Named => "presentation-view"), field!("blockers", List, MESSAGE_BYTES / 32 => "gui-picking-blocker")];
    "gui-physical-close" => [field!("tag", Variant => "gui-physical-request"), field!("context", U64)];
    "gui-physical-event" => [field!("tag", Variant => "gui-physical-request"), field!("context", U64), field!("input", Union => "gui-physical-event")];
    "gui-physical-text" => [field!("tag", Variant => "gui-physical-request"), field!("context", U64), field!("target", Named => "gui-physical-target"), field!("generation", U64), field!("edit", Union => "gui-native-edit")];
    "gui-physical-pointer-down" => [field!("tag", Variant => "gui-physical-event"), field!("pointer", U64), field!("point", Named => "gui-input-vector"), field!("button", Variant => "gui-physical-button")];
    "gui-physical-pointer-move" => [field!("tag", Variant => "gui-physical-event"), field!("pointer", U64), field!("point", Named => "gui-input-vector")];
    "gui-physical-pointer-up" => [field!("tag", Variant => "gui-physical-event"), field!("pointer", U64), field!("point", Named => "gui-input-vector"), field!("button", Variant => "gui-physical-button")];
    "gui-physical-pointer-cancel" => [field!("tag", Variant => "gui-physical-event"), field!("pointer", U64)];
    "gui-physical-wheel" => [field!("tag", Variant => "gui-physical-event"), field!("point", Named => "gui-input-vector"), field!("delta", Named => "gui-input-vector")];
    "gui-physical-key" => [field!("tag", Variant => "gui-physical-event"), field!("key", Variant => "gui-physical-key")];
    "gui-physical-blur" => [field!("tag", Variant => "gui-physical-event")];
    "gui-physical-opened" => [field!("tag", Variant => "gui-physical-response"), field!("context", U64)];
    "gui-physical-closed" => [field!("tag", Variant => "gui-physical-response")];
    "gui-physical-routed" => [field!("tag", Variant => "gui-physical-response"), field!("disposition", Variant => "gui-physical-disposition"), field!("applied", U32), field!("rejected", U32), field!("cancelled", U32), field!("error", Option => "utf8-65536"), field!("remaining", Option => "gui-input-vector"), field!("native", Option => "gui-native-buffer")];
    "gui-physical-rejected" => [field!("tag", Variant => "gui-physical-response"), field!("reason", Utf8, FIELD_BYTES)];
    "gui-physical-revoked" => [field!("tag", Variant => "gui-physical-response"), field!("context", U64)];
    "gui-physical-cancelled" => [field!("tag", Variant => "gui-physical-response"), field!("context", U64), field!("pointers", U8CountedList, ipp_core::services::gui_input::GUI_INPUT_MAX_POINTERS as u32 => "u64"), field!("focus", Bool)];
    "gui-physical-native" => [field!("tag", Variant => "gui-physical-response"), field!("context", U64), field!("state", Option => "gui-native-buffer")];
    "gui-physical-button-primary" => [field!("tag", Variant => "gui-physical-button")];
    "gui-physical-button-secondary" => [field!("tag", Variant => "gui-physical-button")];
    "gui-physical-button-auxiliary" => [field!("tag", Variant => "gui-physical-button")];
    "gui-physical-key-tab" => [field!("tag", Variant => "gui-physical-key")];
    "gui-physical-key-back-tab" => [field!("tag", Variant => "gui-physical-key")];
    "gui-physical-key-enter" => [field!("tag", Variant => "gui-physical-key")];
    "gui-physical-key-space" => [field!("tag", Variant => "gui-physical-key")];
    "gui-physical-key-escape" => [field!("tag", Variant => "gui-physical-key")];
    "gui-physical-key-left" => [field!("tag", Variant => "gui-physical-key")];
    "gui-physical-key-right" => [field!("tag", Variant => "gui-physical-key")];
    "gui-physical-key-up" => [field!("tag", Variant => "gui-physical-key")];
    "gui-physical-key-down" => [field!("tag", Variant => "gui-physical-key")];
    "gui-physical-key-home" => [field!("tag", Variant => "gui-physical-key")];
    "gui-physical-key-end" => [field!("tag", Variant => "gui-physical-key")];
    "gui-native-insert" => [field!("tag", Variant => "gui-native-edit"), field!("text", Utf8, FIELD_BYTES)];
    "gui-native-selection" => [field!("tag", Variant => "gui-native-edit"), field!("start", U32), field!("end", U32)];
    "gui-native-compose" => [field!("tag", Variant => "gui-native-edit"), field!("composition", Named => "gui-native-composition")];
    "gui-native-commit-composition" => [field!("tag", Variant => "gui-native-edit")];
    "gui-native-cancel-composition" => [field!("tag", Variant => "gui-native-edit")];
    "gui-native-backspace" => [field!("tag", Variant => "gui-native-edit")];
    "gui-native-delete" => [field!("tag", Variant => "gui-native-edit")];
    "gui-native-left" => [field!("tag", Variant => "gui-native-edit")];
    "gui-native-right" => [field!("tag", Variant => "gui-native-edit")];
    "gui-native-home" => [field!("tag", Variant => "gui-native-edit")];
    "gui-native-end" => [field!("tag", Variant => "gui-native-edit")];
    "gui-native-select-all" => [field!("tag", Variant => "gui-native-edit")];
    "gui-native-submit" => [field!("tag", Variant => "gui-native-edit")];
    "gui-physical-disposition-routed" => [field!("tag", Variant => "gui-physical-disposition")];
    "gui-physical-disposition-miss" => [field!("tag", Variant => "gui-physical-disposition")];
    "gui-physical-disposition-blocked" => [field!("tag", Variant => "gui-physical-disposition")];
    "gui-physical-disposition-unhandled" => [field!("tag", Variant => "gui-physical-disposition")];
    "gui-input-vector" => [field!("x", FiniteF32), field!("y", FiniteF32)];
    "gui-picking-blocker" => [field!("world", Named => "world-reference"), field!("entity", U64), field!("incarnation", U64)];
    "gui-physical-target" => [field!("world", Named => "world-reference"), field!("entity", U64), field!("component", U16), field!("incarnation", U64)];
    "gui-native-buffer" => [field!("byte_length", U32), field!("state", Named => "gui-native-state")];
    "gui-native-composition" => [field!("text", Utf8, FIELD_BYTES), field!("selection_start", U32), field!("selection_end", U32)];
    "gui-native-state" => [field!("target", Named => "gui-physical-target"), field!("generation", U64), field!("text", Utf8, FIELD_BYTES), field!("selection_start", U32), field!("selection_end", U32), field!("composition", Option => "gui-native-composition")];
    "host-request-gui-input" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("body_bytes", U32), field!("body", Union => "gui-physical-request")];
    "host-response-gui-input" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("body_bytes", U32), field!("body", Union => "gui-physical-response")];
    "host-request-get-root-output-binding" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("world", Named => "world-reference")];
    "host-response-presentation" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("body", Union => "presentation-response")];
    "host-response-root-binding" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("binding", Option => "root-binding")];
    "host-response-world-reference" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("reference", Named => "world-reference")];
    "host-response-output-reference" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("reference", Named => "output-reference")];
    "host-request-list-worlds" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("after", U64)];
    "host-request-create-world" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("symbolic_id", Utf8, FIELD_BYTES), field!("hints", Named => "host-hints-patch"), field!("selected_systems", Option => "host-system-selection"), field!("canvas", Option => "canvas-state"), field!("temporary", Bool)];
    "host-request-open-world" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("world", Named => "world-reference")];
    "host-request-rename-world" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("world", Union => "world-selector"), field!("symbolic_id", Utf8, FIELD_BYTES)];
    "host-request-destroy-world" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("world", Named => "world-reference")];
    "host-request-detach-world" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("session", U64)];
    "host-request-set-capacity-hints" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("session", U64), field!("hints", Named => "host-hints-patch")];
    "host-request-save-world" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("session", U64)];
    "host-request-read-world-save" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("job", U64), field!("offset", U64)];
    "host-request-begin-world-load" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("bytes", U64)];
    "host-request-write-world-load" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("job", U64), field!("offset", U64), field!("bytes", Bytes, FIELD_BYTES)];
    "host-request-finish-world-load" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("job", U64), field!("symbolic_id", Option => "utf8-65536"), field!("hints", Named => "host-hints-patch")];
    "host-request-inspect-world-load" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("job", U64), field!("offset", U32)];
    "host-request-set-world-load-names" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("job", U64), field!("names", List, crate::host::MAX_GRAPH_METADATA_PAGE as u32 => "graph-node-name")];
    "host-request-read-world-load-bindings" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("job", U64), field!("offset", U32)];
    "host-request-acknowledge-world-load" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("job", U64)];
    "host-response-world-graph-page" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("job", U64), field!("root", U32), field!("total", U32), field!("offset", U32), field!("nodes", List, crate::host::MAX_GRAPH_METADATA_PAGE as u32 => "graph-node-metadata")];
    "host-response-world-graph-loaded" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("job", U64), field!("root", Named => "world-reference"), field!("total", U32)];
    "host-response-world-graph-bindings" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("job", U64), field!("offset", U32), field!("bindings", List, crate::host::MAX_GRAPH_BINDING_PAGE as u32 => "graph-node-binding")];
    "graph-node-name" => [field!("id", U32), field!("symbolic_id", Utf8, FIELD_BYTES)];
    "graph-node-metadata" => [field!("id", U32), field!("symbolic_id", Utf8, FIELD_BYTES), field!("persistent_id_low", U64), field!("persistent_id_high", U64)];
    "graph-node-binding" => [field!("id", U32), field!("world", Named => "world-reference")];
    "host-request-cancel-world-transfer" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-request"), field!("job", U64)];
    "host-response-worlds" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("worlds", List, 32 => "host-world"), field!("next", U64)];
    "host-response-created" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("world", Named => "host-world"), field!("reference", Named => "world-reference")];
    "host-response-attached" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("world", Named => "host-world"), field!("session", U64), field!("manifest", Named => "world-manifest"), field!("reference", Named => "world-reference")];
    "host-response-world" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("world", Named => "host-world")];
    "host-response-complete" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response")];
    "host-response-error" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("message", Utf8, FIELD_BYTES)];
    "host-response-detached" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("session", U64), field!("reason", Utf8, FIELD_BYTES)];
    "host-response-transfer" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("job", U64)];
    "host-response-save-chunk" => [field!("magic", U64), field!("connection", U64), field!("request_id", U64), field!("tag", Variant => "host-response"), field!("job", U64), field!("offset", U64), field!("total", U64), field!("bytes", Bytes, FIELD_BYTES)];
    "host-world" => [field!("id", U64), field!("symbolic_id", Utf8, FIELD_BYTES), field!("persistent_id_low", U64), field!("persistent_id_high", U64), field!("entities", U32), field!("systems", List, 1024 => "host-system-hints")];
    "host-hints-patch" => [field!("entities", Option => "u32"), field!("systems", List, 1024 => "host-system-hints")];
    "host-system-hints" => [field!("system", Utf8, FIELD_BYTES), field!("values", List, 1024 => "host-capacity-hint")];
    "host-system-selection" => [field!("systems", List, 1024 => "utf8-65536")];
    "world-manifest" => [field!("systems", List, 1024 => "utf8-65536"), field!("components", List, 1024 => "u16"), field!("operations", List, 32 => "u8")];
    "host-capacity-hint" => [field!("name", Utf8, FIELD_BYTES), field!("value", U32)];
    "world-selector-id" => [field!("tag", Variant => "world-selector"), field!("value", U64)];
    "world-selector-symbol" => [field!("tag", Variant => "world-selector"), field!("value", Utf8, FIELD_BYTES)];

    "value-dynamic" => [field!("tag", Variant => "value"), field!("value", Bytes, FIELD_BYTES)];
    "snapshot-value-dynamic" => [field!("tag", Variant => "snapshot-value"), field!("value", Bytes, FIELD_BYTES)];
    "command-set-dynamic-property" => [field!("tag", Variant => "command"), field!("entity", Union => "reference"), field!("component", U16), field!("name", Utf8, FIELD_BYTES), field!("value", Bytes, FIELD_BYTES)];
    "command-remove-dynamic-property" => [field!("tag", Variant => "command"), field!("entity", Union => "reference"), field!("component", U16), field!("name", Utf8, FIELD_BYTES)];
    "request-lifecycle-unsubscribe" => [
        field!("session", U64), field!("request_id", U64), field!("tag", Variant => "request"), field!("subscription", U64),
    ];
    "response-lifecycle-subscription" => [
        field!("session", U64), field!("request_id", U64), field!("tick", U64), field!("tag", Variant => "response"),
    ];
    "response-lifecycle-events" => [
        field!("session", U64), field!("request_id", U64), field!("tick", U64), field!("tag", Variant => "response"),
        field!("events", List, crate::MAX_LIFECYCLE_PUBLICATIONS as u32 => "lifecycle-publication"),
    ];
    "lifecycle-publication" => [
        field!("subscription", U64), field!("sequence", U64), field!("tick", U64), field!("observation", Union => "lifecycle-observation"),
    ];
    "lifecycle-entity" => [field!("tag", Variant => "lifecycle-observation"), field!("entity", U64)];
    "lifecycle-component" => [
        field!("tag", Variant => "lifecycle-observation"), field!("entity", U64), field!("component", U16),
        field!("previous_incarnation", U64), field!("incarnation", U64),
    ];
    "lifecycle-asset" => [field!("tag", Variant => "lifecycle-observation"), field!("resource", Named => "resource")];
    "request-lifecycle-subscribe" => [
        field!("session", U64), field!("request_id", U64), field!("tag", Variant => "request"),
        field!("subscription", U64), field!("domains", U16), field!("entity", U64), field!("component", U16), field!("asset", U64),
    ];
    "value-bool" => [
        field!("tag", Variant => "value"),
        field!("value", Bool),
    ];
    "snapshot-value-bool" => [
        field!("tag", Variant => "snapshot-value"),
        field!("value", Bool),
    ];
    "render-state-patch" => [
        field!("mask", U16),
        field!("showAllDebugGeometries", Masked, 1 => "bool"),
        field!("debugGeometryColor", Masked, 2 => "linear-rgb"),
        field!("ambientLight", Masked, 4 => "linear-rgb"),
    ];
    "linear-rgb" => [
        field!("r", FiniteF32),
        field!("g", FiniteF32),
        field!("b", FiniteF32),
    ];
    "request-render-state-update" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("changes", Named => "render-state-patch"),
    ];
    "response-render-state-updated" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("changes", Named => "render-state-patch"),
    ];
    "request-geometry-pick" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("view", Union => "view-target"),
        field!("x", FiniteF32),
        field!("y", FiniteF32),
        field!("include_view_plane", Bool),
    ];
    "request-camera-project" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("view", Union => "view-target"),
        field!("x", FiniteF32),
        field!("y", FiniteF32),
        field!("plane", Named => "pick-view-plane"),
    ];
    "response-camera-project" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("view", Option => "view-descriptor"),
        field!("ok", Bool),
        field!("position", Option => "world-point"),
        field!("error", Option => "utf8-65536"),
    ];
    "world-point" => [
        field!("x", FiniteF32),
        field!("y", FiniteF32),
        field!("z", FiniteF32),
    ];
    "response-geometry-pick" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("result", Union => "geometry-pick-outcome"),
    ];
    "view-root" => [
        field!("tag", Variant => "view-target"),
        field!("output", Named => "output-reference"),
        field!("expected_viewport", Named => "view-viewport"),
    ];
    "view-publication" => [
        field!("tag", Variant => "view-target"),
        field!("output", Named => "output-reference"),
        field!("publication", Named => "publication-reference"),
        field!("viewport", Named => "view-viewport"),
    ];
    "view-bound" => [field!("tag", Variant => "view-target"), field!("binding", Named => "root-binding"), field!("publication", Option => "publication-reference")];
    "request-camera-navigate" => [field!("session", U64), field!("request_id", U64), field!("tag", Variant => "request"), field!("binding", Named => "root-binding"), field!("publication", Option => "publication-reference"), field!("kind", U32), field!("first", FiniteF32), field!("second", FiniteF32)];
    "response-camera-navigated" => [field!("session", U64), field!("request_id", U64), field!("tick", U64), field!("tag", Variant => "response")];
    "view-viewport" => [field!("width", U32), field!("height", U32), field!("device_pixel_ratio", NonnegativeFiniteF64)];
    "publication-reference" => [field!("host", U64), field!("revision", U64)];
    "view-descriptor" => [
        field!("output", Named => "output-reference"),
        field!("publication", Named => "publication-reference"),
        field!("viewport", Named => "view-viewport"),
    ];
    "view-path-entry" => [field!("world", Named => "world-reference"), field!("anchor", U64)];
    "pick-result-miss" => [field!("tag", Variant => "geometry-pick-outcome"), field!("view", Named => "view-descriptor")];
    "pick-result-hit" => [
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
    "pick-view-plane" => [
        field!("point_x", FiniteF32),
        field!("point_y", FiniteF32),
        field!("point_z", FiniteF32),
        field!("normal_x", FiniteF32),
        field!("normal_y", FiniteF32),
        field!("normal_z", FiniteF32),
    ];
    "pick-result-failure" => [
        field!("tag", Variant => "geometry-pick-outcome"),
        field!("reason", Utf8, FIELD_BYTES),
    ];

    "metadata" => [
        field!("symbolic_id", Option => "utf8-65536"),
        field!("classes", List, crate::MAX_METADATA_CLASSES as u32 => "utf8-65536"),
    ];
    "field" => [
        field!("offset", U32),
        field!("value", Union => "value"),
    ];
    "component" => [
        field!("type_id", U16),
        field!("fields", List, crate::MAX_INSPECTED_FIELDS as u32 => "snapshot-field"),
    ];
    "snapshot-field" => [
        field!("offset", U32),
        field!("value", Union => "snapshot-value"),
    ];
    "entity" => [
        field!("id", U64),
        field!("metadata", Named => "metadata"),
        field!("parent", U64),
        field!("order_low", U64),
        field!("order_high", U64),
        field!("components", List, crate::MAX_INSPECTED_COMPONENTS as u32 => "component"),
    ];
    "alias-handle" => [
        field!("alias", U32),
        field!("handle", U64),
    ];
    "symbol-handle" => [
        field!("symbol", Utf8, FIELD_BYTES),
        field!("handle", U64),
    ];
    "request-lifecycle-watch" => [
        field!("session", U64), field!("request_id", U64), field!("tag", Variant => "request"),
        field!("world", Named => "world-reference"),
        field!("change", Union => "lifecycle-watch-change"),
    ];
    "lifecycle-watch-add" => [
        field!("tag", Variant => "lifecycle-watch-change"),
        field!("members", List, crate::lifecycle_watch::MAX_LIFECYCLE_MEMBERS as u32 => "lifecycle-watch-selection"),
    ];
    "lifecycle-watch-remove" => [
        field!("tag", Variant => "lifecycle-watch-change"), field!("output", U64),
        field!("generations", List, crate::lifecycle_watch::MAX_LIFECYCLE_MEMBERS as u32 => "u64"),
    ];
    "lifecycle-watch-selection" => [
        field!("target", Union => "lifecycle-watch-target"), field!("kinds", Variant => "lifecycle-watch-kinds"),
    ];
    "lifecycle-watch-entity" => [
        field!("tag", Variant => "lifecycle-watch-target"), field!("entity", U64),
    ];
    "lifecycle-watch-component" => [
        field!("tag", Variant => "lifecycle-watch-target"), field!("entity", U64), field!("component", U16),
    ];
    // Schema field offsets of one component, strictly ascending; never rows or named properties.
    "lifecycle-watch-value" => [
        field!("tag", Variant => "lifecycle-watch-target"), field!("entity", U64), field!("component", U16),
        field!("fields", List, crate::lifecycle_watch::MAX_LIFECYCLE_VALUE_FIELDS as u32 => "u32"),
    ];
    // Every build declares, decodes and answers the lifecycle statistics exchange.
    "request-lifecycle-diagnostics" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("world", Named => "world-reference"),
        field!("output", U64),
    ];
    "response-lifecycle-diagnostics" => [
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
    "response-lifecycle-watch" => [
        field!("session", U64), field!("request_id", U64), field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("world", Named => "world-reference"), field!("output", U64),
        field!("record", Union => "lifecycle-watch-record"),
    ];
    "lifecycle-watch-ack" => [
        field!("tag", Variant => "lifecycle-watch-record"), field!("action", Variant => "lifecycle-watch-change"),
        field!("cut", Option => "lifecycle-watch-cut"), field!("result", Union => "lifecycle-membership-result"),
    ];
    "lifecycle-watch-cut" => [field!("sequence", U64), field!("tick", U64)];
    "lifecycle-watch-applied" => [
        field!("tag", Variant => "lifecycle-membership-result"),
        field!("baselines", List, crate::lifecycle_watch::MAX_LIFECYCLE_MEMBERS as u32 => "lifecycle-watch-baseline"),
    ];
    "lifecycle-watch-rejected" => [
        field!("tag", Variant => "lifecycle-membership-result"), field!("reason", Variant => "lifecycle-membership-rejection"),
    ];
    "lifecycle-watch-cancelled" => [field!("tag", Variant => "lifecycle-membership-result")];
    "lifecycle-watch-baseline" => [
        field!("generation", U64), field!("target", Union => "lifecycle-watch-target"), field!("lifetime", Union => "lifecycle-target-lifetime"),
    ];
    "lifecycle-watch-event" => [
        field!("tag", Variant => "lifecycle-watch-record"), field!("generation", U64), field!("sequence", U64), field!("tick", U64),
        field!("observation", Union => "lifecycle-observation"),
    ];
    // Current values of a value member at the end of an evaluated frame; absent
    // while its entity or component is absent.
    "lifecycle-watch-value-record" => [
        field!("tag", Variant => "lifecycle-watch-record"), field!("generation", U64), field!("tick", U64),
        field!("values", Option => "lifecycle-watch-values"),
    ];
    // Snapshot-encoded field values in the target's field order.
    "lifecycle-watch-values" => [
        field!("fields", List, crate::lifecycle_watch::MAX_LIFECYCLE_VALUE_FIELDS as u32 => "snapshot-field"),
    ];
    "lifecycle-lifetime-entity" => [field!("tag", Variant => "lifecycle-target-lifetime"), field!("live", Bool)];
    "lifecycle-lifetime-component" => [
        field!("tag", Variant => "lifecycle-target-lifetime"), field!("entity_live", Bool), field!("incarnation", U64),
    ];
    "lifecycle-lifetime-removed" => [field!("tag", Variant => "lifecycle-target-lifetime")];
    "resource" => [
        field!("id", U64),
        field!("kind", U16),
        field!("source", Utf8, FIELD_BYTES),
        field!("variant", U32),
        field!("status", Union => "resource-status"),
        field!("representation", Named => "asset-representation"),
    ];
    "asset-representation" => [
        field!("decoded", Bool), field!("graphics_ready", Option => "bool"),
        field!("source_bytes", U64), field!("resident_bytes", U64), field!("graphics_bytes", Option => "u64"),
    ];
    "render-diagnostic" => [
        field!("entity", U64),
        field!("reason", Utf8, FIELD_BYTES),
    ];

    "value-f32" => [
        field!("tag", Variant => "value"),
        field!("value", FiniteF32),
    ];
    "value-entity" => [
        field!("tag", Variant => "value"),
        field!("value", Union => "reference"),
    ];
    "value-u32" => [
        field!("tag", Variant => "value"),
        field!("value", U32),
    ];
    "value-u64" => [
        field!("tag", Variant => "value"),
        field!("value", U64),
    ];
    "value-string" => [
        field!("tag", Variant => "value"),
        field!("value", Utf8, FIELD_BYTES),
    ];
    "value-bytes" => [
        field!("tag", Variant => "value"),
        field!("value", Bytes, FIELD_BYTES),
    ];
    "snapshot-value-f32" => [
        field!("tag", Variant => "snapshot-value"),
        field!("value", FiniteF32),
    ];
    "snapshot-value-entity" => [
        field!("tag", Variant => "snapshot-value"),
        field!("reference_tag", Variant => "snapshot-reference"),
        field!("value", U64),
    ];
    "snapshot-value-u32" => [
        field!("tag", Variant => "snapshot-value"),
        field!("value", U32),
    ];
    "snapshot-value-u64" => [
        field!("tag", Variant => "snapshot-value"),
        field!("value", U64),
    ];
    "snapshot-value-string" => [
        field!("tag", Variant => "snapshot-value"),
        field!("value", Utf8, FIELD_BYTES),
    ];
    "snapshot-value-bytes" => [
        field!("tag", Variant => "snapshot-value"),
        field!("value", Bytes, MESSAGE_BYTES),
    ];
    // A whole schema rows table in the component's row layout.
    "world-reference" => [
        field!("id", U64),
        field!("incarnation", U64),
    ];
    "output-reference" => [
        field!("world", Named => "world-reference"),
        field!("target", Union => "output-target"),
    ];
    "output-canvas" => [field!("tag", Variant => "output-kind")];
    "output-camera" => [field!("tag", Variant => "output-kind")];
    // The World-level canvas; its lifetime is the World's.
    "output-target-canvas" => [field!("tag", Variant => "output-target")];
    "output-target-camera" => [
        field!("tag", Variant => "output-target"),
        field!("entity", U64),
        field!("incarnation", U64),
    ];
    "canvas-extent" => [field!("width", FiniteF32), field!("height", FiniteF32)];
    "canvas-density" => [field!("units_per_metre", FiniteF32)];
    "canvas-state" => [
        field!("extent", Named => "canvas-extent"),
        field!("density", Named => "canvas-density"),
    ];
    "canvas-state-update" => [
        field!("mask", U16),
        field!("extent", Masked, 1 => "canvas-extent"),
        field!("density", Masked, 2 => "canvas-density"),
    ];
    "request-canvas-state-update" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("update", Named => "canvas-state-update"),
    ];
    // Canvas System query record, read through inspection collection 8.
    "canvas-evaluated-extent" => [
        field!("extent", Named => "canvas-extent"),
        field!("tick", U64),
    ];
    "canvas-state-record" => [
        field!("state", Named => "canvas-state"),
        field!("evaluated", Option => "canvas-evaluated-extent"),
    ];
    "value-world" => [
        field!("tag", Variant => "value"),
        field!("value", Option => "world-reference"),
    ];
    "value-output" => [
        field!("tag", Variant => "value"),
        field!("value", Option => "output-reference"),
    ];
    "snapshot-value-world" => [
        field!("tag", Variant => "snapshot-value"),
        field!("value", Option => "world-reference"),
    ];
    "snapshot-value-output" => [
        field!("tag", Variant => "snapshot-value"),
        field!("value", Option => "output-reference"),
    ];
    "value-rows" => [
        field!("tag", Variant => "value"),
        field!("value", Bytes, MESSAGE_BYTES),
    ];
    "snapshot-value-rows" => [
        field!("tag", Variant => "snapshot-value"),
        field!("value", Bytes, MESSAGE_BYTES),
    ];
    // Absence of an optional schema row property.
    "value-unset" => [
        field!("tag", Variant => "value"),
    ];
    "snapshot-value-unset" => [
        field!("tag", Variant => "snapshot-value"),
    ];

    "request-submit-batch" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("batch_id", U32),
        field!("last", Bool),
        field!("operations", List, crate::COMMAND_PAGE_COMMANDS as u32 => "command"),
    ];
    "request-inspect" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("collection", Variant => "inspection-collection"),
        field!("after", U64),
        field!("target", U64),
        field!("limit", U16),
        field!("max_depth", U16),
    ];

    "command-create" => [
        field!("tag", Variant => "command"),
        field!("alias", U32),
        field!("metadata", Named => "metadata"),
        field!("adopt", Bool),
    ];
    "command-delete" => [
        field!("tag", Variant => "command"),
        field!("entity", Union => "reference"),
    ];
    "entity-placement" => [
        field!("parent", Option => "reference-value"),
        field!("before", Option => "reference-value"),
    ];
    "reference-value" => [field!("value", Union => "reference")];
    "command-place-entity" => [
        field!("tag", Variant => "command"),
        field!("entity", Union => "reference"),
        field!("placement", Named => "entity-placement"),
    ];
    "command-delete-subtree" => [
        field!("tag", Variant => "command"),
        field!("root", Union => "reference"),
    ];
    "command-metadata" => [
        field!("tag", Variant => "command"),
        field!("entity", Union => "reference"),
        field!("metadata", Named => "metadata"),
    ];
    "command-insert" => [
        field!("tag", Variant => "command"),
        field!("entity", Union => "reference"),
        field!("component", U16),
        field!("fields", List, crate::MAX_INSERT_FIELDS as u32 => "field"),
        field!("adopt", Bool),
    ];
    "command-set" => [
        field!("tag", Variant => "command"),
        field!("entity", Union => "reference"),
        field!("component", U16),
        field!("field", Named => "field"),
    ];
    // Compare-and-set: `expected` has the field's own type at the field's offset.
    "command-set-field-if" => [
        field!("tag", Variant => "command"),
        field!("entity", Union => "reference"),
        field!("component", U16),
        field!("field", Named => "field"),
        field!("expected", Union => "value"),
    ];
    // One semantic action on the control component at its exact incarnation.
    "command-gui-action" => [
        field!("tag", Variant => "command"),
        field!("entity", Union => "reference"),
        field!("component", U16),
        field!("incarnation", U64),
        field!("action", Union => "gui-action"),
    ];
    "gui-action-press" => [field!("tag", Variant => "gui-action")];
    "gui-action-toggle" => [field!("tag", Variant => "gui-action")];
    "gui-action-set-scalar" => [field!("tag", Variant => "gui-action"), field!("value", FiniteF32)];
    "gui-action-set-text" => [field!("tag", Variant => "gui-action"), field!("text", Utf8, FIELD_BYTES)];
    "gui-action-focus" => [field!("tag", Variant => "gui-action")];
    "gui-action-blur" => [field!("tag", Variant => "gui-action")];
    "gui-action-submit" => [field!("tag", Variant => "gui-action")];
    "gui-action-scroll-to" => [field!("tag", Variant => "gui-action"), field!("offset", Named => "gui-input-vector")];
    "gui-action-scroll-by" => [field!("tag", Variant => "gui-action"), field!("delta", Named => "gui-input-vector")];
    "gui-action-scroll-to-index" => [field!("tag", Variant => "gui-action"), field!("index", U32), field!("offset", FiniteF32)];
    "command-remove" => [
        field!("tag", Variant => "command"),
        field!("entity", Union => "reference"),
        field!("component", U16),
    ];

    "reference-handle" => [
        field!("tag", Variant => "reference"),
        field!("handle", U64),
    ];
    "reference-alias" => [
        field!("tag", Variant => "reference"),
        field!("alias", U32),
    ];
    // A live entity named by its symbolic identifier, resolved at the command's boundary.
    "reference-symbol" => [
        field!("tag", Variant => "reference"),
        field!("symbol", Utf8, FIELD_BYTES),
    ];

    "response-batch-aborted" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("batch_id", U64),
        field!("message", Utf8, crate::MAX_FAILURE_MESSAGE_BYTES as u32),
    ];
    "response-batch" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("outcome", Union => "outcome"),
        field!("effects", List, crate::BATCH_OUTCOME_EFFECTS as u32 => "applied-operation-effect"),
    ];
    // One applied effect at its zero-based operation index, in core order.
    "applied-operation-effect" => [
        field!("operation", U32),
        field!("effect", Union => "operation-effect"),
    ];
    "attachment-effect" => [
        field!("tag", Variant => "operation-effect"),
        field!("receipt", U64),
        field!("parent", Named => "world-reference"),
        field!("anchor", U64),
        field!("incarnation", U64),
        field!("revision", U64),
        field!("child", Option => "world-reference"),
    ];
    // An adopting create or insert found its entity or component already present.
    "operation-effect-adopted" => [field!("tag", Variant => "operation-effect")];
    "command-detach-attachment-receipt" => [
        field!("tag", Variant => "command"),
        field!("receipt", U64),
    ];
    "request-attachment-receipt" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("receipt", U64),
        field!("release", Bool),
    ];
    "response-attachment-receipt" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("receipt", U64),
        field!("state", Variant => "attachment-receipt-state"),
    ];
    "response-runtime-failure" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("scope", Variant => "runtime-failure-scope"),
        field!("faulted", Bool),
        field!("message", Utf8, crate::MAX_FAILURE_MESSAGE_BYTES as u32),
    ];
    "response-frame" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("time", NonnegativeFiniteF64),
    ];
    "response-inspect" => [
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
        field!("gui_focus", List, PAGE_RECORDS => "gui-focus"),
        field!("gui_pointers", List, PAGE_RECORDS => "gui-pointer"),
        field!("canvas", Option => "canvas-state-record"),
    ];
    "response-entity-tree" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("time", NonnegativeFiniteF64),
        field!("next", U64),
        field!("nodes", List, PAGE_RECORDS => "entity-tree-node"),
    ];
    "entity-tree-node" => [
        field!("id", U64),
        field!("parent", U64),
        field!("order_low", U64),
        field!("order_high", U64),
        field!("depth", U16),
    ];
    "animation-controller" => [
        field!("state", Named => "controller-state"),
        field!("description", Named => "controller-description"),
        field!("transition", Option => "controller-transition-state"),
    ];
    "controller-transition-state" => [
        field!("duration", NonnegativeFiniteF64),
        field!("elapsed", NonnegativeFiniteF64),
        field!("easing", U32),
        field!("pending", Bool),
    ];
    "controller-description" => [
        field!("speed", FiniteF32),
        field!("looping", Bool),
        field!("drivers", List, u32::MAX => "animation-driver"),
    ];
    "animation-driver" => [
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
    "animation-property" => [
        field!("target", Union => "animation-target"),
    ];
    "animation-target-entity-link" => [field!("kind", Variant => "animation-target")];
    "animation-target-property" => [field!("kind", Variant => "animation-target"), field!("component", U16), field!("indices", List, crate::MAX_ANIMATION_TARGET_INDICES as u32 => "animation-index")];
    "animation-target-dynamic" => [field!("kind", Variant => "animation-target"), field!("component", U16), field!("name", Utf8, FIELD_BYTES)];
    "animation-target-joints" => [field!("kind", Variant => "animation-target"), field!("indices", List, crate::MAX_ANIMATION_TARGET_INDICES as u32 => "animation-index")];
    "animation-index" => [
        field!("value", U32),
    ];
    "request-controller-create" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("description", Named => "controller-description"),
    ];
    "request-controller-update" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("id", U64),
        field!("description", Named => "controller-description"),
    ];
    "request-controller-delete" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("id", U64),
    ];
    "request-controller-control" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("id", U64),
        field!("control", U32),
        field!("time", NonnegativeFiniteF64),
        field!("speed", FiniteF32),
    ];
    "request-controller-transition" => [
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
    "response-controller" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("id", U64),
    ];
    "controller-state" => [
        field!("id", U64),
        field!("state", U32),
        field!("time", NonnegativeFiniteF64),
    ];
    "playback-event" => [
        field!("controller", Named => "controller-state"),
        field!("kind", U32),
        field!("reason", Utf8, FIELD_BYTES),
    ];
    "request-playback" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tag", Variant => "request"),
        field!("controller", U64),
        field!("control", U32),
        field!("time", NonnegativeFiniteF64),
        field!("speed", FiniteF32),
    ];
    "response-playback" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("events", List, crate::MAX_PLAYBACK_EVENTS as u32 => "playback-event"),
    ];
    "response-resources" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("resources", List, crate::MAX_RESOURCE_EVENT_RECORDS as u32 => "resource"),
    ];
    "response-error" => [
        field!("session", U64),
        field!("request_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "response"),
        field!("code", U16),
        field!("message", Utf8, FIELD_BYTES),
    ];

    "outcome-success" => [
        field!("batch_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "outcome"),
        field!("aliases", List, crate::BATCH_OUTCOME_ALIASES as u32 => "alias-handle"),
        field!("symbols", List, crate::BATCH_OUTCOME_ALIASES as u32 => "symbol-handle"),
    ];
    "outcome-failure" => [
        field!("batch_id", U64),
        field!("tick", U64),
        field!("tag", Variant => "outcome"),
        field!("scope", Variant => "batch-error-scope"),
        field!("operation", Option => "u32"),
        field!("reason", Utf8, FIELD_BYTES),
        field!("aliases", List, crate::BATCH_OUTCOME_ALIASES as u32 => "alias-handle"),
        field!("symbols", List, crate::BATCH_OUTCOME_ALIASES as u32 => "symbol-handle"),
    ];

    "resource-status-unloaded" => [field!("tag", Variant => "resource-status")];
    "resource-status-start" => [field!("tag", Variant => "resource-status")];
    "resource-status-progress" => [
        field!("tag", Variant => "resource-status"),
        field!("completed", U64),
        field!("total", Option => "u64"),
    ];
    "resource-status-loaded" => [field!("tag", Variant => "resource-status")];
    "resource-status-failed" => [
        field!("tag", Variant => "resource-status"),
        field!("error", Utf8, ipp_core::services::asset_management::MAX_ASSET_ERROR_BYTES as u32),
    ];
}

macro_rules! tags {
    ($( $(#[$meta:meta])* $space:ident $name:ident = $value:expr => $layout:literal;)+) => {
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
                    value: $value,
                    layout: $layout,
                },
            )+
        ];
    };
}

tags! {
    GuiPhysicalRequest GUI_PHYSICAL_REQUEST_OPEN = 0 => "gui-physical-open";
    GuiPhysicalRequest GUI_PHYSICAL_REQUEST_CLOSE = 1 => "gui-physical-close";
    GuiPhysicalRequest GUI_PHYSICAL_REQUEST_EVENT = 2 => "gui-physical-event";
    GuiPhysicalRequest GUI_PHYSICAL_REQUEST_TEXT = 3 => "gui-physical-text";
    GuiPhysicalEvent GUI_PHYSICAL_EVENT_POINTER_DOWN = 0 => "gui-physical-pointer-down";
    GuiPhysicalEvent GUI_PHYSICAL_EVENT_POINTER_MOVE = 1 => "gui-physical-pointer-move";
    GuiPhysicalEvent GUI_PHYSICAL_EVENT_POINTER_UP = 2 => "gui-physical-pointer-up";
    GuiPhysicalEvent GUI_PHYSICAL_EVENT_POINTER_CANCEL = 3 => "gui-physical-pointer-cancel";
    GuiPhysicalEvent GUI_PHYSICAL_EVENT_WHEEL = 4 => "gui-physical-wheel";
    GuiPhysicalEvent GUI_PHYSICAL_EVENT_KEY = 5 => "gui-physical-key";
    GuiPhysicalEvent GUI_PHYSICAL_EVENT_BLUR = 6 => "gui-physical-blur";
    GuiPhysicalResponse GUI_PHYSICAL_RESPONSE_OPENED = 0 => "gui-physical-opened";
    GuiPhysicalResponse GUI_PHYSICAL_RESPONSE_CLOSED = 1 => "gui-physical-closed";
    GuiPhysicalResponse GUI_PHYSICAL_RESPONSE_ROUTED = 2 => "gui-physical-routed";
    GuiPhysicalResponse GUI_PHYSICAL_RESPONSE_REJECTED = 3 => "gui-physical-rejected";
    GuiPhysicalResponse GUI_PHYSICAL_RESPONSE_REVOKED = 4 => "gui-physical-revoked";
    GuiPhysicalResponse GUI_PHYSICAL_RESPONSE_CANCELLED = 5 => "gui-physical-cancelled";
    GuiPhysicalResponse GUI_PHYSICAL_RESPONSE_NATIVE = 6 => "gui-physical-native";
    GuiPhysicalButton GUI_PHYSICAL_BUTTON_PRIMARY = 0 => "gui-physical-button-primary";
    GuiPhysicalButton GUI_PHYSICAL_BUTTON_SECONDARY = 1 => "gui-physical-button-secondary";
    GuiPhysicalButton GUI_PHYSICAL_BUTTON_AUXILIARY = 2 => "gui-physical-button-auxiliary";
    GuiPhysicalKey GUI_PHYSICAL_KEY_TAB = 0 => "gui-physical-key-tab";
    GuiPhysicalKey GUI_PHYSICAL_KEY_BACK_TAB = 1 => "gui-physical-key-back-tab";
    GuiPhysicalKey GUI_PHYSICAL_KEY_ENTER = 2 => "gui-physical-key-enter";
    GuiPhysicalKey GUI_PHYSICAL_KEY_SPACE = 3 => "gui-physical-key-space";
    GuiPhysicalKey GUI_PHYSICAL_KEY_ESCAPE = 4 => "gui-physical-key-escape";
    GuiPhysicalKey GUI_PHYSICAL_KEY_LEFT = 5 => "gui-physical-key-left";
    GuiPhysicalKey GUI_PHYSICAL_KEY_RIGHT = 6 => "gui-physical-key-right";
    GuiPhysicalKey GUI_PHYSICAL_KEY_UP = 7 => "gui-physical-key-up";
    GuiPhysicalKey GUI_PHYSICAL_KEY_DOWN = 8 => "gui-physical-key-down";
    GuiPhysicalKey GUI_PHYSICAL_KEY_HOME = 9 => "gui-physical-key-home";
    GuiPhysicalKey GUI_PHYSICAL_KEY_END = 10 => "gui-physical-key-end";
    GuiNativeEdit GUI_NATIVE_EDIT_INSERT = 0 => "gui-native-insert";
    GuiNativeEdit GUI_NATIVE_EDIT_SELECTION = 1 => "gui-native-selection";
    GuiNativeEdit GUI_NATIVE_EDIT_COMPOSE = 2 => "gui-native-compose";
    GuiNativeEdit GUI_NATIVE_EDIT_COMMIT_COMPOSITION = 3 => "gui-native-commit-composition";
    GuiNativeEdit GUI_NATIVE_EDIT_CANCEL_COMPOSITION = 4 => "gui-native-cancel-composition";
    GuiNativeEdit GUI_NATIVE_EDIT_BACKSPACE = 5 => "gui-native-backspace";
    GuiNativeEdit GUI_NATIVE_EDIT_DELETE = 6 => "gui-native-delete";
    GuiNativeEdit GUI_NATIVE_EDIT_LEFT = 7 => "gui-native-left";
    GuiNativeEdit GUI_NATIVE_EDIT_RIGHT = 8 => "gui-native-right";
    GuiNativeEdit GUI_NATIVE_EDIT_HOME = 9 => "gui-native-home";
    GuiNativeEdit GUI_NATIVE_EDIT_END = 10 => "gui-native-end";
    GuiNativeEdit GUI_NATIVE_EDIT_SELECT_ALL = 11 => "gui-native-select-all";
    GuiNativeEdit GUI_NATIVE_EDIT_SUBMIT = 12 => "gui-native-submit";
    GuiPhysicalDisposition GUI_PHYSICAL_DISPOSITION_ROUTED = 0 => "gui-physical-disposition-routed";
    GuiPhysicalDisposition GUI_PHYSICAL_DISPOSITION_MISS = 1 => "gui-physical-disposition-miss";
    GuiPhysicalDisposition GUI_PHYSICAL_DISPOSITION_BLOCKED = 2 => "gui-physical-disposition-blocked";
    GuiPhysicalDisposition GUI_PHYSICAL_DISPOSITION_UNHANDLED = 3 => "gui-physical-disposition-unhandled";
    Request REQUEST_LIFECYCLE_WATCH = 35 => "request-lifecycle-watch";
    Response RESPONSE_LIFECYCLE_WATCH = 37 => "response-lifecycle-watch";
    Request REQUEST_LIFECYCLE_DIAGNOSTICS = 36 => "request-lifecycle-diagnostics";
    Response RESPONSE_LIFECYCLE_DIAGNOSTICS = 38 => "response-lifecycle-diagnostics";
    LifecycleWatchChange LIFECYCLE_WATCH_ADD = 0 => "lifecycle-watch-add";
    LifecycleWatchChange LIFECYCLE_WATCH_REMOVE = 1 => "lifecycle-watch-remove";
    LifecycleWatchTarget LIFECYCLE_WATCH_ENTITY = 0 => "lifecycle-watch-entity";
    LifecycleWatchTarget LIFECYCLE_WATCH_COMPONENT = 1 => "lifecycle-watch-component";
    LifecycleWatchTarget LIFECYCLE_WATCH_VALUE = 2 => "lifecycle-watch-value";
    LifecycleWatchRecord LIFECYCLE_WATCH_ACK = 0 => "lifecycle-watch-ack";
    LifecycleWatchRecord LIFECYCLE_WATCH_EVENT = 1 => "lifecycle-watch-event";
    LifecycleWatchRecord LIFECYCLE_WATCH_VALUE_RECORD = 3 => "lifecycle-watch-value-record";
    LifecycleMembershipResult LIFECYCLE_MEMBERSHIP_APPLIED = 0 => "lifecycle-watch-applied";
    LifecycleMembershipResult LIFECYCLE_MEMBERSHIP_REJECTED = 1 => "lifecycle-watch-rejected";
    LifecycleMembershipResult LIFECYCLE_MEMBERSHIP_CANCELLED = 2 => "lifecycle-watch-cancelled";
    LifecycleTargetLifetime LIFECYCLE_LIFETIME_ENTITY = 0 => "lifecycle-lifetime-entity";
    LifecycleTargetLifetime LIFECYCLE_LIFETIME_COMPONENT = 1 => "lifecycle-lifetime-component";
    LifecycleTargetLifetime LIFECYCLE_LIFETIME_REMOVED = 2 => "lifecycle-lifetime-removed";
    LifecycleMembershipRejection LIFECYCLE_MEMBERSHIP_STALE_WORLD = 0 => "empty";
    LifecycleMembershipRejection LIFECYCLE_MEMBERSHIP_STALE_SESSION = 1 => "empty";
    LifecycleMembershipRejection LIFECYCLE_MEMBERSHIP_STALE_MEMBER = 2 => "empty";
    LifecycleMembershipRejection LIFECYCLE_MEMBERSHIP_ALREADY_ACTIVE = 3 => "empty";
    LifecycleMembershipRejection LIFECYCLE_MEMBERSHIP_TRACKING_ENDED = 4 => "empty";
    LifecycleMembershipRejection LIFECYCLE_MEMBERSHIP_CAPACITY = 5 => "empty";
    LifecycleWatchKinds LIFECYCLE_WATCH_ENTITY_CREATED = 1 => "empty";
    LifecycleWatchKinds LIFECYCLE_WATCH_ENTITY_METADATA_CHANGED = 2 => "empty";
    LifecycleWatchKinds LIFECYCLE_WATCH_ENTITY_DELETED = 4 => "empty";
    LifecycleWatchKinds LIFECYCLE_WATCH_COMPONENT_INSERTED = 8 => "empty";
    LifecycleWatchKinds LIFECYCLE_WATCH_COMPONENT_UPDATED = 16 => "empty";
    LifecycleWatchKinds LIFECYCLE_WATCH_COMPONENT_REPLACED = 32 => "empty";
    LifecycleWatchKinds LIFECYCLE_WATCH_COMPONENT_REMOVED = 64 => "empty";
    LifecycleWatchKinds LIFECYCLE_WATCH_VALUE_CHANGED = 128 => "empty";
    // Request and response tag 26 carried Surface item edits; requests 31 and
    // 32 (GUI action, now a command) and responses 28 (GUI terminal) and 33 are
    // also retired. Retired tags are never reused.
    Request REQUEST_GUI_OBSERVATION = 34 => "request-gui-observation";
    Response RESPONSE_GUI_OBSERVATION = 36 => "response-gui-observation";
    HostRequest HOST_REQUEST_RESOLVE_WORLD = 14 => "host-request-resolve-world";
    PresentationRequest PRESENTATION_REQUEST_SURFACE = 1 => "presentation-request-surface";
    PresentationRequest PRESENTATION_REQUEST_SELECT = 2 => "presentation-request-select";
    PresentationRequest PRESENTATION_REQUEST_CLEAR = 3 => "presentation-request-clear";
    PresentationRequest PRESENTATION_REQUEST_FRAME = 4 => "presentation-request-frame";
    PresentationRequest PRESENTATION_REQUEST_READ_CAPTURE = 5 => "presentation-request-read-capture";
    PresentationRequest PRESENTATION_REQUEST_RELEASE_CAPTURE = 6 => "presentation-request-release-capture";
    PresentationRequest PRESENTATION_REQUEST_CANCEL_FRAME = 7 => "presentation-request-cancel-frame";
    PresentationResponse PRESENTATION_RESPONSE_SURFACE = 1 => "presentation-response-surface";
    PresentationResponse PRESENTATION_RESPONSE_VIEW = 2 => "presentation-response-view";
    PresentationResponse PRESENTATION_RESPONSE_FRAME = 3 => "presentation-response-frame";
    PresentationResponse PRESENTATION_RESPONSE_CAPTURE = 4 => "presentation-response-capture";
    PresentationResponse PRESENTATION_RESPONSE_CHUNK = 5 => "presentation-response-chunk";
    PresentationResponse PRESENTATION_RESPONSE_COMPLETE = 6 => "presentation-response-complete";
    PresentationResponse PRESENTATION_RESPONSE_ERROR = 7 => "presentation-response-error";
    PresentationError PRESENTATION_ERROR_UNSUPPORTED = 1 => "presentation-error-unsupported";
    PresentationError PRESENTATION_ERROR_UNAVAILABLE = 2 => "presentation-error-unavailable";
    PresentationError PRESENTATION_ERROR_STALE_VIEW = 3 => "presentation-error-stale-view";
    PresentationError PRESENTATION_ERROR_INVALID_VIEWPORT = 4 => "presentation-error-invalid-viewport";
    PresentationError PRESENTATION_ERROR_OBSOLETE_PUBLICATION = 5 => "presentation-error-obsolete-publication";
    PresentationError PRESENTATION_ERROR_CAPACITY = 6 => "presentation-error-capacity";
    PresentationError PRESENTATION_ERROR_TIMEOUT = 7 => "presentation-error-timeout";
    PresentationError PRESENTATION_ERROR_DRAW_FAILED = 8 => "presentation-error-draw-failed";
    HostRequest HOST_REQUEST_PRESENTATION = 23 => "host-request-presentation";
    HostRequest HOST_REQUEST_GET_ROOT_OUTPUT_BINDING = 24 => "host-request-get-root-output-binding";
    HostRequest HOST_REQUEST_GUI_INPUT = 25 => "host-request-gui-input";
    HostResponse HOST_RESPONSE_GUI_INPUT = 19 => "host-response-gui-input";
    HostResponse HOST_RESPONSE_PRESENTATION = 17 => "host-response-presentation";
    HostResponse HOST_RESPONSE_ROOT_BINDING = 18 => "host-response-root-binding";
    HostRequest HOST_REQUEST_BIND_OUTPUT = 15 => "host-request-bind-output";
    HostRequest HOST_REQUEST_RESOLVE_OUTPUT = 16 => "host-request-resolve-output";
    HostRequest HOST_REQUEST_SET_ROOT_OUTPUT = 17 => "host-request-set-root-output";
    HostRequest HOST_REQUEST_CLEAR_ROOT_OUTPUT = 18 => "host-request-clear-root-output";
    HostResponse HOST_RESPONSE_CREATED = 12 => "host-response-created";
    HostResponse HOST_RESPONSE_WORLD_REFERENCE = 10 => "host-response-world-reference";
    HostResponse HOST_RESPONSE_OUTPUT_REFERENCE = 11 => "host-response-output-reference";
    HostRequest HOST_REQUEST_LIST_WORLDS = 1 => "host-request-list-worlds";
    HostRequest HOST_REQUEST_CREATE_WORLD = 2 => "host-request-create-world";
    HostRequest HOST_REQUEST_OPEN_WORLD = 3 => "host-request-open-world";
    HostRequest HOST_REQUEST_RENAME_WORLD = 4 => "host-request-rename-world";
    HostRequest HOST_REQUEST_DESTROY_WORLD = 5 => "host-request-destroy-world";
    HostRequest HOST_REQUEST_DETACH_WORLD = 6 => "host-request-detach-world";
    HostRequest HOST_REQUEST_SET_CAPACITY_HINTS = 7 => "host-request-set-capacity-hints";
    HostRequest HOST_REQUEST_SAVE_WORLD = 8 => "host-request-save-world";
    HostRequest HOST_REQUEST_READ_WORLD_SAVE = 9 => "host-request-read-world-save";
    HostRequest HOST_REQUEST_BEGIN_WORLD_LOAD = 10 => "host-request-begin-world-load";
    HostRequest HOST_REQUEST_WRITE_WORLD_LOAD = 11 => "host-request-write-world-load";
    HostRequest HOST_REQUEST_FINISH_WORLD_LOAD = 12 => "host-request-finish-world-load";
    HostRequest HOST_REQUEST_CANCEL_WORLD_TRANSFER = 13 => "host-request-cancel-world-transfer";
    HostRequest HOST_REQUEST_INSPECT_WORLD_LOAD = 19 => "host-request-inspect-world-load";
    HostRequest HOST_REQUEST_SET_WORLD_LOAD_NAMES = 20 => "host-request-set-world-load-names";
    HostRequest HOST_REQUEST_READ_WORLD_LOAD_BINDINGS = 21 => "host-request-read-world-load-bindings";
    HostRequest HOST_REQUEST_ACKNOWLEDGE_WORLD_LOAD = 22 => "host-request-acknowledge-world-load";
    HostResponse HOST_RESPONSE_WORLD_GRAPH_PAGE = 14 => "host-response-world-graph-page";
    HostResponse HOST_RESPONSE_WORLD_GRAPH_LOADED = 15 => "host-response-world-graph-loaded";
    HostResponse HOST_RESPONSE_WORLD_GRAPH_BINDINGS = 16 => "host-response-world-graph-bindings";
    HostResponse HOST_RESPONSE_WORLDS = 1 => "host-response-worlds";
    HostResponse HOST_RESPONSE_ATTACHED = 2 => "host-response-attached";
    HostResponse HOST_RESPONSE_WORLD = 3 => "host-response-world";
    HostResponse HOST_RESPONSE_COMPLETE = 4 => "host-response-complete";
    HostResponse HOST_RESPONSE_ERROR = 5 => "host-response-error";
    HostResponse HOST_RESPONSE_DETACHED = 6 => "host-response-detached";
    HostResponse HOST_RESPONSE_TRANSFER = 7 => "host-response-transfer";
    HostResponse HOST_RESPONSE_SAVE_CHUNK = 9 => "host-response-save-chunk";
    WorldSelector WORLD_SELECTOR_ID = 0 => "world-selector-id";
    WorldSelector WORLD_SELECTOR_SYMBOL = 1 => "world-selector-symbol";
    AnimationTarget ANIMATION_TARGET_PROPERTY = 0 => "animation-target-property";
    AnimationTarget ANIMATION_TARGET_JOINTS = 1 => "animation-target-joints";
    AnimationTarget ANIMATION_TARGET_DYNAMIC = 2 => "animation-target-dynamic";
    AnimationTarget ANIMATION_TARGET_ENTITY_LINK = 3 => "animation-target-entity-link";
    PlaybackControl PLAYBACK_CONTROL_PLAY = 0 => "empty";
    PlaybackControl PLAYBACK_CONTROL_PAUSE = 1 => "empty";
    PlaybackControl PLAYBACK_CONTROL_STOP = 2 => "empty";
    PlaybackControl PLAYBACK_CONTROL_SEEK = 3 => "empty";
    PlaybackControl PLAYBACK_CONTROL_RESTART = 4 => "empty";
    PlaybackState PLAYBACK_STATE_STOPPED = ipp_core::systems::animation::AnimationPlaybackStatus::Stopped as u8 => "empty";
    PlaybackState PLAYBACK_STATE_PLAYING = ipp_core::systems::animation::AnimationPlaybackStatus::Playing as u8 => "empty";
    PlaybackState PLAYBACK_STATE_PAUSED = ipp_core::systems::animation::AnimationPlaybackStatus::Paused as u8 => "empty";
    PlaybackState PLAYBACK_STATE_COMPLETED = ipp_core::systems::animation::AnimationPlaybackStatus::Completed as u8 => "empty";
    PlaybackEvent PLAYBACK_EVENT_STARTED = ipp_core::systems::animation::AnimationPlaybackEventKind::Started as u8 => "empty";
    PlaybackEvent PLAYBACK_EVENT_PAUSED = ipp_core::systems::animation::AnimationPlaybackEventKind::Paused as u8 => "empty";
    PlaybackEvent PLAYBACK_EVENT_STOPPED = ipp_core::systems::animation::AnimationPlaybackEventKind::Stopped as u8 => "empty";
    PlaybackEvent PLAYBACK_EVENT_COMPLETED = ipp_core::systems::animation::AnimationPlaybackEventKind::Completed as u8 => "empty";
    PlaybackEvent PLAYBACK_EVENT_INVALIDATED = ipp_core::systems::animation::AnimationPlaybackEventKind::Invalidated as u8 => "empty";
    PlaybackEvent PLAYBACK_EVENT_FAILED = ipp_core::systems::animation::AnimationPlaybackEventKind::Failed as u8 => "empty";

    InspectionCollection INSPECT_SUMMARY = 0 => "empty";
    InspectionCollection INSPECT_ENTITIES = 1 => "empty";
    InspectionCollection INSPECT_RESOURCES = 2 => "empty";
    InspectionCollection INSPECT_CONTROLLERS = 3 => "empty";
    InspectionCollection INSPECT_RENDER_DIAGNOSTICS = 4 => "empty";
    InspectionCollection INSPECT_ENTITY_TREE = 5 => "empty";
    InspectionCollection INSPECT_GUI_FOCUS = 6 => "empty";
    InspectionCollection INSPECT_GUI_POINTERS = 7 => "empty";
    InspectionCollection INSPECT_CANVAS = 8 => "empty";
    Request REQUEST_LIFECYCLE_SUBSCRIBE = 19 => "request-lifecycle-subscribe";
    Request REQUEST_LIFECYCLE_UNSUBSCRIBE = 20 => "request-lifecycle-unsubscribe";
    Response RESPONSE_LIFECYCLE_SUBSCRIPTION = 16 => "response-lifecycle-subscription";
    Response RESPONSE_LIFECYCLE_EVENTS = 17 => "response-lifecycle-events";
    LifecycleObservation LIFECYCLE_ENTITY_CREATED = 1 => "lifecycle-entity";
    LifecycleObservation LIFECYCLE_ENTITY_METADATA_CHANGED = 2 => "lifecycle-entity";
    LifecycleObservation LIFECYCLE_ENTITY_DELETED = 3 => "lifecycle-entity";
    LifecycleObservation LIFECYCLE_COMPONENT_INSERTED = 4 => "lifecycle-component";
    LifecycleObservation LIFECYCLE_COMPONENT_UPDATED = 5 => "lifecycle-component";
    LifecycleObservation LIFECYCLE_COMPONENT_REPLACED = 6 => "lifecycle-component";
    LifecycleObservation LIFECYCLE_COMPONENT_REMOVED = 7 => "lifecycle-component";
    LifecycleObservation LIFECYCLE_ASSET_STATUS_CHANGED = 8 => "lifecycle-asset";
    LifecycleObservation LIFECYCLE_ASSET_REMOVED = 9 => "lifecycle-asset";
    LifecycleObservation LIFECYCLE_ASSET_GRAPHICS_INVALIDATED = 10 => "lifecycle-asset";
    Value VALUE_BOOL = ipp_core::components::schema::FieldKind::Bool as u8 => "value-bool";
    SnapshotValue SNAPSHOT_VALUE_BOOL = ipp_core::components::schema::FieldKind::Bool as u8 => "snapshot-value-bool";
    Request REQUEST_RENDER_STATE_UPDATE = 10 => "request-render-state-update";
    Request REQUEST_CANVAS_STATE_UPDATE = 41 => "request-canvas-state-update";
    Response RESPONSE_RENDER_STATE_UPDATED = 12 => "response-render-state-updated";
    Value VALUE_F32 = ipp_core::components::schema::FieldKind::F32 as u8 => "value-f32";
    Value VALUE_ENTITY = ipp_core::components::schema::FieldKind::Entity as u8 => "value-entity";
    Value VALUE_U32 = ipp_core::components::schema::FieldKind::U32 as u8 => "value-u32";
    Value VALUE_U64 = ipp_core::components::schema::FieldKind::U64 as u8 => "value-u64";
    Value VALUE_STRING = ipp_core::components::schema::FieldKind::String as u8 => "value-string";
    Value VALUE_DYNAMIC = ipp_core::components::schema::FieldKind::Dynamic as u8 => "value-dynamic";
    SnapshotValue SNAPSHOT_VALUE_DYNAMIC = ipp_core::components::schema::FieldKind::Dynamic as u8 => "snapshot-value-dynamic";
    Value VALUE_BYTES = ipp_core::components::schema::FieldKind::Bytes as u8 => "value-bytes";
    SnapshotValue SNAPSHOT_VALUE_F32 = ipp_core::components::schema::FieldKind::F32 as u8 => "snapshot-value-f32";
    SnapshotValue SNAPSHOT_VALUE_ENTITY = ipp_core::components::schema::FieldKind::Entity as u8 => "snapshot-value-entity";
    SnapshotValue SNAPSHOT_VALUE_U32 = ipp_core::components::schema::FieldKind::U32 as u8 => "snapshot-value-u32";
    SnapshotValue SNAPSHOT_VALUE_U64 = ipp_core::components::schema::FieldKind::U64 as u8 => "snapshot-value-u64";
    SnapshotValue SNAPSHOT_VALUE_STRING = ipp_core::components::schema::FieldKind::String as u8 => "snapshot-value-string";
    SnapshotValue SNAPSHOT_VALUE_BYTES = ipp_core::components::schema::FieldKind::Bytes as u8 => "snapshot-value-bytes";
    // Snapshot value tag 14 referred to a base descriptor table; retired tags are never reused.
    Value VALUE_ROWS = ipp_core::components::schema::FieldKind::Rows as u8 => "value-rows";
    Value VALUE_WORLD = ipp_core::components::schema::FieldKind::World as u8 => "value-world";
    OutputKind OUTPUT_CANVAS = 0 => "output-canvas";
    OutputKind OUTPUT_CAMERA = 1 => "output-camera";
    OutputTarget OUTPUT_TARGET_CANVAS = 0 => "output-target-canvas";
    OutputTarget OUTPUT_TARGET_CAMERA = 1 => "output-target-camera";
    Value VALUE_OUTPUT = ipp_core::components::schema::FieldKind::Output as u8 => "value-output";
    SnapshotValue SNAPSHOT_VALUE_WORLD = ipp_core::components::schema::FieldKind::World as u8 => "snapshot-value-world";
    SnapshotValue SNAPSHOT_VALUE_OUTPUT = ipp_core::components::schema::FieldKind::Output as u8 => "snapshot-value-output";
    SnapshotValue SNAPSHOT_VALUE_ROWS = ipp_core::components::schema::FieldKind::Rows as u8 => "snapshot-value-rows";
    Value VALUE_UNSET = ipp_core::components::schema::FieldKind::Unset as u8 => "value-unset";
    SnapshotValue SNAPSHOT_VALUE_UNSET = ipp_core::components::schema::FieldKind::Unset as u8 => "snapshot-value-unset";
    SnapshotReference SNAPSHOT_REF_HANDLE = REF_HANDLE => "empty";

    Request REQUEST_CONTROLLER_CREATE = 15 => "request-controller-create";
    Request REQUEST_CONTROLLER_UPDATE = 16 => "request-controller-update";
    Request REQUEST_CONTROLLER_DELETE = 17 => "request-controller-delete";
    Request REQUEST_CONTROLLER_CONTROL = 18 => "request-controller-control";
    Request REQUEST_CONTROLLER_TRANSITION = 27 => "request-controller-transition";
    Response RESPONSE_CONTROLLER = 15 => "response-controller";
    Request REQUEST_PLAYBACK = 13 => "request-playback";
    Response RESPONSE_PLAYBACK = 14 => "response-playback";
    AnimationTransitionEasing ANIMATION_TRANSITION_LINEAR = 0 => "empty";
    AnimationTransitionEasing ANIMATION_TRANSITION_SMOOTHSTEP = 1 => "empty";
    AnimationTransitionStartTime ANIMATION_TRANSITION_RESTART = 0 => "empty";
    AnimationTransitionStartTime ANIMATION_TRANSITION_PRESERVE = 1 => "empty";
    AnimationTransitionStartTime ANIMATION_TRANSITION_MATCH_PHASE = 2 => "empty";
    AnimationTransitionStartTime ANIMATION_TRANSITION_SEEK = 3 => "empty";
    PlaybackControl PLAYBACK_CONTROL_PLAY_AT_SPEED = 5 => "empty";

    Response RESPONSE_BATCH_ABORTED = 24 => "response-batch-aborted";
    Request REQUEST_SUBMIT_BATCH = 1 => "request-submit-batch";
    Request REQUEST_INSPECT = 3 => "request-inspect";
    Request REQUEST_ATTACHMENT_RECEIPT = 33 => "request-attachment-receipt";
    Response RESPONSE_ATTACHMENT_RECEIPT = 35 => "response-attachment-receipt";
    OperationEffect ATTACHMENT_WRITTEN = 0 => "attachment-effect";
    OperationEffect ATTACHMENT_DETACHED = 1 => "attachment-effect";
    OperationEffect ATTACHMENT_SUPERSEDED = 2 => "attachment-effect";
    OperationEffect OPERATION_ADOPTED = 3 => "operation-effect-adopted";
    AttachmentReceiptState RECEIPT_PENDING = 0 => "empty";
    AttachmentReceiptState RECEIPT_RETIRED = 1 => "empty";
    AttachmentReceiptState RECEIPT_RELEASED = 2 => "empty";
    Request REQUEST_GEOMETRY_PICK = 9 => "request-geometry-pick";
    Request REQUEST_CAMERA_PROJECT = 12 => "request-camera-project";
    Request REQUEST_CAMERA_NAVIGATE = 40 => "request-camera-navigate";
    Response RESPONSE_CAMERA_NAVIGATED = 40 => "response-camera-navigated";

    Command COMMAND_CREATE = 1 => "command-create";
    Command COMMAND_DELETE = 2 => "command-delete";
    Command COMMAND_PLACE_ENTITY = 17 => "command-place-entity";
    Command COMMAND_DELETE_SUBTREE = 18 => "command-delete-subtree";
    Command COMMAND_DETACH_ATTACHMENT_RECEIPT = 22 => "command-detach-attachment-receipt";
    Command COMMAND_METADATA = 3 => "command-metadata";
    Command COMMAND_INSERT = 4 => "command-insert";
    Command COMMAND_SET = 5 => "command-set";
    Command COMMAND_SET_FIELD_IF = 23 => "command-set-field-if";
    Command COMMAND_SET_DYNAMIC_PROPERTY = 14 => "command-set-dynamic-property";
    Command COMMAND_REMOVE_DYNAMIC_PROPERTY = 15 => "command-remove-dynamic-property";
    Command COMMAND_REMOVE = 6 => "command-remove";
    Command COMMAND_GUI_ACTION = 24 => "command-gui-action";
    // Command tags 7-13, 16 and 19-21 are retired; retired tags are never reused.
    // GUI action 5 (explicit value replacement) is retired and never reused.
    GuiAction GUI_ACTION_PRESS = 0 => "gui-action-press";
    GuiAction GUI_ACTION_TOGGLE = 1 => "gui-action-toggle";
    GuiAction GUI_ACTION_SET_SCALAR = 2 => "gui-action-set-scalar";
    GuiAction GUI_ACTION_SET_TEXT = 3 => "gui-action-set-text";
    GuiAction GUI_ACTION_FOCUS = 4 => "gui-action-focus";
    GuiAction GUI_ACTION_BLUR = 6 => "gui-action-blur";
    GuiAction GUI_ACTION_SUBMIT = 7 => "gui-action-submit";
    GuiAction GUI_ACTION_SCROLL_TO = 8 => "gui-action-scroll-to";
    GuiAction GUI_ACTION_SCROLL_BY = 9 => "gui-action-scroll-by";
    GuiAction GUI_ACTION_SCROLL_TO_INDEX = 10 => "gui-action-scroll-to-index";

    Reference REF_HANDLE = 0 => "reference-handle";
    Reference REF_ALIAS = 1 => "reference-alias";
    // Reference tag 2 is retired; retired tags are never reused.
    Reference REF_SYMBOL = 3 => "reference-symbol";

    Response RESPONSE_BATCH = 1 => "response-batch";
    Response RESPONSE_INSPECT = 3 => "response-inspect";
    Response RESPONSE_ENTITY_TREE = 34 => "response-entity-tree";
    Response RESPONSE_FRAME = 4 => "response-frame";
    // Response tag 5 is retired; retired tags are never reused.
    Response RESPONSE_RESOURCES = 9 => "response-resources";
    Response RESPONSE_GEOMETRY_PICK = 11 => "response-geometry-pick";
    Response RESPONSE_CAMERA_PROJECT = 13 => "response-camera-project";
    Response RESPONSE_RUNTIME_FAILURE = 22 => "response-runtime-failure";
    RuntimeFailureScope FAILURE_DRAW = crate::RuntimeFailureScope::Draw as u8 => "empty";
    RuntimeFailureScope FAILURE_RESOURCE = crate::RuntimeFailureScope::Resource as u8 => "empty";
    RuntimeFailureScope FAILURE_CONTEXT = crate::RuntimeFailureScope::Context as u8 => "empty";
    RuntimeFailureScope FAILURE_WORLD = crate::RuntimeFailureScope::World as u8 => "empty";
    Response RESPONSE_ERROR = 255 => "response-error";

    GeometryPickOutcome PICK_OUTCOME_MISS = 0 => "pick-result-miss";
    GeometryPickOutcome PICK_OUTCOME_HIT = 1 => "pick-result-hit";
    GeometryPickOutcome PICK_OUTCOME_FAILURE = 3 => "pick-result-failure";
    ViewTarget VIEW_ROOT = 0 => "view-root";
    ViewTarget VIEW_PUBLICATION = 1 => "view-publication";
    ViewTarget VIEW_BOUND = 2 => "view-bound";

    Outcome OUTCOME_SUCCESS = 0 => "outcome-success";
    Outcome OUTCOME_FAILURE = 1 => "outcome-failure";

    BatchErrorScope BATCH_ERROR_OPERATION = 0 => "empty";
    BatchErrorScope BATCH_ERROR_COMMIT = 1 => "empty";
    Option OPTION_NONE = 0 => "empty";
    Option OPTION_SOME = 1 => "present";
    AssetResourceStatus RESOURCE_UNLOADED = 0 => "resource-status-unloaded";
    AssetResourceStatus RESOURCE_START = 1 => "resource-status-start";
    AssetResourceStatus RESOURCE_PROGRESS = 2 => "resource-status-progress";
    AssetResourceStatus RESOURCE_LOADED = 3 => "resource-status-loaded";
    AssetResourceStatus RESOURCE_FAILED = 4 => "resource-status-failed";
}

pub(crate) const CONVENTIONS: &[(&str, &str)] = &[
    (
        "attachment-receipts",
        "session-owned-bounded-registry;effects=ordered-operation-index,exact-parent,anchor,component-incarnation,nonreused-write-revision,child;conditional-detach-resolves-at-operation;partial-prefix-retained;pending-is-not-retired;release-invalidates-handle;runtime-only;reply-and-registry-capacity-before-callbacks",
    ),
    (
        "gui-local",
        "ordinary-world-entity-component-incarnation;payloads=length-u32-bytes;per-world-session-only;query=kind-u8:entity0-u64|tree1-root?-bool-u64,after?-bool-u64,limit-u16-1..256,maxdepth-u16-0..64;action=world-u64x2,entity-u64,component-u16,incarnation-u64,revision-u32,operation-u8:press0|toggle1|scalar2-f32|text3-utf8|focus4|replace5-value|blur6;value=tag-u8:none0|bool1-u8|scalar2-f32|text3-utf8-max65536;page=world-u64x2,next?-bool-u64,count-u32-max256,row*;row=entity-u64,parent?-bool-u64,order-u128-le,depth-u16,control?-bool-(target,role-u8:button0|checkbox1|slider2|text3,revision-u32,value,label-utf8,ancestry-count-u32-u64*,enabled-bool,visible-bool,available-bool,focused-bool,hovered-bool,pressed-bool,captured-bool);terminal=tag-u8:applied0-effect|rejected1-reason|cancelled2;effect=target,source-u8:semantic0|replacement1,tick-u64,ancestry-count-u32-u64*,kind-u8:pressed0|focus-changed1-focused-bool-changed-bool|committed2-revision-u32-value;reason=u8:session0|capacity1|duplicate2|unavailable3|context6|path7|cancelled8|delivery-session9|delivery-capacity10|target11|revision12-current-u32|local-unavailable13|unsupported14|value15|revision-exhausted16;reserve-slot-rejection-and-exact-applied-bytes-before-mutation;single-terminal-no-generic-command-ack;header-tick=applied-effect-tick|no-effect-zero;terminal-not-frame-observation;no-root-required;equal-replacement-advances-revision;visited-entity-pages-not-state-replication",
    ),
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
    (
        "hello",
        "request=IPPB;reply=IPPB,version-u32,schema-hash-u64,connection-u64;no-client-claim;client-decides-compatibility",
    ),
    (
        "contract-request",
        "request=IPCQ;reply=IPCR,contract=IPPB,version-u32,schema-hash-u64,descriptors;after-hello;admitted-and-charged-as-reply",
    ),
    (
        "host-control",
        "revision=2;request=IPPH-2-0-0-0,connection-u64,request-u64,tag-u8;response=IPPA-2-0-0-0,connection-u64,request-u64,tag-u8;requests=1-list,2-create,3-open,4-rename,5-destroy,6-detach,7-hints,14-resolve-world,15-bind-output,16-resolve-output,17-set-root,18-clear-root;responses=1-worlds,2-attached,3-world,4-complete,5-error,6-detached,10-world-reference,11-output-reference,12-created;create=world-options,selected-system-names-option,canvas-state-option=extent-f32x2,units-per-metre-f32-finite-positive,only-with-canvas-system;output-reference=world-reference,target-u8:canvas0-world-lifetime|camera1-entity-u64,incarnation-u64;attached=descriptor,session-u64,selected-world-manifest,world-reference;world-manifest=systems-string-list,components-u16-list,operations-u8-list;operations=entity-links0,animation2,joint-animation3,constraints4,look-at5,geometry6,rendering7,camera8,surface9,gui10,particles11,canvas12;descriptor=id-u64,symbol-string,persistent-u128,hints;selector=tag-u8:0-id-u64,1-symbol-string;hints=optional-entities-u32,system-string-to-string-u32-map;connection-hello=announcement-only;standalone-session-hello=announcement-and-world-manifest;creation-independent-of-opening;opening-independent-of-root;ordered-with-world-ingress;world-list-page=32;session-fresh;temporary-explicit;world-clock-host-owned",
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
    AssetFormat {
        name: "ASSET_FONT",
        type_id: 17,
        format: "IPPF;version=1;quadratic-contours;original-glyph-identities;cmap;metrics;pair-kerning;cpu-layout;renderer-owned-acceleration",
    },
    AssetFormat {
        name: "ASSET_DRAWING",
        type_id: 18,
        format: "IPPD;version=1;quadratic-contours;paint-order;solid-srgb-rgba;nonzero-or-evenodd;source-bounds;tolerance;renderer-owned-acceleration",
    },
    AssetFormat {
        name: "ASSET_PARTICLE_CACHE",
        type_id: ipp_core::systems::particles::PARTICLE_CACHE_TYPE.0,
        format: "IPPC;version-u32=1;space-u32=local0|world1;frames-u32;directory=time-f32,offset-u32,count-u32;sample=id-u64,birth-f32,death-f32,position-f32x3,velocity-f32x3,rotation-f32x4,size-f32;60-byte-samples;ordered-times-and-identities;exact-payload",
    },
    AssetFormat {
        name: "ASSET_PARTICLE_SURFACE",
        type_id: ipp_core::systems::particles::PARTICLE_SURFACE_TYPE.0,
        format: "IPPM;version=1|2|3;decoded-mesh-metadata;triangle-area-weighted-emission;cpu-only",
    },
    AssetFormat {
        name: "ASSET_SHADER",
        type_id: ipp_core::services::asset_management::shader::SHADER_TYPE.0,
        format: "IPPH;version-u32=2;recipe-flags-u32;backend-utf8;attributes-u32;parameters-count-u32:name-utf8,kind-u8;backends-count-u32:name-utf8,vertex-utf8,fragment-utf8;immutable;interface=glsl-es-300-v1;shader-parameter-kinds=f32:1,i32:2,u32:3,bool:4,vec2:5,vec3:6,vec4:7,mat2:8,mat3:9,mat4:10,texture2D:11",
    },
    AssetFormat {
        name: "ASSET_GEOMETRY",
        type_id: ipp_core::systems::geometry::GEOMETRY_TYPE.0,
        format: "IPPG;version-u32=1;count-u32>=1;part=tag-u32,parameters-f32x7,trs-f32x10,joints-u32x2;box0=min3,max3,zero;sphere1=center3,radius,zero3;pill2=start3,end3,radius;joints-none=4294967295,4294967295;joint-pair-requires-skeleton:0..31-and-zero-endpoints;positive-radius;finite-values;exact-payload;union-leaf-order;geometry-skeleton-zero=own-skin-or-self",
    },
    AssetFormat {
        name: "ASSET_ANIMATION",
        type_id: ipp_core::systems::animation::ANIMATION_TYPE.0,
        format: "IPPA;version-u32=4;track=target-u8:property0|joints1|dynamic2|entity-link3;entity-link-key-kind10=parent-slot-u32,before-slot-u32;none-slot=4294967295;slots-resolve-per-driver;dynamic=component-u16,name-utf8;dynamic-value-kind11=length-u32,typed-value;joints=count-u32:1..32,ordinals-u32-ascending;pose-kind9=count-u32,trs-f32x10;pose-count-matches-joints;duration-f64;tracks-u32>=1;property=component-u16,offset-count-u8:1|4,offsets-u32;key-count-u32;key=time-f64,value,curve-u8;value=field-kind-u8,payload;quaternion-kind=8:xyzw-f32;entity=u64;owned=length-u32,bytes;curve=step:0,linear:1,bezier:2;bezier=time1-f64,value1,time2-f64,value2;monotone-times;last-key-step;exact-payload",
    },
    AssetFormat {
        name: "ASSET_SKELETON",
        type_id: ipp_core::SKELETON_TYPE.0,
        format: "IPPS;version=1;count-u32=1..32;parent-u32-root=4294967295;parent-precedes-child;trs-f32x10;exact-payload",
    },
    AssetFormat {
        name: "ASSET_POSE",
        type_id: ipp_core::POSE_TYPE.0,
        format: "IPPP;version=1;count-u32=1..32;trs-f32x10;exact-payload",
    },
    AssetFormat {
        name: "ASSET_SKIN",
        type_id: ipp_core::SKIN_TYPE.0,
        format: "IPPB;version=1;count-u32=1..32;joint-u32;inverse-bind-f32x16-column-major-affine-invertible;exact-payload",
    },
    AssetFormat {
        name: "ASSET_MESH",
        type_id: ipp_core::MESH_TYPE.0,
        format: "IPPM;version=1|2|3;v1=position-f32x3,color-f32x3,index-u16;v2=position-f32x3,color-f32x3,uv-f32x2,index-u16;v3=ordered-attribute-descriptors-and-packed-streams,normal-f32x3-semantic4-finite-nonzero;finite-values;loaded-means-usable",
    },
    AssetFormat {
        name: "ASSET_TEXTURE",
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
    sink.write(&3u16.to_le_bytes());

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
        sink.write(&[tag.space as u8, tag.value]);
        write_string(sink, tag.layout);
    }

    sink.write(&(ASSET_FORMATS.len() as u16).to_le_bytes());
    for format in ASSET_FORMATS {
        write_string(sink, format.name);
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
