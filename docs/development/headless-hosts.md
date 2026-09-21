# Running Headless Hosts

[Build guide](building.md) · [Runtime](../architecture/runtime.md) · [Protocol](../architecture/protocol-and-schema.md) · [Testing policy](integration-testing.md)

Native WebSocket and browser worker hosts run the same headless World behavior through target-generated clients. Hosts own time; clients enqueue work and observe outcomes. The [client guide](../../packages/ipp-client/README.md) owns connection, World attachment and persistence APIs.

## Run and verify

Install the [pinned tools](building.md), then run at repository root:

```sh
python tools/ipp.py setup node
python tools/ipp.py setup browser --with-deps
python tools/ipp.py test native browser worlds
```

Select additional suites for the behavior being changed:

| Suite | Evidence |
| --- | --- |
| `native`, `browser` | Real WebSocket executable or Chromium worker/WASM, matching generated client and lifecycle |
| `worlds`, `lifecycle` | Shared World/resource behavior, subscriptions, partial effects and session isolation |
| `hierarchy` | Object parenting and terminal LookAt through native scenarios and browser frames |
| `snapshots` | Reference-only persistence through native/browser transports and fresh-worker rendering |
| `client`, `contracts` | Codecs/correlation and executed native/WASM contract generation |

Use `python tools/ipp.py test --list` for the current [suite registry](../../tools/pipeline/suites.json). Unit tests and headless frame notifications do not establish [rendered-image evidence](rendering.md).

For an interactive native session, start the Host:

```sh
cargo run -p ipp-server --no-default-features --features websocket --locked
```

Then, in another terminal:

```sh
python tools/ipp.py dev headless-client ws://127.0.0.1:9231
```

The example prepares its matching client, creates source/driven entities, waits for events, edits the source and prints state. The driven Scalar retains authored 99 while evaluating to 9, then 13. Use `python tools/ipp.py dev headless-client --build` to prepare without connecting. [Example source](../../examples/headless-client/main.ts).

## Host operation

The native executable listens on loopback; `--bind 127.0.0.1:0` selects an ephemeral port and prints a flushed JSON readiness URL. An explicit file root/prefix enables filesystem access; the default server exposes none. The current adapter is local `ws://`; see [startup options](../../crates/ipp-server/src/main.rs) and [transport limits](../../crates/ipp-server/src/websocket.rs).

Bootstrap checks the target contract before ordinary requests. `batch()` resolves after ordered application and evaluation; `inspect()` observes a Host boundary. `waitForFrame()` waits for a newer notification and sends no step request. Semantic batch failure retains applied work and permits correction; protocol violations and connection congestion have separate failure handling. Use [diagnostics](building.md#diagnostic-output) for lifecycle and command boundaries.

Browser workers own their clock, MessagePort and WASM instance. Visible workers require `requestAnimationFrame`; paused workers use a maintenance timer to preserve ingress and asset progression without advancing simulation time. Resume resets the timing baseline; clients never supply `dt`. The native Host targets 60 Hz using deadlines that account for frame work and wakeup jitter. Overruns add no further sleep and do not accumulate catch-up frames. Buffer ownership and delivery backpressure live in the [worker transport](../../packages/ipp-client/src/worker.ts) and [WASM Host](../../crates/ipp-wasm/src).

## Maintained integration harness

[Shared scenarios](../../tests/integration/scenarios) express operations/outcomes independently of ports, process handles and wire framing. Native and browser environment runners own launch, readiness, bounded waits, artifact capture and cleanup. Add another driver when introducing a transport instead of copying scenarios.

World profiles are prepared under `target/world-host-build/{native,wasm}`. Browser distributions under `target/browser-build/` contain generated clients, worker support, executed `export.wasm`, matching final `runtime.wasm` and `build-report.json`. [Build configurations](../../tools/pipeline/profiles.json) select capabilities. Runners retain logs, schema/build identity and outcomes under `target/integration-artifacts/`.

## Dependency and size accounting

The client uses platform WebSocket/MessagePort without runtime package dependencies. React/build tooling is separate. Native networking remains behind `websocket`; compiler macros and contract exporters are build costs. [Workspace policy](../architecture/rust-workspace.md) and the [size commands](building.md#release-and-size-experiments) govern dependency and artifact measurements. Report final WASM separately from export modules and JavaScript support; count shared files once.
