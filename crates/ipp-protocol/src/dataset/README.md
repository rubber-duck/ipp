# Dataset connection lane

The dataset codec and Host connection adapter expose [Host-owned DataService sources](../../../../docs/architecture/data.md) independently of World command batches and asset payloads. Names remain literal Host-wide strings. Producer tokens identify one incarnation and belong to the connection that created it; release detaches a producer and explicit destruction removes its source.

Finite updates use shared `IoUploadAssembly` byte staging. Transport credit acknowledges framing progress, while one final outcome reports the ordered committed prefix or a transport refusal. DataService owns domain validation, source storage, expiry and notifications. Production source admission occurs with exclusive Host access, before World evaluation reads. Malformed incomplete payloads never enter domain admission.

[Requests](requests.rs), [responses](responses.rs), [payload encoding](payload.rs) and the [wire manifest](../wire.rs) define implementation formats and bounds. Typed source reads copy one bounded independent observation, with schema, incarnation, stable row identities and pagination; they acquire no demand and promise no shared snapshot across pages.

The maintained [dataset scenario](../../../../tests/integration/scenarios/datasets.ts) runs through matching generated native WebSocket and worker/WASM clients with readiness, failure artifacts and owned cleanup. The shared [combined authoring scenario](../../../../tests/integration/scenarios/data-authoring.ts) adds expression assets, entity-local projection, driven and animated parameters, shared windows and metadata-only persistence through fresh Hosts. The existing [animation scene](../../../../tests/render/animation-fixture.ts) checks expression-driven visible position through completed WebGL frames; this does not establish Plot rendering.

## Completed binding and driver observations

`BindingView` and `DriverStatus` are read-only dataset-lane requests naming an attached World session and an exact entity handle. The physical connection must own that session. Admission reserves reliable output before reading; credit stays charged through transport delivery. Queries acquire no source demand, perform no preparation/evaluation and never acknowledge presentation dirty state.

[Observation encoding](observations.rs) owns the bounded body inside the normal connection/request/tag envelope and length-prefixed response. A binding body carries source incarnation (optional), binding incarnation, evaluated tick (optional), four availability strings (reason/detail/output/input), dirty, total rows, offset, optional continuation, named exact-kind column descriptors, and aligned rows. Each row has its stable source-row identity and one explicit result per column: valid results carry one canonical dataset value; invalid results carry their reason and optional input slot. Optional integers encode a canonical presence byte plus a `u64` (zero when absent). The page limit and actual encoded byte budget apply before copying; an oversized descriptor or individual row fails explicitly. Pages observe independent completed cuts and must be compared using lifetime/tick fences.

A driver body carries payload availability, evaluation state, retained reason, optional input slot, rejection detail and recovered. Stable reason names distinguish preparation failures from arithmetic invalidity. Observation is separate from component mutation and animation controls; there is no step or dirty-acknowledgement request.

## Generated authoring

[Executed-target metadata](../data_authoring.rs) obtains IPPE node/operator tags and IPPW/IPDI headers/tags by encoding valid declarations through the canonical core codecs. Core expression limits and driver input counts enter the same hashed contract. [The generator template](../../../../tools/ipp-schema-gen/src/data-authoring.template.ts) exposes typed declarations/builders, window constraints and driver selectors; application authors supply ordinary values and generated field addresses rather than bytes. Core remains the authority for graph semantics, window admission and declaration bounds.
