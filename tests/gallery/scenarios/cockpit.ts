import assert from "node:assert/strict";
import type {
  CameraViewMotion,
  DatasetPage,
  Inspection,
  WorldReference,
} from "@ipp/client";
import type { RgbaImage } from "../../../tools/shared-host/images.js";
import {
  assertCylinder,
  assertFlat,
  COCKPIT_ANCHORS,
  type CockpitViewport,
} from "../cockpit-oracle.js";
import { assertFrameRateData } from "./frame-rate.js";
import { exerciseCockpitLights } from "./cockpit-lights.js";

export interface CockpitState {
  readonly options: Readonly<Record<string, unknown>>;
  readonly scene: Inspection;
  readonly panels: readonly Inspection[];
  readonly panelWorlds: readonly WorldReference[];
  readonly componentIds: Readonly<Record<string, number>>;
  readonly navigation: string;
}

/** A real application driver supplies input, completed images and World state. */
export interface CockpitTrackingEvidence {
  samples: {
    target: number;
    from: number;
    pose: number;
    observationMs: number;
    velocity: number | undefined;
    mode: string | undefined;
  }[];
  handlerMs: number[];
  viewport: readonly [number, number];
  controller: string;
  retainedController: string | undefined;
  target: number;
  converged: number;
  released: number;
}

export interface CockpitDriver {
  cameraViewport(): Promise<CockpitViewport>;
  tracking(): Promise<CockpitTrackingEvidence>;
  history(): Promise<DatasetPage>;
  contentHeight(panel: number, height: number | null): Promise<void>;
  text(
    panel: number,
    id: string,
    value: string,
    submit?: boolean,
  ): Promise<void>;
  state(): Promise<CockpitState>;
  options(patch: Readonly<Record<string, unknown>>): Promise<void>;
  drag(
    panel: number,
    id: string,
    distance: number,
    fast?: boolean,
  ): Promise<readonly CockpitState[]>;
  press(panel: number | "shell", id: string, fraction?: number): Promise<void>;
  capture(name: string): Promise<RgbaImage>;
  action(name: string, value?: unknown): Promise<unknown>;
  advance(seconds: number): Promise<void>;
  navigate(motion?: CameraViewMotion): Promise<void>;
}

function entity(state: Inspection, id: string) {
  const result = state.entities.find(
    (value) => value.metadata.symbolicId === id,
  );
  assert.ok(result, `${id} exists in its owning World`);
  return result;
}

function fields(state: Inspection, id: string, component: number) {
  const result = entity(state, id).components.find(
    (value) => value.component === component,
  );
  assert.ok(result, `${id} has component ${component}`);
  return result.fields;
}

function phase(state: CockpitState) {
  const value = entity(state.scene, "cockpit-planet").components.find(
    (component) => component.component === state.componentIds.CustomMaterial,
  )?.properties?.phase;
  assert.equal(value?.kind, "f32");
  return value!.value;
}

function visible(frame: RgbaImage) {
  let blueInk = 0;
  let light = 0;
  let dark = 0;
  let fallbackRed = 0;
  for (let index = 0; index < frame.pixels.length; index += 4) {
    const r = frame.pixels[index]!;
    const g = frame.pixels[index + 1]!;
    const b = frame.pixels[index + 2]!;
    if (b > r + 20 && g > 70) blueInk++;
    if (r + g + b > 540) light++;
    if (r + g + b < 120) dark++;
    if (r > 240 && g < 16 && b < 16) fallbackRed++;
  }
  assert.ok(
    fallbackRed < 256,
    "custom material programs render without solid-red fallback",
  );
  assert.ok(
    blueInk > 500,
    "completed frame contains instrument and telemetry ink",
  );
  assert.ok(light > 500, "completed frame contains readable bright controls");
  assert.ok(
    dark > 500,
    "completed frame retains the dark cockpit and space backdrop",
  );
}

function difference(
  first: RgbaImage,
  second: RgbaImage,
  label: string,
  minimumY = 0,
) {
  assert.equal(first.width, second.width);
  assert.equal(first.height, second.height);
  let changed = 0;
  for (
    let index = Math.floor(first.height * minimumY) * first.width * 4;
    index < first.pixels.length;
    index += 4
  ) {
    const delta =
      Math.abs(first.pixels[index]! - second.pixels[index]!) +
      Math.abs(first.pixels[index + 1]! - second.pixels[index + 1]!) +
      Math.abs(first.pixels[index + 2]! - second.pixels[index + 2]!);
    if (delta > 24) changed++;
  }
  assert.ok(
    changed > 150,
    `${label} changes visible completed pixels (${changed})`,
  );
}

function exteriorDifference(
  first: RgbaImage,
  second: RgbaImage,
  category: string,
) {
  let windowChanges = 0;
  let foregroundChanges = 0;
  const starPixels = new Set<number>();
  for (let y = 0; y < first.height; y++)
    for (let x = 0; x < first.width; x++) {
      const index = (y * first.width + x) * 4;
      const delta = [0, 1, 2].reduce(
        (sum, channel) =>
          sum +
          Math.abs(
            first.pixels[index + channel]! - second.pixels[index + channel]!,
          ),
        0,
      );
      if (delta <= 24) continue;
      if (
        category === "stars" &&
        Math.max(
          first.pixels[index]!,
          first.pixels[index + 1]!,
          first.pixels[index + 2]!,
        ) < 8
      )
        starPixels.add(y * first.width + x);
      if (
        y < first.height * 0.62 &&
        x > first.width * 0.05 &&
        x < first.width * 0.95
      )
        windowChanges++;
      // The fixed-size console no longer covers the entire lower viewport.
      // Its central dial deck is opaque; space beside its ends can show stars.
      if (
        y > first.height * 0.85 &&
        y < first.height * 0.95 &&
        x > first.width * 0.3 &&
        x < first.width * 0.7
      )
        foregroundChanges++;
    }
  assert.ok(
    windowChanges > (category === "stars" ? 40 : 150),
    `${category} changes visible window pixels (${windowChanges})`,
  );
  assert.ok(
    foregroundChanges < 100,
    `${category} preserves cockpit pixels (${foregroundChanges})`,
  );
  if (category === "stars") {
    // Isolate stars against empty space, excluding GUI text and the planet.
    // Connected bright regions must remain compact at different output sizes.
    const areas: number[] = [];
    while (starPixels.size) {
      const queue = [starPixels.values().next().value!];
      starPixels.delete(queue[0]!);
      let area = 0;
      while (queue.length) {
        const pixel = queue.pop()!;
        area++;
        const x = pixel % first.width;
        const y = Math.floor(pixel / first.width);
        for (let dy = -1; dy <= 1; dy++)
          for (let dx = -1; dx <= 1; dx++) {
            const nextX = x + dx;
            const nextY = y + dy;
            if (
              nextX < 0 ||
              nextX >= first.width ||
              nextY < 0 ||
              nextY >= first.height
            )
              continue;
            const next = nextY * first.width + nextX;
            if (starPixels.delete(next)) queue.push(next);
          }
      }
      areas.push(area);
    }
    assert.ok(areas.length >= 3, `visible distant stars (${areas.length})`);
    const maximum = Math.max(...areas);
    assert.ok(
      maximum < Math.max(24, 32 * (first.width / 1600) ** 2),
      `stars remain compact points rather than soft blobs (${maximum} pixels)`,
    );
  }
}

export async function exerciseCockpitTracking(driver: CockpitDriver) {
  await driver.options({ carousel: 1 });
  await driver.advance(0.3);
  const evidence = await driver.tracking();
  assert.equal(
    evidence.handlerMs.length,
    12,
    "all held physical moves reach the scene",
  );
  assert.equal(
    evidence.retainedController,
    evidence.controller,
    "held retargeting and release retain the native animation controller",
  );
  assert.ok(
    evidence.samples.every(
      (sample, index) =>
        index === 0 ||
        sample.pose >= evidence.samples[index - 1]!.pose - 0.0001,
    ),
    "held native poses progress monotonically through each burst",
  );
  assert.ok(
    evidence.samples.some(
      (sample) => sample.mode === "hold" && (sample.velocity ?? 0) > 0,
    ),
    "direct manipulation holds a runtime sample and retains gesture velocity",
  );
  assert.ok(
    Math.abs(evidence.converged - evidence.target) < 0.01,
    "held pose converges to the latest pointer without release",
  );
  assert.ok(
    Math.abs(evidence.released - Math.round(evidence.target)) < 0.01,
    "release snaps from the tracked pose to the nearest pane",
  );
  return evidence;
}

export type CockpitPart = "carousel" | "presentation" | "exterior" | "forms";

/** Measure evaluated leaves; layout-only containers need no CanvasBounds. */
function box(state: CockpitState, panel: number, id: string) {
  const value = fields(
    state.panels[panel]!,
    id,
    state.componentIds.CanvasBounds!,
  );
  const bounds = Object.fromEntries(
    ["x", "y", "width", "height"].map((name) => [name, Number(value[name])]),
  ) as Record<"x" | "y" | "width" | "height", number>;
  assert.ok(
    bounds.width > 0 && bounds.height > 0,
    `${id} has a usable evaluated box`,
  );
  return bounds;
}

function assertForms(state: CockpitState, tab: "scene" | "gui") {
  const pairs =
    tab === "scene"
      ? [
          ["cockpit-illumination/label", "cockpit-illumination/slider"],
          ["cockpit-atmosphere/label", "cockpit-atmosphere/slider"],
          ["cockpit-gain/label", "cockpit-gain/slider"],
          ["cockpit-route-range/label", "cockpit-route-range/field"],
          ["cockpit-charge/label", "cockpit-charge/slider"],
        ]
      : [
          ["cockpit-opacity/label", "cockpit-opacity/slider"],
          ["cockpit-projection/label", "cockpit-projection/slider"],
        ];
  const ids =
    tab === "scene"
      ? [
          "cockpit-illumination/slider",
          "cockpit-lights-playing",
          "cockpit-gain/slider",
          "cockpit-route-range/field",
          "cockpit-autopilot",
          "cockpit-flight-mode/cruise/mark",
          "cockpit-pulse",
        ]
      : [
          "cockpit-skin",
          "cockpit-font",
          "cockpit-opacity/slider",
          "cockpit-surface/flat",
          "cockpit-projection/slider",
          "cockpit-previous",
        ];
  const viewport = box(state, 0, "cockpit-content-0");
  for (const [caption, control] of pairs) {
    const label = box(state, 0, caption!);
    const input = box(state, 0, control!);
    assert.ok(
      input.y - label.y - label.height > 1,
      `${caption} has breathing room above ${control}`,
    );
    assert.ok(
      Math.abs(input.x - label.x) < 1,
      `${control} aligns with its caption`,
    );
  }
  const dimensions = Object.fromEntries(
    ids.map((id) => {
      const measured = box(state, 0, id);
      assert.ok(
        measured.x >= viewport.x - 1 &&
          measured.x + measured.width <= viewport.x + viewport.width + 1,
        `${id} fits horizontally inside its scroll viewport`,
      );
      return [id, { width: measured.width, height: measured.height }];
    }),
  );
  if (tab === "scene") {
    const numeric = box(state, 0, "cockpit-route-range/field");
    assert.ok(
      numeric.width >= 3 * numeric.height &&
        numeric.width <= 6 * numeric.height,
      "numeric entry fits its digits and step buttons",
    );
    const mark = box(state, 0, "cockpit-flight-mode/cruise/mark");
    assert.ok(
      mark.width <= 30 && mark.height <= 30,
      "radio target stays compact beside its label",
    );
    const autopilot = box(state, 0, "cockpit-autopilot");
    assert.ok(
      autopilot.width > autopilot.height * 3,
      "checkbox caption and compact marker share one control",
    );
  }
  return dimensions;
}

function texts(state: CockpitState, panel: number) {
  return state.panels[panel]!.entities.flatMap((entity) =>
    entity.components
      .filter(
        (component) => component.component === state.componentIds.CanvasText,
      )
      .map((component) => String(component.fields.text)),
  );
}

async function exerciseForms(driver: CockpitDriver) {
  const until = async (
    predicate: (state: CockpitState) => boolean,
    reason: string,
  ) => {
    const deadline = performance.now() + 15000;
    for (;;) {
      const state = await driver.state();
      if (predicate(state)) return state;
      assert.ok(performance.now() < deadline, reason);
      await driver.advance(0.03);
    }
  };
  const evidence: {
    skin: string;
    tab: string;
    dimensions: ReturnType<typeof assertForms>;
  }[] = [];
  const worlds = (await driver.state()).panelWorlds;
  for (const skin of ["glass", "cybertron"]) {
    await driver.action("font", "shure-tech-mono");
    const beforeSkin = await driver.state();
    const beforeIds = beforeSkin.panels.map((panel) =>
      panel.entities
        .filter((entity) =>
          entity.components.some(
            (component) =>
              component.component === beforeSkin.componentIds.GuiBehavior,
          ),
        )
        .flatMap((entity) =>
          entity.metadata.symbolicId
            ? [[entity.metadata.symbolicId, entity.id] as const]
            : [],
        ),
    );
    await driver.action("skin", skin);
    const afterSkin = await driver.state();
    for (const [panel, ids] of beforeIds.entries())
      for (const [id, handle] of ids)
        assert.equal(
          entity(afterSkin.panels[panel]!, id).id,
          handle,
          "skin changes retain mounted native controls",
        );
    await driver.options({
      carousel: 0,
      lightsPlaying: false,
      atmospherePlaying: false,
    });
    await driver.advance(0.3);
    await driver.press(0, "cockpit-control-tab/scene");
    const scene = await driver.state();
    evidence.push({
      skin,
      tab: "scene",
      dimensions: assertForms(scene, "scene"),
    });
    const autopilot = Boolean(scene.options.autopilot);
    await driver.press(0, "cockpit-autopilot");
    await until(
      (state) => state.options.autopilot === !autopilot,
      "physical checkbox updates scene state",
    );
    await driver.press(0, "cockpit-gain/slider", 0.25);
    await until(
      (state) => Number(state.options.gain) < 0.5,
      "physical signal slider updates the dashboard input",
    );
    await driver.text(0, "cockpit-route-range/field", "2.4", true);
    await until(
      (state) => Math.abs(Number(state.options.range) - 2.4) < 0.001,
      "numeric entry commits through native editing",
    );
    visible(await driver.capture(`cockpit-forms-${skin}-scene`));
    await driver.press(0, "cockpit-control-tab/gui");
    const gui = await until(
      (state) =>
        state.panels[0]!.entities.some(
          (entity) => entity.metadata.symbolicId === "cockpit-skin",
        ),
      "GUI tab mounts display controls",
    );
    assert.equal(
      fields(
        gui.panels[0]!,
        "cockpit-control-tab/gui",
        gui.componentIds.GuiButton!,
      ).selected,
      true,
      "GUI tab owns native selection",
    );
    evidence.push({ skin, tab: "gui", dimensions: assertForms(gui, "gui") });
    const selectedFont = "noto-sans";
    const originalFont = fields(
      gui.panels[0]!,
      "cockpit-font",
      gui.componentIds.GuiFont!,
    ).source;
    await driver.press(0, "cockpit-font");
    await driver.press(0, `cockpit-font/options/${selectedFont}`);
    const fontState = await until(
      (state) => state.options.font === selectedFont,
      "cockpit font dropdown updates application settings",
    );
    assert.equal(
      entity(fontState.panels[0]!, "cockpit-control-tab/gui").id,
      entity(gui.panels[0]!, "cockpit-control-tab/gui").id,
      "font changes preserve mounted controls",
    );
    assert.notEqual(
      fields(fontState.panels[0]!, "cockpit-font", gui.componentIds.GuiFont!)
        .source,
      originalFont,
      "cockpit controls receive the selected font",
    );
    await driver.press(0, "cockpit-opacity/slider", 0.75);
    await until(
      (state) =>
        Number(state.options.panelOpacity) > 0.8 &&
        Number(state.options.panelOpacity) < 0.95,
      "GUI slider changes presentation state",
    );
    visible(await driver.capture(`cockpit-forms-${skin}-gui`));
    await driver.options({ carousel: 1 });
    await driver.advance(0.3);
    const deadline = performance.now() + 15000;
    let history = await driver.history();
    while (history.rows.length < 3) {
      assert.ok(
        performance.now() < deadline,
        "real presentation samples reach the cockpit graph",
      );
      await driver.advance(0.1);
      history = await driver.history();
    }
    assertFrameRateData(history);
    const system = await driver.state();
    const readouts = texts(system, 1).join("\n");
    assert.match(readouts, /FRAME RATE/);
    assert.match(readouts, /Node \d/);
    assert.match(readouts, /Triangles/);
    assert.match(readouts, /Client platform/);
    assert.doesNotMatch(readouts, /Waiting for presentation diagnostics/);
    const chart = box(system, 1, "cockpit-fps-chart");
    assert.ok(
      chart.width > 600 && Math.abs(chart.height - 180) < 0.1,
      `live FPS graph uses the wider screen: ${JSON.stringify(chart)}`,
    );
    visible(await driver.capture(`cockpit-forms-${skin}-system`));
    await driver.advance(0.6);
    const nextSampleDeadline = performance.now() + 15000;
    let later = await driver.history();
    while (later.rows.at(-1)!.id <= history.rows.at(-1)!.id) {
      assert.ok(
        performance.now() < nextSampleDeadline,
        "FPS dataset streams subsequent completed presentations",
      );
      await driver.advance(0.1);
      later = await driver.history();
    }
    await driver.options({ carousel: 2 });
    await driver.advance(0.3);
    const terminal = await driver.state();
    const terminalText = texts(terminal, 2);
    assert.match(terminalText.join("\n"), /BashKit.*Ghostty/);
    const input = terminal.panels[2]!.entities.find(
      (entity) => entity.metadata.symbolicId === "cockpit-terminal",
    );
    assert.ok(
      input?.components.some(
        (component) =>
          component.component === terminal.componentIds.GuiInputTarget,
      ),
      "terminal exposes a generic client-managed input target",
    );
    assert.ok(
      !terminal.panels[2]!.entities.some((entity) =>
        entity.metadata.classes.includes("terminal-content"),
      ),
      "loading the cockpit does not start a shell",
    );
    visible(await driver.capture(`cockpit-forms-${skin}-terminal`));
    assert.deepEqual(
      (await driver.state()).panelWorlds,
      worlds,
      "tabs and edits retain all six Canvas Worlds",
    );
    await driver.options({ carousel: 0 });
    await driver.advance(0.3);
    await driver.press(0, "cockpit-control-tab/scene");
  }
  for (const tab of ["scene", "gui"]) {
    const first = evidence.find(
      (value) => value.skin === "glass" && value.tab === tab,
    )!;
    const second = evidence.find(
      (value) => value.skin === "cybertron" && value.tab === tab,
    )!;
    for (const [id, dimensions] of Object.entries(first.dimensions))
      for (const axis of ["width", "height"] as const)
        assert.ok(
          Math.abs(dimensions[axis] - second.dimensions[id]![axis]) < 0.1,
          `${id} keeps evaluated ${axis} across skins`,
        );
  }
  return { forms: evidence };
}

export async function exerciseCockpit(
  driver: CockpitDriver,
  part: CockpitPart = "presentation",
) {
  if (part === "forms") return exerciseForms(driver);
  const option = async (name: string, value: unknown) => {
    const deadline = performance.now() + 15_000;
    for (;;) {
      if ((await driver.state()).options[name] === value) return;
      assert.ok(
        performance.now() < deadline,
        `${name} becomes ${String(value)} after physical input`,
      );
      await driver.advance(0.03);
    }
  };
  const initial = await driver.state();
  assert.equal(initial.navigation, "yaw");
  assert.equal(initial.panels.length, 6);
  assert.ok(
    initial.panels.every((panel) =>
      panel.resources.every((resource) => resource.status === "loaded"),
    ),
  );
  const camera = fields(
    initial.scene,
    "cockpit-camera",
    initial.componentIds.Transform!,
  );
  assert.equal(
    initial.options.surface,
    "curved",
    "default presents the shared cylinder",
  );
  assertCylinder(
    initial.scene,
    initial.componentIds,
    Number(initial.options.projection),
  );
  await driver.navigate({ kind: "pan", x: 0.2, y: 0.2 });
  assert.deepEqual(
    fields(
      (await driver.state()).scene,
      "cockpit-camera",
      initial.componentIds.Transform!,
    ),
    camera,
    "cockpit navigation preserves the seated position",
  );
  const identities = initial.panels.map((panel, index) =>
    panel.entities
      .filter(
        (value) =>
          (index !== 0 ||
            /cockpit-(grip|control-tab)/.test(
              value.metadata.symbolicId ?? "",
            )) &&
          value.components.some(
            (component) =>
              component.component === initial.componentIds.GuiBehavior,
          ),
      )
      .map((value) => [value.metadata.symbolicId, value.id]),
  );
  const glass = await driver.capture("cockpit-glass");
  visible(glass);
  for (let panel = 4; panel < 6; panel++) {
    assert.ok(
      !initial.panels[panel]!.entities.some((entry) =>
        entry.components.some(
          (component) =>
            component.component === initial.componentIds.GuiBehavior,
        ),
      ),
      "integrated dashboards contain only animated instrument paint",
    );
  }
  if (part === "carousel") {
    const tracking = await exerciseCockpitTracking(driver);
    await driver.action("projection", 8);
    await driver.advance(0.3);
    const near = await driver.state();
    assert.equal(
      near.options.projection,
      8.8,
      "too-near requests clamp before the dashboard",
    );
    assertCylinder(near.scene, near.componentIds, 8.8);
    await driver.action("projection", Number(initial.options.projection) + 1);
    await driver.advance(0.3);
    const far = await driver.state();
    assertCylinder(
      far.scene,
      far.componentIds,
      Number(initial.options.projection) + 1,
    );
    await driver.action("projection", Number(initial.options.projection));
    await driver.advance(0.3);
    const grab = await driver.drag(1, "cockpit-header-1", 0.13);
    const positions = grab.map((snapshot) => Number(snapshot.options.carousel));
    assert.ok(
      positions.every(
        (value, index) => index === 0 || value < positions[index - 1]!,
      ),
      `header drag moves continuously right: ${positions}`,
    );
    assert.ok(
      positions.some((value) => value % 1 !== 0),
      "drag retains fractional angular positions",
    );
    for (const snapshot of grab)
      assertCylinder(
        snapshot.scene,
        snapshot.componentIds,
        Number(snapshot.options.projection),
      );
    await driver.advance(0.3);
    await driver.options({ carousel: 0 });
    await driver.advance(0.3);
    const widgetPosition = Number((await driver.state()).options.carousel);
    await driver.drag(0, "cockpit-gain/slider", 0.08);
    assert.equal(
      (await driver.state()).options.carousel,
      widgetPosition,
      "widget drag never arms carousel chrome capture",
    );
    await driver.drag(0, "cockpit-header-0", -0.25, true);
    assert.ok(
      Number((await driver.state()).options.carousel) > widgetPosition,
      "fast input preserves pending grab and release",
    );
    await driver.options({ carousel: 1 });
    await driver.advance(0.3);
    for (const skin of ["glass", "cybertron"]) {
      await driver.action("skin", skin);
      for (const focus of [0, 1, 2]) {
        await driver.options({ carousel: focus });
        await driver.advance(0.3);
        visible(await driver.capture(`cockpit-demo-${skin}-${focus}`));
      }
      await driver.options({ carousel: 1 });
      await driver.advance(0.3);

      visible(await driver.capture(`cockpit-front-${skin}`));
    }
    const retained = await driver.state();
    for (let index = 0; index < 5; index++)
      for (const [name, id] of identities[index]!)
        assert.equal(entity(retained.panels[index]!, String(name)).id, id);
    return {
      tracking,
      positions,
      widgetPosition,
      worlds: (await driver.state()).panelWorlds.length,
    };
  }
  if (part === "exterior") {
    const motion = await exerciseCockpitLights(driver);
    await driver.action("scan", false);
    await driver.action("atmospherePlayback", false);
    await driver.action("skin", "glass");
    await driver.options({ carousel: 0 });
    await driver.advance(0.3);
    await driver.press(0, "cockpit-lights-playing");
    await option("lightsPlaying", false);
    const lamps = await driver.capture("cockpit-light-controls");
    await driver.press(0, "cockpit-illumination/slider", 0.2);
    const dimmed = await driver.state();
    assert.ok(Number(dimmed.options.illumination) < 0.6);
    const darkLamps = await driver.capture("cockpit-lights-dimmed");
    difference(lamps, darkLamps, "dashboard emission mask intensity", 0.7);
    await driver.action("illumination", 1);
    await driver.action("cities", 0);
    await driver.action("stars", 0);
    await driver.action("atmosphere", 0);
    await driver.advance(0.3);
    const exteriorOff = await driver.capture("cockpit-exterior-off");
    for (const [name, property] of [
      ["cities", "cityGain"],
      ["atmosphere", "intensity"],
      ["stars", "starGain"],
    ] as const) {
      await driver.action(name, 1);
      const category = await driver.capture(`cockpit-exterior-${name}`);
      const observed = await driver.state();
      const value = entity(observed.scene, "cockpit-planet").components.find(
        (component) =>
          component.component === observed.componentIds.CustomMaterial,
      )?.properties?.[property];
      assert.deepEqual(value, { kind: "f32", value: 1 });
      exteriorDifference(exteriorOff, category, name);
      await driver.action(name, 0);
    }
    await driver.action("cities", 1);
    await driver.action("stars", 1);
    await driver.action("atmosphere", 1);
    await driver.action("lightsPlayback", true);
    await driver.action("atmospherePlayback", true);
    const resumed = phase(await driver.state());
    await driver.advance(0.12);
    assert.notEqual(phase(await driver.state()), resumed);
    visible(await driver.capture("cockpit-restored"));
    return { worlds: (await driver.state()).panelWorlds.length, motion };
  }
  await driver.options({ carousel: 0, charge: 0.7 });
  await driver.advance(0.3);
  await driver.press(0, "cockpit-pulse");
  await option("pulse", 1);
  await driver.action("scan", false);
  await option("scanning", false);
  await driver.options({ carousel: 0 });
  await driver.advance(0.3);
  await driver.press(0, "cockpit-atmosphere-playing");
  await option("atmospherePlaying", false);
  const stopped = await driver.state();
  const pausedPhase = phase(stopped);
  await driver.advance(0.12);
  assert.equal(
    phase(await driver.state()),
    pausedPhase,
    "Host animation pauses at its current property value",
  );
  await driver.press(0, "cockpit-control-tab/gui");
  await driver.press(0, "cockpit-projection/slider", 0.5);
  const projected = await driver.state();
  assert.ok(Number(projected.options.projection) > 0.1);
  const projection = await driver.capture("cockpit-projected");
  visible(projection);
  difference(glass, projection, "projection controls");
  await driver.press(0, "cockpit-next");
  await option("carousel", 1);
  await driver.advance(0.3);
  const rotated = await driver.capture("cockpit-carousel");
  visible(rotated);
  difference(projection, rotated, "carousel placement");
  await driver.options({ carousel: 0 });
  await driver.advance(0.3);
  await driver.press(0, "cockpit-surface/curved");
  await option("surface", "curved");
  assertCylinder(
    (await driver.state()).scene,
    initial.componentIds,
    Number((await driver.state()).options.projection),
  );
  visible(await driver.capture("cockpit-curved"));

  await driver.press("shell", "demo-display");
  await driver.press("shell", "demo-skin-picker");
  await driver.press("shell", "demo-skin-picker/options/cybertron");
  await option("skin", "cybertron");
  await driver.press("shell", "demo-display/close");
  const cybertron = await driver.capture("cockpit-cybertron");
  visible(cybertron);
  difference(rotated, cybertron, "skin selection");
  const afterSkin = await driver.state();
  for (let index = 0; index < 5; index++)
    for (const [name, id] of identities[index]!)
      assert.equal(
        entity(afterSkin.panels[index]!, String(name)).id,
        id,
        "skin and carousel retain control identities",
      );
  for (const skin of ["cybertron", "glass"]) {
    await driver.action("skin", skin);
    for (const carousel of [1, 1.4, 0.7]) {
      await driver.options({ carousel });
      await driver.advance(0.3);
      const distance = 9 + carousel;
      await driver.action("projection", distance);
      assertCylinder(
        (await driver.state()).scene,
        initial.componentIds,
        distance,
      );
      if (carousel === 1) {
        // Center the actual Display controls even in the narrow viewport, then
        // return to the requested matrix pose for geometry/pixel assertions.
        await driver.options({ carousel: 0 });
        await driver.advance(0.3);
        await driver.press(0, "cockpit-surface/flat");
        await driver.options({ carousel });
      } else await driver.options({ surface: "flat" });
      await option("surface", "flat");
      // Local selection notifications must settle before accepting the roundtrip.
      await driver.advance(0.35);
      const flat = await driver.state();
      assert.equal(
        flat.options.surface,
        "flat",
        "Flat remains selected after control field subscriptions settle",
      );
      assertFlat(flat.scene, initial.componentIds, distance);
      assert.deepEqual(
        flat.panelWorlds,
        initial.panelWorlds,
        "Flat retains all six Canvas Worlds",
      );
      await driver.options({ carousel: 0, charge: 0.7 });
      await driver.advance(0.3);
      await driver.press(0, "cockpit-control-tab/scene");
      await driver.press(0, "cockpit-pulse");
      await option("pulse", Number(flat.options.pulse) + 1);
      await driver.press(0, "cockpit-control-tab/gui");
      await driver.options({ carousel });
      await driver.advance(0.3);
      const flatFrame = await driver.capture(
        `cockpit-${skin}-flat-${carousel}`,
      );
      visible(flatFrame);
      if (carousel === 1) {
        await driver.options({ carousel: 0 });
        await driver.advance(0.3);
        await driver.press(0, "cockpit-surface/curved");
        await driver.options({ carousel });
      } else await driver.options({ surface: "curved" });
      await option("surface", "curved");
      await driver.advance(0.35);
      const curved = await driver.state();
      assert.equal(curved.options.surface, "curved");
      assertCylinder(curved.scene, initial.componentIds, distance);
      assert.deepEqual(
        curved.panelWorlds,
        initial.panelWorlds,
        "Curved retains all six Canvas Worlds",
      );
      for (const name of COCKPIT_ANCHORS.slice(4)) {
        assert.deepEqual(
          fields(curved.scene, name, initial.componentIds.Transform!),
          fields(initial.scene, name, initial.componentIds.Transform!),
          "dashboard instruments retain their physical mounts",
        );
        assert.deepEqual(
          fields(curved.scene, name, initial.componentIds.FlatSurface!),
          fields(initial.scene, name, initial.componentIds.FlatSurface!),
          "dashboard instrument surfaces stay flat",
        );
      }
      const curvedFrame = await driver.capture(
        `cockpit-${skin}-curved-${carousel}`,
      );
      visible(curvedFrame);
      difference(
        flatFrame,
        curvedFrame,
        "surface geometry changes completed pixels",
      );
      for (let index = 0; index < 5; index++)
        for (const [name, id] of identities[index]!)
          assert.equal(
            entity(curved.panels[index]!, String(name)).id,
            id,
            "shape switches retain Canvas controls",
          );
    }
  }
  return { worlds: (await driver.state()).panelWorlds.length };
}
