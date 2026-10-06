# GUI composites through real input

This family drives the GUI's [controls and composites](../../../docs/architecture/gui.md#controls-and-composites) and the React kit's composites with real input on both hosts: Playwright's pointer and keyboard reach the canvas, the reusable [canvas adapter](../../../packages/ipp-react/src/gui/platform/input.ts) relays them through the Host's physical ingress, clients act through their generated batch clients, and cases assert routing outcomes, committed fields, focus, active items, momentary effects, application callbacks and captured pixels. The worker host runs WASM/WebGL in Chromium; the native host runs GLES behind a WebSocket with its completed-presentation channel.

```sh
python tools/ipp.py test gui-composites --software --egl-dir /lib64
```

## Parts

The family is split into parts. Each part runs once per host in a browser of its own against a fresh Host, with its own canvas, panel Worlds and budget, so no part depends on state another left and a slow part fails alone. Each is a suite command of its own (`<part>-webgl` and `<part>-gles`), which the pipeline schedules side by side. Cases within a part run in order and may build on each other.

| Part | Covers |
| --- | --- |
| `foundations` | client focus, blur and traversal across Worlds; context requests |
| `sliders` | the vertical slider, the slider wheel in a scroll view and the dial |
| `range` | a range's thumbs as focus stops, their keys, drags and writes |
| `groups` | segmented and tab groups; an option list's active item and Escape |
| `number` | numeric entry, rejection, Escape, keys and step parts on the Host clock |
| `overlays` | light, modal and hint overlays, with focus entry and return |
| `toast` | the kit's toast stack over a focused panel, paused and dismissed on the Host clock |
| `kit-choice` | the kit's radio group, tab strip and tree view |
| `kit-overlays` | the kit's context menu, confirmation dialog, popover and tooltip |
| `kit-select` | the kit's dropdown, searchable dropdown, multi-select and autocomplete |
| `kit-values` | the kit's range slider, knob and numeric stepper |
| `colour` | the colour control's focus parts, keys, drags and translucency over the checker |
| `kit-colour` | the kit's colour picker: hex entry, hue drag, invalid hex and presets |

The scenario files describe each case's steps and assertions.

## Harness

- [composite-tests.ts](support/composite-tests.ts), the Node side, registers a part's cases as one test per host, launches the hosts and the browser, loads the part's page module into the page and runs its cases. `run.case` names, times and reports each case; `run.step` calls a step of the page; `run.capture` keeps a capture. Every run keeps `outcomes.json`, `timing.json` (setup and each case's seconds) and its captures with the browser environment's evidence, also when a case fails.
- [composites.ts](pages/composites.ts), the page side, opens the parent canvas and the part's panel Worlds at their origins, attaches the input relay and provides the steps every part shares: points and boxes of named controls, client focus and blur, focus and native text waits, effects since a cut, routing outcomes, values, scrolling, selection, active items, overlays and captures.
- [panels.ts](pages/panels.ts) declares the panels built by commands; `pages/kit-*-panel.ts` declare the kit's panels with React roots.

Reads that cross Worlds are taken together. Two Worlds can report focus at once for a moment: reads answered on either side of the frame that moves focus between them, or a client's focus in one World before the context adopts it and blurs the other at its next routing boundary. `focus` takes such a read again after a frame and fails only when several Worlds still hold focus five frames later.

## Host-clock timing

Delays and repeats count the Host frame deltas a World receives, so they are measured on that clock, never by the wall clock or a sleep. Effects and lifecycle value records carry the tick at which they happened; every frame event and inspection answered for a panel records that tick's World time. A delay is the time between two ticks: `hostInterval` returns the least and the most it can be from the samples around them, and a runtime that waits its delay can only produce `most` at least that delay, however loaded the machine. Under ordinary load the samples fall on the ticks themselves and both bounds agree. Waits on the Host clock, such as the toast's paused time, read World time from inspections.

## Adding a part

1. Write `pages/<part>.ts`: its panels as `PanelSpec`s with origins, `CANVAS` from them, and `prepare`, which opens the page and returns `page.steps` with any steps of its own. Re-export the transports from `pages/composites.ts`.
2. Write `<part>.test.ts`: `compositeTests<typeof prepare>("<part>", { budget: 60 }, async (run) => …)` with its cases in `run.case`.
3. Add the part to `PARTS` in [build.mjs](build.mjs) and its `<part>-webgl` and `<part>-gles` commands to the `gui-composites` suite in [suites.json](../../../tools/pipeline/suites.json).

Run one part alone through its pipeline step, which builds its prerequisites:

```sh
python tools/ipp.py regression --only test:gui-composites:<part>-webgl --software
```

With the prerequisites built (`python tools/ipp.py build native gles-host browser:render font-assets gui-composites-fixtures`), `node --test --test-name-pattern "worker WASM/WebGL" target/gui-composites/<part>.test.js` runs it directly on software rendering, and `--test-name-pattern "native WebSocket/GLES"` with `IPP_EGL_LIBRARY_DIR` set runs it on the native host.

## Budgets

A part's budget bounds its setup and cases on one host; launching and closing the browser and Host have their own allowance. A part takes at most a sixth of its budget alone under software rendering, so a machine running several suites at once still passes; today's parts take 2 to 8 seconds against 60. A part that outgrows this is split, not given a larger budget.
