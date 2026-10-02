# Shared Development Host

[Headless hosts](headless-hosts.md) · [Rendering](rendering.md) · [Testing policy](integration-testing.md) · [Build guide](building.md)

The shared development Host is one long-lived native GLES Host that many client processes use at the same time. Each client declares its own Worlds through the generated TypeScript client, presents its own root, pins interaction through real input ingress, captures completed frames as PNG files and can stay connected in an interactive session. It exists so several agents or people doing parallel client-side work, such as GUI skins, React components or data visualisation, share one built and running Host instead of each building and launching their own.

The Host is the [`gles_host`](../../crates/ipp-server/examples/gles_host.rs) testing entry point, unchanged; the [shared-host tool](../../tools/shared-host/cli.ts) adds the commands, the state file, the presentation lock and sessions. It is a development and testing aid: not a production host, and its captures are not regression evidence by themselves.

## When to use it

Use it to iterate on client code against real presentation: edit, capture, look, repeat, in about a second per capture once a session is open. Use it when several agents work on separate client modules at once and each needs rendered output.

Use a per-test Host instead for maintained scenarios and regression: their runners launch, await, record and clean up their own Host (see [the integration harness](headless-hosts.md#maintained-integration-harness)). The shared Host also cannot test a Rust or contract change until its owner restarts it from a checkout containing that change, does not cover browser WebGL, and measures nothing about performance: it is a debug build on a GPU that every client shares.

## Commands

Prepare a checkout once as the [build guide](building.md#pipeline-setup-and-commands) describes, with `python tools/ipp.py setup node`, `python tools/ipp.py setup python` and, to build the Host, `python tools/ipp.py setup rust`; the Host also needs native EGL and GLES libraries ([EGL setup](../../crates/ipp-render-gl/examples/smoke/README.md)). Run the commands from the checkout root; `node tools/shared-host/shared-host.mjs --help` lists them. A first round with the smallest client module, [hello.tsx](../../tools/shared-host/hello.tsx), whose arguments are the text it shows:

```sh
node tools/shared-host/shared-host.mjs host start --egl-dir /lib64   # owner only
node tools/shared-host/shared-host.mjs host status
node tools/shared-host/shared-host.mjs run tools/shared-host/hello.tsx "Some text"
node tools/shared-host/shared-host.mjs session start tools/shared-host/hello.tsx --name greeting
node tools/shared-host/shared-host.mjs session capture greeting "Other text"
node tools/shared-host/shared-host.mjs session list
node tools/shared-host/shared-host.mjs session stop greeting
node tools/shared-host/shared-host.mjs compare target/shared-host-captures/hello/hello.png target/shared-host-captures/greeting/hello.png --zoom-factor 2 --out target/shared-host-captures/pair.png
node tools/shared-host/shared-host.mjs host stop                      # owner only
```

`host start` builds `gles-host` and `font-assets` through the pipeline in the current checkout (skip with `--no-build`), copies the executable and its generated client to a private directory, starts the Host in the background on ephemeral loopback ports and records its state; it refuses to start while that checkout's Host runs. The EGL directory defaults to `IPP_EGL_LIBRARY_DIR`, then `/lib64`. `run MODULE [ARGS...]` opens a client module, serves one request and disconnects; `session start MODULE --name NAME [ARGS...]` keeps it open for `session capture NAME [ARGS...]` (see [interactive sessions](#interactive-sessions)), and `session stop --all`, for the owner, stops every session. The tool takes its own options (`--host`, `--out`, `--name`, `--json`, `--idle-minutes`, and `--egl-dir`, `--no-build` and `--all` where they apply) from anywhere on the command line and passes every other argument to the client module. Images are written to `target/shared-host-captures/<name>/` of the current checkout, named by the session, or for `run` by `--name` or the module's file name, unless `--out DIR` is given; `--json` prints the reply as JSON. `compare` writes two PNG files side by side, each optionally cropped with `--first-box` or `--second-box X,Y,W,H` and enlarged by `--zoom-factor`, to the file `--out` names, `compare.png` in the current directory by default. The launcher runs the `shared-host` pipeline product and rebuilds it with `python tools/ipp.py build shared-host` when one of its sources changed.

## Finding and joining the Host

The Host's checkout records it in `target/shared-host-state/host.json`: process id, WebSocket URL, the commit it was built from, its contract hash, the private copy of executable and generated client, the shared font and the log path. Commands find it there; from another checkout select it with `--host` or `IPP_SHARED_HOST`, naming the Host's checkout, its state directory or its `host.json`. Client modules and captures always come from the checkout whose launcher runs.

Clients connect through the generated client copied with the Host, so a connection never meets a contract mismatch. A client module written against a newer contract, for example using a row field the running Host lacks, fails where it uses that field; ask the owner for a restart from a checkout that has it. `host status` prints the contract hash in use.

## Writing a client module

A client module is a TypeScript or TSX file whose default export is `defineClient({ open, capture, close })` from [client.ts](../../tools/shared-host/client.ts); [hello.tsx](../../tools/shared-host/hello.tsx) is the smallest complete one, and the [skin lab](../../tests/skin-lab/README.md) is a larger one. A module elsewhere imports `defineClient` by its relative path, as the lab's [lab.tsx](../../tests/skin-lab/lab.tsx) does. The module is bundled from source on load, with `react`, `@ipp/client`, `@ipp/react`, `@ipp/react/gui` and `@ipp/react/gui-kit` resolved to the tool's own instances, and `@ipp/host-contract` to the Host's generated client module, so a module can read the runtime's exported values, such as the design language's tokens, when it loads. Bundling does not check types; `python tools/ipp.py check typecheck` checks the modules under the paths the root [tsconfig.json](../../tsconfig.json) includes.

- `open(context, args)` creates the client's Worlds and declarations and returns its state. Create Worlds with `temporary: true` so they end with the connection, include `context.name` in their symbolic ids, and bind the root output to capture with `context.host.setRootOutput`, at the extent and device scale the captures need.
- `capture(state, context, args)` serves one request and returns `{ images, summary, report }`; each image is written as `<key>.png`. `context.capture(binding)` presents a root binding and returns its settled frame. For pinned states, call `context.present(binding, section)` and, inside the section, open a physical input context on the given view with `context.host.input.open(view)`, send input, read a frame with `settled` from [presentation.ts](../../tools/shared-host/presentation.ts) and close the input context before the section ends.
- `close(state, context)` releases what `open` created.

`context.contract` is the Host's generated client module, `context.font()` returns the shared GUI font, and `context.load(path, previous)` bundles another module from source with the same shared instances, returning nothing while it is unchanged; use it for content an open session should pick up, such as theme or data files.

## Concurrent presentation and capture

Every client owns its Worlds and its root bindings, and keeps declaring, editing and evaluating independently. The Host has one presentation surface, which draws exactly one selected root binding at a time, so `context.present` holds the Host-wide presentation lock (the directory `presentation.lock` beside `host.json`, owned by a process id), selects the client's binding, runs the section and clears the selection before releasing the lock. Captures from different clients are therefore serialized; other work is not. A lock whose owner process died is broken by the next client, and waiting gives up after 300 s.

Correctness does not rest on the lock. A frame or capture request names the exact selected view, and the Host fails it with `staleView` when another selection replaced that view, instead of drawing another client's root; the lock only keeps clients from causing those failures. Physical input contexts belong to the selected view, so open and close them inside the section.

Limits:

- The native Host serves at most 8 connections at once (`MAX_CONNECTIONS` in [websocket.rs](../../crates/ipp-server/src/websocket.rs)). Every session and every running `run` or `host status` holds one; a further connection is refused and the command says so.
- The presentation surface is at most 2048 by 2048 pixels.
- A capture waits for two identical consecutive frames and gives up after 240 frames, so continuously animating content does not settle.
- Waits add up: seven sessions capturing at the same moment waited up to about 5.5 s for the lock.
- Renderer state such as the glyph atlas is shared, so identical content can differ by a level at glyph edges between captures, and a heavy World slows every client's frames.

## Interactive sessions

`session start` launches a detached process that opens the module once and serves requests on a loopback port recorded in `target/shared-host-state/sessions/<name>.json`, with its log beside it. Names must be unique on the Host. `session capture` sends one request; before serving it the session bundles the module again and, when the bundle changed, calls the new `capture` with the state the first `open` returned. Keep that state's shape compatible while editing, or stop and start the session after changing what `open` creates. A session keeps the tool code it started with, ends after 120 idle minutes (`--idle-minutes`), and ends when the Host stops.

## Sharing rules

- One owner starts and stops the Host and says so where the participants coordinate, such as the Beads task. Nobody else restarts, rebuilds into or stops it, or stops sessions they did not start.
- Rebuilding `target/gles-host` never affects the running Host. A runtime or contract change reaches clients only when the owner runs `host stop` and `host start` from a checkout containing it.
- A restart closes every connection, destroys every temporary World and ends every session; requests in flight fail. Participants start their sessions again afterwards.
- When done, stop your sessions. The owner stops the Host when the work ends; only files under `target/` remain.

Images from the shared Host are development evidence. Behaviour a change needs to keep belongs in a maintained scenario under the [testing policy](integration-testing.md), registered with the pipeline and run by its own environment.
