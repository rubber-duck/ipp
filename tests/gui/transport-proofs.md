# GUI physical delivery proofs

These maintained fault drivers exercise ordinary GUI effects across independent connections to one clock-owning Host. They supplement, rather than replace, the generated-client semantic scenarios in `gui-local.test.ts`.

## Evidence bounds

- Worker `retainedBytes` is a test payload ledger, **not an independently measured live account total or total memory usage**. Production native pending-ticket counts and the reviewed lease implementation support the retention inference.
- The receiver is terminated **after congestion has already closed its connection**, before transferred output drains. This does not establish detection of an otherwise-live receiver crash.
- Native replacement attaches through a pre-established fourth socket; it is not a fresh TCP connect after B fails.
- Development and production-minified consumers use the same Worker/WASM compilation.
- No 64-connection exhaustion, rendering, B–N scaling, performance, whole-GUI or global TypeScript/browser validation claim follows from these focused proofs.

Run the focused selections through the regression entry point:

```sh
python tools/ipp.py regression --only test:gui-local:native-output --only test:gui-local:native --only test:gui-local:worker --only test:gui-local:worker-delivery
```

## Native socket boundary

The Linux-only GUI test in `crates/ipp-server/src/websocket_congestion_tests.rs` uses real TCP/WebSocket sockets and the production socket writer and Host loop. B stops reading its socket after its subscription acknowledgement. The test correlates the exact socket tuple with the kernel transmit queue, physical completion counter, retained response leases and connection-account headroom. Sampling temporarily reserves and immediately releases byte-only credit; it neither supplies time nor changes the account limit. A's applied effects must match C's effects byte-for-byte while B's kernel writes remain stalled. Only B may exhaust its account; A/C and a replacement attachment must continue.

The driver encodes native requests and validates them with the production codec. It is not a generated TypeScript-client test. The separate native GUI selection covers that participant. Per-sample evidence survives assertion failures under `target/transport-proofs/`; regression logs retain startup failures. This socket-queue assertion does not claim non-Linux coverage.

## Worker receiver boundary

`drivers/browser-observer-delivery.ts` retains the original blocked-then-resumed receiver scenario and adds termination of an actual dedicated receiving Worker before queued output can drain. A and C keep running on independent ports of the same production runtime Worker. Failure/close must retain every pending delivery until the owner explicitly disposes the terminated endpoint. Replacement attachment, close from inside an effect callback, its subsequent exact completion, and rejection of an old-connection ACK on a new endpoint are also exercised.

The reentrant-close case holds the receiving thread inside the real callback for 250 ms while the independent Host continues running. The test requires actual later queued output rather than treating the delay as readiness. The callback's exact delivery must complete successfully, decrement the native pending count by one and remove only its payload from the ledger; later autonomous output may still be outstanding in that historical record. Complete connection drain is sampled and verified separately after transport closure but before explicit owner disposal. No production event reordering or clock advancement is injected.

The receiver fixture can bootstrap a diagnostic wrapper around the unchanged shipped runtime Worker. It delegates WASM instantiation and every intercepted export, reads the existing native pending-delivery count, and records detached buffers only after the actual transferable `postMessage`. A separate bounded diagnostic channel samples this state; it does not inject clock ticks, acknowledgements, GUI commands or substitute outputs. The payload ledger measures transferred bytes retained by outstanding deliveries, not all account metadata or WASM heap usage. Native pending counts and disposal results come from the actual runtime.

Both development and production-minified consumer/receiver bundles run the termination scenario. They use the same target-generated client and assembled runtime Worker/WASM artifacts; there is no claim of two different runtime compilations. Exact build identities, delivery traces and failure artifacts are retained by the browser environment under `target/integration-artifacts/`. The suite is headless semantic/transport evidence, not rendered-pixel, whole-GUI, scaling or performance evidence.
