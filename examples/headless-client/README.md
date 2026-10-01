# Headless client

[main.ts](main.ts) demonstrates an explicit World and generated-client session with the native Host: create entities in an ordered batch, bind a linear driver, observe a runtime frame, edit the source and inspect the driven value. It illustrates how a constraint writes its target's stored value every frame.

Start the native Host as described in the [headless guide](../../docs/development/headless-hosts.md), then run from the repository root:

```sh
python tools/ipp.py dev headless-client ws://127.0.0.1:9231
```

The command builds the matching native contract and strictly checks the example before running; add `--build` to build only. Each observation is one JSON line, with bigint identities represented as decimal strings. The example uses the baseline scalar and linear-driver components. The Host owns time, and batches retain partial effects on failure.

The example creates its own World, opens an authoring session and writes ordinary component state. Closing that session deletes no entities. Cleanup therefore closes the session, destroys only the exact World this invocation created, then closes its Host connection. Existing Worlds and their sessions are unaffected. See the [client guide](../../packages/ipp-client/README.md) for session boundaries.

`python tools/ipp.py regression --suite headless-client` runs the real CLI against the maintained native Host harness. It verifies source and driven values, repeat invocation, and preservation of an independently created World. It requires no browser or renderer.
