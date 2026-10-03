/**
 * The gallery dashboard's tools beyond the first panels: the workbench's
 * tabs and the CONTROLS tab's value and selection controls, the SCOPE
 * popover, the TELEMETRY scene tree, FIND and the COLOUR tab's picker,
 * each through real routed input and checked by its effect on the panel,
 * the scope or the 3D scene; and the exploded planes the overlays take.
 *
 * Controls the independent restatement places (the tabs, the TELEMETRY view
 * choice, FIND, the SCOPE trigger) are pressed where it puts them, and their
 * evaluated bounds must agree with it. Controls inside the CONTROLS tab's
 * scrolling column and the overlays' rows are pressed at the bounds the
 * runtime reports, which change with scrolling; their outcome is what the
 * checks assert.
 */
import type { Inspection } from "@ipp/client";
import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import { runBrowserEnvironment } from "../browser/environment.js";
import { galleryEnvironment, openGallery } from "./gallery-driver.js";
import {
  LAYERS,
  OVERLAYS,
  PANEL,
  control,
  projectContent,
  type ContentRect,
  type LogicalRect,
} from "./gallery-gui-panel.js";
import {
  PIXEL_COVERAGE_CANVAS_SHARE,
  dynamicProperty,
  SKIN_SETTLE_MS,
  assertPlanes,
  awaitStationIdle,
  fieldsWith,
  obliquePanel,
  toggleLayers,
  waitForGuiState,
  type Gallery,
  type RegionStats,
} from "./gallery-gui-support.js";
import type {
  GalleryGuiControl,
  GalleryGuiState,
  GalleryWaveform,
} from "./viewer-browser-helper.js";

const environment = (name: string) => ({
  ...galleryEnvironment,
  evidenceParent: resolve(`target/integration-artifacts/gallery-gui/${name}`),
});

/** Open the GUI page and wait for the station's first sync, toasts gone. */
async function openDashboard(
  scenario: Parameters<Parameters<typeof runBrowserEnvironment>[3]>[0],
) {
  const g = await openGallery(scenario, {
    initialPage: "gui",
    canvasShare: PIXEL_COVERAGE_CANVAS_SHARE,
  });
  await g.page.waitForFunction(
    () =>
      document.querySelector<HTMLOutputElement>("#status")?.dataset.state ===
      "ready",
  );
  await awaitStationIdle(g);
  return g;
}

/** Press at the centre of a canvas rectangle, on the plane `depth` metres out. */
async function pressAt(g: Gallery, rect: ContentRect, depth = 0) {
  const [point] = await projectContent(
    g,
    [[rect[0] + rect[2] / 2, rect[1] + rect[3] / 2]],
    depth,
  );
  await g.page.mouse.click(point!.clientX, point!.clientY);
}

/** The one control `predicate` names. */
function find(
  state: GalleryGuiState,
  predicate: (control: GalleryGuiControl) => boolean,
  description: string,
): GalleryGuiControl {
  const matches = state.controls.filter(predicate);
  assert.equal(matches.length, 1, `expected one ${description}`);
  return matches[0]!;
}

const bySymbol = (symbol: string) => (control: GalleryGuiControl) =>
  control.symbol === symbol;

const overlayOpen = (state: GalleryGuiState, symbol: string) =>
  state.overlays.find((overlay) => overlay.symbol === symbol)?.visible === true;

const sidebar = (g: Gallery, selector: string) =>
  g.page.locator(selector).textContent();

/** A custom material's `energy`, one of its dynamic properties. */
function energy(inspection: Inspection, symbolicId: string): number {
  const value = dynamicProperty(inspection, symbolicId, "energy");
  assert.equal(value.kind, "f32", `${symbolicId}'s energy is not a number`);
  return Number(value.value);
}

/** Agree with the restatement to within rounding. */
function assertAt(control: GalleryGuiControl, expected: ContentRect) {
  control.bounds.forEach((value, axis) =>
    assert.ok(
      Math.abs(value - expected[axis]!) < 0.05,
      `${control.symbol} lies at ${JSON.stringify(control.bounds)}, not ${JSON.stringify(expected)}`,
    ),
  );
}

/** A label's ink region inside a control rectangle. */
function labelRegion([x, y, width, height]: ContentRect): LogicalRect {
  return [x + 12, y + 10, x + width - 12, y + height - 10];
}

/** Text in the accent (cyan) rather than the text colour (white). */
function accentText(stats: RegionStats): boolean {
  return stats.max[0] < 170 && stats.max[1] > 200;
}

test("Gallery GUI's workbench tabs, value controls, list and popover drive the scene", {
  timeout: 240_000,
}, async (context) => {
  await runBrowserEnvironment(
    "GUI workbench",
    environment("workbench"),
    context.signal,
    async (scenario) => {
      const g = await openDashboard(scenario);
      const text = (selector: string) => sidebar(g, selector);

      // The tabs hug their labels where the restatement puts them; the
      // selected one shows its label in the accent. A press on CONTROLS
      // selects it, and its label takes the accent from NODES.
      const tabs = await waitForGuiState(g);
      (["NODES", "CONTROLS", "COLOUR"] as const).forEach((name, index) =>
        assertAt(
          control(tabs, { role: "button", name }),
          PANEL.tab(index as 0 | 1 | 2),
        ),
      );
      await g.page.mouse.move(1, 1);
      await new Promise((resolve) => setTimeout(resolve, SKIN_SETTLE_MS));
      await g.capture("workbench-nodes");
      await pressAt(g, PANEL.tab(1));
      await g.page.waitForFunction(
        () => document.querySelector("#gui-tab")?.textContent === "controls",
      );
      await g.page.mouse.move(1, 1);
      await new Promise((resolve) => setTimeout(resolve, SKIN_SETTLE_MS));
      await g.capture("workbench-controls");
      const labels = {
        nodes: labelRegion(PANEL.tab(0)),
        controls: labelRegion(PANEL.tab(1)),
      };
      const [before, after] = await Promise.all(
        ["workbench-nodes", "workbench-controls"].map((label) =>
          g.call<Record<keyof typeof labels, RegionStats>>(
            "galleryGuiRegionStats",
            label,
            labels,
          ),
        ),
      );
      await scenario.evidence.record("tab-labels", { before, after });
      assert.ok(accentText(before!.nodes), "NODES is not shown selected");
      assert.ok(!accentText(before!.controls), "CONTROLS is shown selected");
      assert.ok(accentText(after!.controls), "CONTROLS is not shown selected");
      assert.ok(!accentText(after!.nodes), "NODES is still shown selected");

      // BEAM: a relative drag up the knob raises the value; its paired
      // stepper shows it, and the beam's energy follows.
      const beamBefore = energy(await g.inspect(), "gui-projector-beam");
      const controls = await waitForGuiState(g);
      const knob = control(controls, { role: "slider", name: "BEAM" });
      const [dial] = await projectContent(g, [
        [
          knob.bounds[0] + knob.bounds[2] / 2,
          knob.bounds[1] + knob.bounds[3] / 2,
        ],
      ]);
      await g.page.mouse.move(dial!.clientX, dial!.clientY);
      await g.page.mouse.down();
      await g.page.mouse.move(dial!.clientX, dial!.clientY - 60, {
        steps: 8,
      });
      await g.page.mouse.up();
      const raised = await waitForGuiState(g, (state) => {
        const value = control(state, { role: "slider", name: "BEAM" }).value;
        return value.kind === "scalar" && value.value > 100;
      });
      const beam = control(raised, { role: "slider", name: "BEAM" }).value;
      assert.equal(beam.kind, "scalar");
      const percent = beam.kind === "scalar" ? beam.value : Number.NaN;
      await g.page.waitForFunction(
        (value) =>
          document
            .querySelector("#gui-tuning")
            ?.textContent?.startsWith(`beam ${value}%`) ?? false,
        percent,
      );
      const stepper = find(
        await waitForGuiState(g),
        (candidate) =>
          candidate.kind === "text" &&
          (candidate.symbol ?? "").startsWith("gui-beam/input"),
        "BEAM stepper field",
      );
      assert.deepEqual(stepper.value, { kind: "scalar", value: percent });
      // The child controls and DOM readout can commit before the parent
      // World's separate React root acknowledges the projected energy.
      const projected = await g.waitFor(
        (inspection) =>
          Math.abs(
            energy(inspection, "gui-projector-beam") / beamBefore -
              percent / 100,
          ) < 1e-3,
      );
      const beamAfter = energy(projected, "gui-projector-beam");
      assert.ok(
        Math.abs(beamAfter / beamBefore - percent / 100) < 1e-3,
        `the beam's energy went from ${beamBefore} to ${beamAfter} at ${percent}%`,
      );

      // The sweep band crosses the whole scope with each scan loop at the
      // first SWEEP range: near the scope's left edge early in the loop and
      // near its right edge late in it. The sweep and scan clips hold at the
      // Host times the test seeks, so only the band moves between the two
      // captures: per column of the scope, the light the early capture has
      // over the late one peaks at the early band and dips at the late one.
      const motion = await g.call<GalleryWaveform>("galleryWaveform");
      const sweeping = motion.controllers.find(({ description }) =>
        description.drivers.some(
          ({ property }) => "name" in property && property.name === "phase",
        ),
      );
      assert.ok(sweeping, "the sweep band has no controller");
      assert.equal(sweeping.state, "playing");
      const held = [sweeping.id, motion.scan.controller!.id];
      await g.call("controlGalleryAnimation", held, { action: "pause" }, true);
      await g.page.mouse.move(1, 1);
      const [scopeX, scopeY, scopeWidth, scopeHeight] = PANEL.waveform;
      const columns = 48;
      const rows = 12;
      const scopePoints = Array.from(
        { length: rows * columns },
        (_, index) =>
          [
            scopeX + (scopeWidth * ((index % columns) + 0.5)) / columns,
            scopeY + (scopeHeight * (Math.floor(index / columns) + 0.5)) / rows,
          ] as const,
      );
      // Mean light, the sum of the channels, of each column at a share of
      // the 2.4 s scan loop.
      const sweepAt = async (label: string, share: number) => {
        const time = share * 2.4;
        await g.call(
          "controlGalleryAnimation",
          sweeping.id,
          { action: "seek", time },
          true,
        );
        const seeked = (
          await g.call<GalleryWaveform>("galleryWaveform")
        ).controllers.find(({ id }) => id === sweeping.id);
        assert.ok(
          seeked?.state === "paused" && Math.abs(seeked.time - time) < 1e-6,
          `the sweep did not hold at ${time} s: ${seeked?.state} ${seeked?.time}`,
        );
        await g.capture(label);
        const samples = await g.call<readonly (readonly number[])[]>(
          "sampleGalleryGuiCapture",
          label,
          scopePoints,
        );
        return Array.from({ length: columns }, (_, column) => {
          let light = 0;
          for (let row = 0; row < rows; row++) {
            const [red, green, blue] = samples[row * columns + column]!;
            light += red! + green! + blue!;
          }
          return light / rows;
        });
      };
      const early = await sweepAt("sweep-early", 0.05);
      const late = await sweepAt("sweep-late", 0.95);
      await g.call("controlGalleryAnimation", held, { action: "play" }, true);
      const lift = early.map((light, column) => light - late[column]!);
      const earlyColumn = lift.indexOf(Math.max(...lift));
      const lateColumn = lift.indexOf(Math.min(...lift));
      const travel = {
        early: (earlyColumn + 0.5) / columns,
        late: (lateColumn + 0.5) / columns,
        lift: [lift[earlyColumn]!, lift[lateColumn]!],
      };
      await scenario.evidence.record("sweep-travel", travel);
      assert.ok(
        travel.early < 0.15 &&
          travel.late > 0.85 &&
          travel.lift[0]! > 100 &&
          travel.lift[1]! < -100,
        `the sweep band does not cross the scope edge to edge: ${JSON.stringify(travel)}`,
      );

      // SWEEP: the upper thumb dragged from the end of the rail to its
      // middle narrows the band's travel to the scope's left half, which the
      // scope paint takes. The column scrolls to its end first, which shows
      // SWEEP and RATE.
      await g.call(
        "galleryGuiAction",
        { role: "scrollView", name: "CONTROLS" },
        { kind: "scrollTo", offset: [0, 10_000] },
      );
      const range = control(await waitForGuiState(g), {
        role: "slider",
        name: "SWEEP",
      });
      const [x, y, width, height] = range.bounds;
      const edge = 0.75 * Math.min(width, height);
      const thumb = (value: number) =>
        x + edge / 2 + (value / 100) * (width - edge);
      const [from, to] = await projectContent(g, [
        [thumb(100), y + height / 2],
        [thumb(50), y + height / 2],
      ]);
      await g.page.mouse.move(from!.clientX, from!.clientY);
      await g.page.mouse.down();
      await g.page.mouse.move(to!.clientX, to!.clientY, { steps: 8 });
      await g.page.mouse.up();
      await g.page.waitForFunction(
        () =>
          document
            .querySelector("#gui-tuning")
            ?.textContent?.includes("sweep 0-50%") ?? false,
      );

      // RATE: the list opens from its trigger on the anchored plane; FAST
      // closes it, logs the rate and doubles the scan's and sweep's speed.
      const scrolled = await waitForGuiState(g);
      const rate = find(scrolled, bySymbol("gui-rate"), "RATE trigger");
      await pressAt(g, rate.bounds as unknown as ContentRect);
      const listed = await waitForGuiState(g, (state) =>
        overlayOpen(state, OVERLAYS.rate),
      );
      const fast = find(
        listed,
        (candidate) =>
          candidate.overlay === OVERLAYS.rate && candidate.label === "FAST",
        "FAST option",
      );
      await pressAt(g, fast.bounds as unknown as ContentRect);
      await waitForGuiState(
        g,
        (state) =>
          !overlayOpen(state, OVERLAYS.rate) &&
          (state.eventLog.items[0]?.text ?? "").endsWith("SCAN RATE FAST"),
      );
      assert.match((await text("#gui-tuning")) ?? "", /, fast$/);
      const waveform = await g.call<GalleryWaveform>("galleryWaveform");
      assert.equal(waveform.scan.controller?.description.speed, 2);
      const sweep = waveform.controllers.filter((candidate) =>
        candidate.description.drivers.some(
          (driver) =>
            "name" in driver.property && driver.property.name === "phase",
        ),
      );
      assert.deepEqual(
        sweep.map(({ description }) => description.speed),
        [2],
      );

      // SCOPE: the popover opens from the monitor's header; SCANLINES
      // changes the scope paint's pattern; a press outside closes it and
      // reaches nothing beneath it.
      assertAt(
        control(await waitForGuiState(g), { role: "button", name: "SCOPE" }),
        PANEL.scope,
      );
      await pressAt(g, PANEL.scope);
      const popover = await waitForGuiState(g, (state) =>
        overlayOpen(state, OVERLAYS.scope),
      );
      // An option is its radio mark and, beside it, its label, a second
      // Button as wide as its word. A press on the word's last letters,
      // past the mark's width, selects the option.
      const option = (part: "mark" | "label") =>
        control(popover, {
          role: "button",
          symbol: `gui-scope-grid/scanlines/${part}`,
        }).bounds;
      const mark = option("mark");
      const word = option("label");
      assert.ok(
        word[0] >= mark[0] + mark[2] && word[2] > 2 * mark[2],
        `SCANLINES label ${word.join(",")} beside mark ${mark.join(",")}`,
      );
      await pressAt(g, [
        word[0] + word[2] * 0.6,
        word[1],
        word[2] * 0.3,
        word[3],
      ]);
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-scope")?.textContent ===
          "scanlines, sweep on",
      );
      const command = await text("#gui-command");
      await pressAt(g, PANEL.pulse);
      await waitForGuiState(g, (state) => !overlayOpen(state, OVERLAYS.scope));
      assert.equal(
        await text("#gui-command"),
        command,
        "the press that closed the popover reached PULSE",
      );

      // In the exploded view the panels stay whole on the base plane and an
      // open list floats on the anchored plane above its trigger; a press
      // there picks an option.
      await obliquePanel(g);
      try {
        const trigger = control(await waitForGuiState(g), {
          role: "button",
          symbol: "gui-rate",
        });
        await pressAt(g, trigger.bounds as unknown as ContentRect);
        const open = await waitForGuiState(g, (state) =>
          overlayOpen(state, OVERLAYS.rate),
        );
        const slow = find(
          open,
          (candidate) =>
            candidate.overlay === OVERLAYS.rate && candidate.label === "SLOW",
          "SLOW option",
        );
        await g.page.mouse.move(1, 1);
        await new Promise((resolve) => setTimeout(resolve, SKIN_SETTLE_MS));
        const flat = await g.capture("workbench-list-flat");
        await toggleLayers(g, true);
        if (!overlayOpen(await waitForGuiState(g), OVERLAYS.rate))
          await pressAt(g, trigger.bounds as unknown as ContentRect);
        await waitForGuiState(g, (state) => overlayOpen(state, OVERLAYS.rate));
        await g.page.mouse.move(1, 1);
        await new Promise((resolve) => setTimeout(resolve, SKIN_SETTLE_MS));
        const exploded = await g.capture("workbench-list-exploded");
        const [sx, sy, sw, sh] = slow.bounds;
        const shifts = await assertPlanes(
          g,
          { label: "workbench-list-flat", frame: flat.frame },
          { label: "workbench-list-exploded", frame: exploded.frame },
          {
            panel: {
              // The gain readout's display digits.
              at: [
                PANEL.gainReadout[0] + 20,
                PANEL.gainReadout[1] + PANEL.gainReadout[3] / 2,
              ],
              plane: LAYERS.panel,
            },
            list: {
              // The SLOW option's label.
              at: [sx + 32, sy + sh / 2],
              plane: LAYERS.anchored,
              radius: 14,
            },
          },
        );
        await scenario.evidence.record("list-planes", shifts);
        await pressAt(g, [sx, sy, sw, sh], LAYERS.anchored * LAYERS.spacing);
        await g.page.waitForFunction(
          () =>
            document
              .querySelector("#gui-tuning")
              ?.textContent?.endsWith(", slow") ?? false,
        );
        await toggleLayers(g, false);
      } finally {
        await g.call("releaseGalleryGuiTransform");
      }
      assert.deepEqual(g.errors, []);
    },
  );
});

test("Gallery GUI's scene tree, FIND and colour picker name and light the scene", {
  timeout: 240_000,
}, async (context) => {
  await runBrowserEnvironment(
    "GUI scene tools",
    environment("scene-tools"),
    context.signal,
    async (scenario) => {
      const g = await openDashboard(scenario);
      const text = (selector: string) => sidebar(g, selector);

      // SCENE: the TELEMETRY view choice swaps the event log for the scene
      // tree; CORE names itself in the FOCUS readout and lights the
      // projector's rim in the projection colour.
      const resting = await waitForGuiState(g);
      assertAt(
        control(resting, { role: "button", name: "SCENE" }),
        PANEL.telemetryView(1),
      );
      const trimBefore = fieldsWith(
        await g.inspect(),
        "gui-projector-trim",
        "r",
      );
      await pressAt(g, PANEL.telemetryView(1));
      const tree = await waitForGuiState(g, (state) =>
        state.controls.some(({ symbol }) =>
          (symbol ?? "").startsWith("gui-scene-tree/row/core"),
        ),
      );
      const core = find(
        tree,
        (candidate) =>
          candidate.kind === "button" &&
          candidate.symbol === "gui-scene-tree/row/core",
        "CORE row",
      );
      await pressAt(g, core.bounds as unknown as ContentRect);
      await g.page.waitForFunction(
        () => document.querySelector("#gui-focus")?.textContent === "CORE",
      );
      const focused = await waitForGuiState(g);
      assert.equal(
        focused.texts.find(({ symbol }) => symbol === "gui-readout-focus-value")
          ?.text,
        "CORE",
      );
      const trim = fieldsWith(await g.inspect(), "gui-projector-trim", "r");
      assert.notEqual(Number(trim.r), Number(trimBefore.r));
      assert.ok(Number(trim.b) > Number(trim.r), "the rim is not lit in cyan");

      // FIND: typing suggests the nodes whose names hold the text; Enter
      // finds the first, selects it and scrolls the grid's body to it.
      assertAt(control(focused, { role: "text", name: "FIND" }), PANEL.find);
      await pressAt(g, PANEL.find);
      await waitForGuiState(
        g,
        (state) => control(state, { role: "text", name: "FIND" }).focused,
      );
      await g.page.keyboard.insertText("fox");
      await waitForGuiState(g, (state) => overlayOpen(state, OVERLAYS.find));
      await g.page.keyboard.press("Enter");
      await g.page.waitForFunction(
        () =>
          document
            .querySelector("#gui-nodes")
            ?.textContent?.endsWith("foxtrot selected") ?? false,
      );
      const found = await waitForGuiState(g, (state) => {
        const body = state.controls.find(
          ({ symbol }) => symbol === "gui-nodes/body",
        );
        return body?.value.kind === "scroll" && body.value.offset[1] > 0;
      });
      const body = found.controls.find(
        ({ symbol }) => symbol === "gui-nodes/body",
      )!;
      assert.ok(body.scroll);
      assert.equal(
        body.value.kind === "scroll" ? body.value.offset[1] : 0,
        body.scroll.capacity[1],
        "FIND did not scroll the last node into view",
      );

      // COLOUR: a preset swatch sets the projection colour, which the
      // projector's light takes; choosing the AMBER accent sets the accent's.
      await pressAt(g, PANEL.tab(2));
      const picker = await waitForGuiState(g, (state) =>
        state.controls.some(({ label }) => label === "Magenta"),
      );
      const magenta = find(
        picker,
        (candidate) => candidate.label === "Magenta",
        "Magenta preset",
      );
      await pressAt(g, magenta.bounds as unknown as ContentRect);
      await g.page.waitForFunction(
        () => document.querySelector("#gui-colour")?.textContent === "#F4449F",
      );
      const light = fieldsWith(await g.inspect(), "gui-projector-light", "r");
      assert.ok(
        Number(light.r) > Number(light.g) && Number(light.b) > Number(light.g),
        `the projector light is not magenta: ${JSON.stringify(light)}`,
      );
      await pressAt(g, PANEL.amber);
      await g.page.waitForFunction(
        () => document.querySelector("#gui-colour")?.textContent === "#FFC450",
      );
      assert.equal(await text("#gui-accent"), "amber");
      assert.deepEqual(g.errors, []);
    },
  );
});

test("Gallery GUI lifts the dialog and the toasts to their planes and takes presses there", {
  timeout: 240_000,
}, async (context) => {
  // Exploded depth is a plane's id times the spacing, whichever other
  // planes are in use: the dialog and the toasts keep their depths.
  await runBrowserEnvironment(
    "GUI overlay planes",
    environment("overlay-planes"),
    context.signal,
    async (scenario) => {
      const g = await openDashboard(scenario);
      await g.call(
        "galleryGuiAction",
        { role: "checkbox", name: "SCAN" },
        { kind: "toggle" },
      );
      // A toast that stays: CLEAR's, with UNDO.
      await g.call(
        "galleryGuiAction",
        { role: "button", name: "CLEAR" },
        { kind: "press" },
      );
      await waitForGuiState(g, (state) =>
        state.controls.some(({ label }) => label === "Log cleared."),
      );
      const toastText: readonly [number, number] = [
        PANEL.toast(0, 1)[0] + 16 + 13 + 16 + 40,
        PANEL.toast(0, 1)[1] + PANEL.toast(0, 1)[3] / 2,
      ];
      await obliquePanel(g);
      try {
        // The dialog on the dialog plane and the toast on the toast plane.
        const purge = await projectContent(g, [
          [
            PANEL.purge[0] + PANEL.purge[2] / 2,
            PANEL.purge[1] + PANEL.purge[3] / 2,
          ],
        ]);
        await g.page.mouse.click(purge[0]!.clientX, purge[0]!.clientY);
        await waitForGuiState(g, (state) =>
          overlayOpen(state, OVERLAYS.dialog),
        );
        await g.page.mouse.move(1, 1);
        await new Promise((resolve) => setTimeout(resolve, SKIN_SETTLE_MS));
        const flat = await g.capture("planes-dialog-flat");
        await toggleLayers(g, true);
        await g.page.mouse.move(1, 1);
        const exploded = await g.capture("planes-dialog-exploded");
        // The dialog's feature is the amber action's left edge beside
        // Cancel: edges on both axes fix a shift that a line of text, much
        // like itself shifted along the line, leaves loose.
        const action = PANEL.dialog.action;
        const shifts = await assertPlanes(
          g,
          { label: "planes-dialog-flat", frame: flat.frame },
          { label: "planes-dialog-exploded", frame: exploded.frame },
          {
            panel: {
              at: [
                PANEL.gainReadout[0] + 20,
                PANEL.gainReadout[1] + PANEL.gainReadout[3] / 2,
              ],
              plane: LAYERS.panel,
            },
            dialog: {
              at: [action[0], action[1] + action[3] / 2],
              plane: LAYERS.dialog,
            },
            toast: { at: toastText, plane: LAYERS.toast },
          },
        );
        await scenario.evidence.record("dialog-planes", shifts);
        // Input meets each plane nearest first: Cancel at its projection on
        // the dialog's plane answers it.
        await pressAt(g, PANEL.dialog.cancel, LAYERS.dialog * LAYERS.spacing);
        await waitForGuiState(
          g,
          (state) =>
            !overlayOpen(state, OVERLAYS.dialog) &&
            (state.eventLog.items[0]?.text ?? "").endsWith(
              "NODE PURGE CANCELLED",
            ),
        );
        // The toast's close button on the toast's plane dismisses it.
        const close = PANEL.toast(0, 1);
        await pressAt(
          g,
          [close[0] + close[2] - 8 - 32, close[1], 32, close[3]],
          LAYERS.toast * LAYERS.spacing,
        );
        await waitForGuiState(
          g,
          (state) =>
            !state.controls.some(({ label }) => label === "Log cleared."),
        );
        await toggleLayers(g, false);
      } finally {
        await g.call("releaseGalleryGuiTransform");
      }
      assert.deepEqual(g.errors, []);
    },
  );
});
