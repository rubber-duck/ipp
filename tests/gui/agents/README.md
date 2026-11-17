# Semantic GUI agent demonstration

The [shared scenario](scenarios/agent-actions.tsx) builds a React form with button, text, controlled text and radio choices in two Worlds. The matching generated client exports the semantic adapter under test. [The driver](drivers/browser-agent-actions.ts) supplies either native WebSocket/GLES or worker/WASM/WebGL and can cut the real connection after a submitted message; scenario assertions do not depend on wire bytes, URLs or launch mechanics.

Run the maintained suite from the repository root:

```sh
python tools/ipp.py test gui-agents --software
```

For focused regression evidence, select `test:gui-agents:native` or `test:gui-agents:browser` through `python tools/ipp.py regression --only ... --software`. Native presentation needs `IPP_EGL_LIBRARY_DIR`; concurrent browser pipelines use software rendering. The [suite catalog](../../../tools/pipeline/suites.json) owns prerequisites and the GUI regression group includes both arrangements.

The scenario checks duplicate labels, materialized native group order (including tied sibling order labels and entities with several control components), acknowledgement/callback/field agreement, controlled rejection, another writer, disabled/hidden/retained-unavailable controls, masked-text redaction, component replacement both during observation and between query/action, temporary-watch removal acknowledgement, bounded enumeration, plain parent retirement between pages, ordered partial failure and transport uncertainty. Captures wait for completed source publications and assert skinned pixels and visible text/selection changes. Evidence retains the matching schema hashes, JSON-safe tool/result schemas, action reports and before/after PNGs. Owned Worlds are temporary; roots, observers, presentation and transports are released on completion or failure.

This provides a transport-independent agent boundary demonstration. It does not establish external model/MCP integration, screen-reader support or application-owned off-page option discovery; see the [adapter guide](../../../packages/ipp-client/GUI_AGENTS.md).
