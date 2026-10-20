# IPP terminal client

A client-owned Ghostty terminal rendered with ordinary IPP Canvas components. BashKit and headless Ghostty share one dedicated browser Worker, separate from the IPP Host Worker. The [GUI architecture](../../docs/architecture/gui.md#client-managed-content) owns the boundary.

`startBrowserTerminal` opens a normal transferable endpoint on an existing browser Host. Bootstrap supplies the target World/entity, matching generated client URL and immutable assets. Every display batch, input record and layout observation then uses the standard protocol. No SharedArrayBuffer or cross-origin isolation is required. `TerminalSession` (from `@ipp/terminal/headless`) also accepts an already connected generated client, keeping authoring independent of transport selection.

Applications explicitly create a `GuiInputTarget` with a positive, explicit `GuiLayout` width and height, start the shell and close it before closing the Host. The terminal observes those fields for resize. Core owns focus, picking and capture. Ctrl+Shift+Escape releases keyboard focus even when the terminal captures Tab and Escape. Only one client may bind input to an exact target lifetime.

Ghostty owns terminal state and encodes keys, paste, mouse input and replies. Changed rows become packed `CanvasGlyphRun` components, background spans and cursor paint. The adapter retains at most 160 columns by 80 rows and uses the bundled monospaced font's original glyph identities. It supports foreground/background ANSI colours, inverse/faint/hidden text, selection and basic underline/strike/overline. Bold/italic font variants, animated blinking, ligature shaping and clipboard copying of terminal selections are not implemented. The renderer does not promise glyph coverage beyond the supplied font.

Set `transparentBackground: true` to show the surrounding Surface behind ordinary terminal text. The cockpit uses this option; coloured cell backgrounds, selection and cursor paint remain visible.

BashKit is a virtual shell with an in-memory filesystem, shell pipelines and built-in full-screen tools; it does not execute arbitrary native programs. Input/output and interpreter work have explicit bounds. Exceeding a bound ends the session with an error rather than silently dropping bytes. BashKit has no public terminal disposal method, so the browser owner always terminates the Worker after closing the protocol connection.

The pinned dependencies are `@everruns/bashkit-wasm` 0.19.0 and `@gespenst/core` 0.1.2. Only Gespenst's headless API is used; IPP supplies rendering. The build verifies the packaged Ghostty Wasm checksum and retains licenses. Both Wasm modules and the Worker bundle are measured in `target/terminal-build/build-report.json`; they add no runtime dependency to `ipp-core`.

Build the static example or run the maintained real-runtime checks:

```sh
python tools/ipp.py build terminal
python tools/ipp.py test terminal --software
```

Serve `target/terminal-build/` over HTTP to try the [standalone example](../../examples/browser-terminal/README.md). The browser cockpit uses the same package and worker. Transport-independent engine and browser scenarios live in [tests/terminal](../../tests/terminal).
