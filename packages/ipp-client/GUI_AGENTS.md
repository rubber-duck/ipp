# Semantic GUI agents

`createGuiAgent` adapts explicitly supplied generated World clients into `snapshot`, `query` and `act`. It reads ordinary component inspection and lifecycle observations and submits ordinary `guiAction` commands. It introduces no runtime state, input ownership, clock control or agent transport. The [GUI architecture](../../docs/architecture/gui.md#identity-and-authoritative-state) and [protocol outcomes](../../docs/architecture/protocol-and-schema.md#operations-and-outcomes) remain authoritative.

```ts
import { createGuiAgent, guiAgentTools } from "@ipp/client";

const agent = createGuiAgent({ settings: settingsClient, preview: previewClient });
const found = await agent.query({ world: "settings", label: "Project" });
if (found.controls.length !== 1) throw new Error("Choose an exact control");
const result = await agent.act({
  steps: [{ target: { ref: found.controls[0]!.ref }, action: { kind: "text", value: "Atlas" } }],
});
// Inspect both result.steps (acknowledgements) and result.observed (later values).
```

Matching target-generated GUI modules also export the adapter. The application chooses the clients, applies its authorization policy before forwarding mutations, and maps the [tool schemas](src/gui-agent-schema.ts) to its chosen transport. Tool results contain application-authored text; treat it as data, never instructions. `guiAgentTools` describes inputs; `guiAgentResultSchema` describes snapshot/query and action results using JSON Schema 2020-12. Runtime handles, incarnations, sessions, schema hashes and ticks use decimal strings so results round-trip through JSON without losing precision. [Types](src/gui-agent-types.ts) own the API details.

## Observation and identity

For custom World selections, include the GUI and Canvas Systems and `ipp.lifecycle-publisher` for the lifecycle watches used by discovery. Each World has an application-supplied key. Labels match exactly, including case, and need not be unique within or across Worlds. Queries return every matching control; label actions reject ambiguous or incomplete observations. Prefer the returned opaque reference once the caller chooses a target. A reference includes the adapter instance, originating session, World incarnation, entity generation and exact component incarnation. Reordering preserves it; removal/replacement or a new adapter/session does not retarget it. World keys select the clients supplied when constructing the adapter; they never reopen or reconnect a session.

Inspection pages are independent observations. Each control carries the tick that read its fields and each World reports its read interval, truncation and any detected lifetime or tree inconsistencies. Temporary lifecycle watches fence component identity across those reads and their acknowledged removal; controls that change lifetime during observation are excluded and counted. The default entity bound includes non-control entities; raise `maxEntities` for larger applications and check `truncated`. Queries filter this bounded observation, so an incomplete result cannot establish that a label is unique or absent. When siblings share an order value, the adapter reads their order from the World’s tree inspection. A bounded or changing tree can leave some positions unresolved, including when a parent disappears between pages. Those rows are ungrouped and the observation reports incomplete discovery. Native controls on a committed cyclic branch remain visible to discovery as unavailable and ungrouped. Other rooted controls in that World remain usable. No snapshot is atomic across pages or Worlds, and values, labels and eligibility can change immediately after a read.

Native `GuiBehavior` supplies evaluated enabled, visible and available fields. Actions are listed only when that observed eligibility allows them; current native admission remains decisive. A group without `GuiBehavior` reports null eligibility, since it has no evaluated record. Masked inputs redact both text and committed numeric values. Focus parts expose native indexes without inventing per-part labels; focus actions revalidate the current part configuration. Scalar actions on range sliders address their lower value, as the existing native action does.

## Controls and application composition

Discovery describes native control kinds, not inferred application widgets. A `GuiGroup` organizes its materialized items in observed core tree order; `query({ within: group.ref })` lists them and excludes controls nested inside an item. An entity with several control components is unavailable for input. Only its native primary control determines whether it is a group item or a container whose descendants can be items. A RadioGroup therefore exposes its native group and option buttons, with selection read from their actual `selected` fields. The group's visible caption is not automatically a semantic label.

Dropdown/autocomplete options, filtering, composite values, commands that are not mounted, and application intent belong to the authoring application. This adapter neither registers nor fabricates them, opens a popup to reveal them, nor infers Field label/description/error relationships from Canvas text. A caller may perform an exposed native action and query again after its application has materialized new controls. Per-part semantic labels remain separate work. This is not a screen-reader bridge.

## Ordered actions and final values

`act` submits each step as one generated World batch containing one `guiAction`. It awaits that acknowledgement before the next step, including across Worlds. These are ordered operations, not a transaction or a cross-World atomic batch. A failure stops the suffix without undoing the successful prefix. An operation rejection reports the native reason, such as `StaleTarget`, `Unavailable`, `UnsupportedAction` or `InvalidValue`. An acknowledged commit failure is marked `failed` and may retain effects. Generated encoding failures (`IPP_REQUEST_NOT_SENT`) and Host ingress refusals (`IPP_REQUEST_REJECTED`) are known unapplied rejections. A lost acknowledgement is `unknown`: the operation may already have applied, so it must not be automatically retried. Already closed sessions are rejected before submission. Later steps are `notRun`.

`ok` means every requested action acknowledged success. `observed` is a fresh read afterward, including when a step fails; it does not claim that all requested values persisted, that every momentary effect was consumed, or that arbitrary asynchronous application work completed. Another writer or a controlled React value may change the same native field. Applications can supply `afterAction` to await a known boundary, such as the frame delivering a field observation and a React root's controlled reconciliation, before reading final values. Barrier errors are reported separately and stop later steps. A disconnected World remains explicitly unavailable rather than returning cached values.

The [maintained demonstration](../../tests/gui/agents/README.md) uses real generated native and worker/WASM clients, React callbacks, independent writes and completed GUI captures. It includes controlled rejection, lifecycle races and lost transport acknowledgement.

## Comparison reference

The comparison uses [UI SDK's agent guide](https://ui-sdk.dev/docs/agents/connect-an-agent) and [agent implementation at `282f44729ce08282ef1dc37fa093b1997bbeb848`](https://github.com/vercel-labs/ui/blob/282f44729ce08282ef1dc37fa093b1997bbeb848/packages/core/src/page/agent/agent.ts). The useful common boundary is discoverable state, typed actions, ambiguous-target handling and honest post-action values. IPP applies those ideas through native lifetimes and generated command acknowledgements. UI SDK is not a dependency; DOM registries, automatic off-page reveal, external MCP/WebMCP, history/undo and off-page application intent are outside this adapter.
