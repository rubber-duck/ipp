# Headless client

[main.ts](main.ts) demonstrates a generated-client session with the native Host: create entities in an ordered batch, bind a linear driver, observe a runtime frame, edit the source and inspect authored/effective values. It illustrates how evaluation can change a component's effective value while preserving its authored value.

Start the native Host as described in the [headless guide](../../docs/development/headless-hosts.md), then run from the repository root:

```sh
python tools/ipp.py dev headless-client ws://127.0.0.1:9231
```

The command builds the matching client before running; add `--build` to build only. The example uses the baseline scalar and linear-driver components. The Host owns time, batches retain partial effects on failure, and the example closes its connection in `finally`. See the [client guide](../../packages/ipp-client/README.md) for session boundaries.
