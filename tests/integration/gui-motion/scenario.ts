import { createElement as h } from "react";
import { renderDiagnostics } from "../../../packages/ipp-client/src/diagnostics.js";
import type {
  AnimationWorldClient,
  AssetWorldClient,
  Client,
  Command,
  EntityRef,
  FieldValue,
  GuiPhysicalContext,
  GuiWorldClient,
  HostClientBase,
  PresentationView,
  PresentedCapture,
  RowPropertyValue,
  RowsInput,
  RowsLayoutDescriptor,
  WorldReference,
} from "@ipp/client";
import {
  Children,
  Entity,
  createRoot,
  type ReactWorldClient,
  type ReactWorldRoot,
} from "../../../packages/ipp-react/src/index.js";
import { Button, Font, Layout } from "../../../packages/ipp-react/src/gui.js";
import {
  GuiKit,
  Spinner,
  type GuiKitContract,
} from "../../../packages/ipp-react/src/gui-kit.js";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../camera-fixtures.js";
import { canvasOutput } from "../../../packages/ipp-client/src/references.js";
import { controlState } from "../scenarios/gui-lifecycle.js";
import { guiAction } from "../gui-actions.js";
import {
  ATTACHMENTS,
  LIFECYCLE,
  CAMERA,
  RENDER,
  SURFACE,
  CANVAS,
  GUI,
  selectSystems,
} from "../system-selections.js";
export { nativePresentationTransport } from "../../../packages/ipp-client/src/native-presentation.js";
export { workerTransport } from "../../../packages/ipp-client/src/worker.js";

interface MotionContract {
  readonly GUI_PAINT_PART_KEYS: readonly {
    index: number;
    part: string;
    state: string | null;
    variant: string | null;
  }[];
  encodeRowsTable<Row extends object>(
    layout: RowsLayoutDescriptor,
    rows: RowsInput<Row>,
  ): Uint8Array<ArrayBuffer>;
  /** Built-in looks with their appearance and motion rows. */
  readonly GUI_SKIN_LOOKS: Readonly<
    Record<
      string,
      {
        readonly em: number;
        readonly parts: readonly Part[];
        readonly motion: readonly Part[];
      }
    >
  >;
}

type MotionClient = GuiWorldClient & AnimationWorldClient;
type Part = Record<string, RowPropertyValue>;
export interface MotionImage {
  name: string;
  width: number;
  height: number;
  pixels: number[];
  sequence: bigint;
  publication: PresentedCapture["publication"];
}

export interface MotionEnvironment {
  capture(image: MotionImage): Promise<void>;
  record(value: object): Promise<void>;
}
const width = 96;
const height = 64;
// Interruption checks start near the transition midpoint, so they can only
// tell continuity from restart while the Host-time envelope around a capture
// stays below about 0.22 × this duration. Software rendering's capture round
// trips take 0.4–0.6 s, beyond the 0.45 s that two seconds would allow.
const transitionDuration = 4;

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

function handle(id: bigint): EntityRef {
  return { kind: "handle", id };
}

function rgba(capture: PresentedCapture, x: number, y: number): number[] {
  return [...new Uint8Array(capture.pixels, (y * width + x) * 4, 4)];
}

function near(actual: number[], expected: number[], tolerance = 3): boolean {
  return actual.every(
    (value, index) => Math.abs(value - expected[index]!) <= tolerance,
  );
}

function linearChannel(value: number): number {
  const srgb = value / 255;
  return srgb <= 0.04045 ? srgb / 12.92 : ((srgb + 0.055) / 1.055) ** 2.4;
}

function background(capture: PresentedCapture): number[] {
  const sum = [0, 0, 0, 0];
  for (let vertical = 24; vertical < 40; vertical++) {
    for (let horizontal = 24; horizontal < 32; horizontal++) {
      for (const [channel, value] of rgba(
        capture,
        horizontal,
        vertical,
      ).entries())
        sum[channel]! += value;
    }
  }
  return sum.map((value) => value / 128);
}

/**
 * A client theme's own transitions on a nested canvas, the built-in looks'
 * default transitions on unthemed controls driven by physical input, then
 * the React kit's one reduced-motion setting for the runtime and the kit.
 */
export async function ordinarySkinMotion(
  host: HostClientBase<MotionClient>,
  contract: MotionContract & GuiKitContract,
  font: ArrayBuffer,
  environment: MotionEnvironment,
) {
  const themed = await themedSkinMotion(host, contract, environment);
  const unthemed = await defaultSkinMotion(host, contract, environment);
  const kit = await kitReducedMotion(host, contract, font, environment);
  return { themed, unthemed, kit };
}

async function themedSkinMotion(
  host: HostClientBase<MotionClient>,
  contract: MotionContract,
  environment: MotionEnvironment,
) {
  // The child canvas and the parent canvas both span the viewport at 96
  // units per Surface metre.
  const childWorld = await host.createWorld({
    selectedSystems: selectSystems(GUI, RENDER, LIFECYCLE),
    symbolicId: "motion-child",
    canvas: { extent: [width, height], unitsPerMetre: 96 },
  });
  const parentWorld = await host.createWorld({
    selectedSystems: selectSystems(
      ATTACHMENTS,
      CAMERA,
      SURFACE,
      CANVAS,
      LIFECYCLE,
    ),
    symbolicId: "motion-parent",
    canvas: { extent: [width, height], unitsPerMetre: 96 },
  });
  // A canvas World's Surface anchors are canvas slots, so the camera
  // presents the child through a spatial anchor in a World without Canvas.
  const cameraWorld = await host.createWorld({
    selectedSystems: selectSystems(ATTACHMENTS, CAMERA, SURFACE, LIFECYCLE),
    symbolicId: "motion-camera",
  });
  const child = await host.openWorld(childWorld.reference);
  const parent = await host.openWorld(parentWorld.reference);
  const images: Omit<MotionImage, "pixels">[] = [];
  const evidence: object[] = [];
  let completed = false;

  async function record(value: object) {
    evidence.push(value);
    await environment.record(value);
  }

  function key(
    part: string,
    state: string | null = null,
    variant: string | null = null,
  ): number {
    const found = contract.GUI_PAINT_PART_KEYS.find(
      (entry) =>
        entry.part === part &&
        entry.state === state &&
        entry.variant === variant,
    );
    check(found, `Missing target paint key ${part}/${state}/${variant}`);
    return found.index;
  }

  function field(
    client: Client,
    entity: EntityRef,
    component: string,
    name: string,
    value: FieldValue,
  ): Command {
    const descriptor = client.components[component];
    check(
      descriptor?.fields[name],
      `Missing target field ${component}.${name}`,
    );
    return {
      kind: "setField",
      entity,
      component: descriptor.id,
      field: { offset: descriptor.fields[name].offset, value },
    };
  }

  function rows(component: string, parts: Part[]): FieldValue {
    const layout = child.components[component]?.fields.parts?.rows;
    check(layout, `Missing target rows ${component}.parts`);
    return {
      kind: "rows",
      value: contract.encodeRowsTable(layout, {
        nextSlot: parts.length,
        rows: new Map(parts.map((part, index) => [index, part])),
      }),
    };
  }

  // Plain boxes: the rows state away the default checkbox look's line,
  // corner cuts and focus glow, which they would otherwise sit on.
  const appearance = [
    {
      part: key("background"),
      color: [1, 0, 0, 1],
      opacity: 1,
      scale: [1, 1],
      border_width: 0,
      corner_cut: [0, 0, 0, 0],
    },
    {
      part: key("background", "idle", "checked"),
      color: [0, 0, 1, 1],
      opacity: 1,
      scale: [1, 1],
    },
    {
      part: key("focusRing"),
      color: [0, 1, 0, 1],
      opacity: 1,
      scale: [1, 1],
      border_width: 2,
      corner_cut: [0, 0, 0, 0],
      glow_intensity: 0,
    },
  ];
  // Clipless timing rows: every transition into these appearances takes the
  // transition duration, linearly.
  const motions = (): Part[] =>
    appearance.map((part) => ({
      part: part.part,
      duration: transitionDuration,
      easing: 0,
    }));

  try {
    const declarations: Command[] = [
      createEntity(1, "canvas"),
      createEntity(2, "theme"),
      insertComponent(child, "GuiTheme", { kind: "alias", alias: 2 }),
      field(
        child,
        { kind: "alias", alias: 2 },
        "GuiTheme",
        "parts",
        rows("GuiTheme", appearance),
      ),
      insertComponent(child, "GuiThemeMotion", { kind: "alias", alias: 2 }),
      field(
        child,
        { kind: "alias", alias: 2 },
        "GuiThemeMotion",
        "parts",
        rows("GuiThemeMotion", motions()),
      ),
      createEntity(3, "checkbox"),
      insertComponent(child, "GuiCheckbox", { kind: "alias", alias: 3 }),
      insertComponent(
        child,
        "GuiLayout",
        { kind: "alias", alias: 3 },
        { width: 48, height: 24 },
      ),
      insertComponent(
        child,
        "CanvasStyle",
        { kind: "alias", alias: 3 },
        { x: 20, y: 20 },
      ),
      insertComponent(child, "GuiSkin", { kind: "alias", alias: 3 }),
      field(child, { kind: "alias", alias: 3 }, "GuiSkin", "theme", {
        kind: "entity",
        value: { kind: "alias", alias: 2 },
      }),
      {
        kind: "placeEntity",
        entity: { kind: "alias", alias: 3 },
        placement: { parent: { kind: "alias", alias: 1 }, before: null },
      },
      createEntity(4, "publication-witness"),
      insertComponent(
        child,
        "CanvasBox",
        { kind: "alias", alias: 4 },
        { width: 8, height: 8 },
      ),
      insertComponent(
        child,
        "CanvasStyle",
        { kind: "alias", alias: 4 },
        { red: 0, green: 0, blue: 0 },
      ),
      {
        kind: "placeEntity",
        entity: { kind: "alias", alias: 4 },
        placement: { parent: { kind: "alias", alias: 1 }, before: null },
      },
    ];
    const initial = successfulBatch(await child.batch(declarations));
    const control = aliasId(initial, 3);
    const theme = aliasId(initial, 2);
    const witness = aliasId(initial, 4);
    /** A 1 m wide Surface anchor presenting the child World's canvas. */
    const childSurface = (client: MotionClient): Command[] => [
      createEntity(2, "child-surface"),
      insertComponent(
        client,
        "FlatSurface",
        { kind: "alias", alias: 2 },
        { width: 1, height: height / 96 },
      ),
      {
        kind: "insertComponent",
        entity: { kind: "alias", alias: 2 },
        component: client.components.WorldAttachment!.id,
        fields: [
          {
            offset: client.components.WorldAttachment!.fields.mode!.offset,
            value: { kind: "u32", value: 1 },
          },
          {
            offset: client.components.WorldAttachment!.fields.child!.offset,
            value: { kind: "world", value: childWorld.reference },
          },
        ],
      },
    ];
    const parentInitial = successfulBatch(
      await parent.batch(childSurface(parent)),
    );
    const binding = await host.setRootOutput(
      canvasOutput(parentWorld.reference),
      {
        width,
        height,
        devicePixelRatio: 1,
      },
    );
    let view = await host.presentation.select(
      await host.presentation.surface(),
      binding,
    );
    let sequence = 0n;
    let marker = 0;
    let expectedWitness = [0, 0, 0, 255];

    function nextWitness() {
      marker++;
      check(marker < 64, "Fixture witness capacity exhausted");
      const linear = [
        (marker % 4) / 3,
        (Math.floor(marker / 4) % 4) / 3,
        Math.floor(marker / 16) / 3,
      ];
      const color = [
        ...linear.map((value) =>
          Math.round(
            255 *
              (value <= 0.0031308
                ? 12.92 * value
                : 1.055 * value ** (1 / 2.4) - 0.055),
          ),
        ),
        255,
      ];
      const operations = ["red", "green", "blue"].map((channel, index) =>
        field(child, handle(witness), "CanvasStyle", channel, {
          kind: "f32",
          value: linear[index]!,
        }),
      );
      return { color, operations };
    }

    async function mark(label: string) {
      const next = nextWitness();
      expectedWitness = next.color;
      const outcome = successfulBatch(await child.batch(next.operations));
      await record({
        label,
        witness: expectedWitness,
        mutationTick: outcome.tick,
      });
      return outcome;
    }

    async function saveImage(name: string, capture: PresentedCapture) {
      const image = {
        name,
        width,
        height,
        pixels: [...new Uint8Array(capture.pixels)],
        sequence: capture.sequence,
        publication: capture.publication,
      };
      images.push({
        name,
        width,
        height,
        sequence: image.sequence,
        publication: image.publication,
      });
      await environment.capture(image);
    }

    async function captureUntil(
      label: string,
      predicate: (image: PresentedCapture) => boolean,
      save = true,
    ) {
      const deadline = performance.now() + 12_000;
      let last: PresentedCapture | undefined;
      do {
        last = await host.presentation.capture(view, {
          afterSequence: sequence,
        });
        sequence = last.sequence;
        if (near(rgba(last, 3, 3), expectedWitness)) {
          if (last.failedDrawCalls !== 0) {
            await saveImage(`failure-${label}`, last);
            throw new Error(
              `${label}: causally included capture has failed draws`,
            );
          }
          if (predicate(last)) {
            if (save) await saveImage(label, last);
            return last;
          }
        }
      } while (performance.now() < deadline);
      if (last) await saveImage(`failure-${label}`, last);
      throw new Error(
        `${label} capture failed: witness ${last && rgba(last, 3, 3)}, background ${last && background(last)}, edge ${last && rgba(last, 20, 32)}, draws ${last?.drawCalls}/${last?.failedDrawCalls}`,
      );
    }

    // Actions run inside timed envelopes, so each side reads only the checked
    // field. The exact target and any change to the checkbox's lifetime come
    // from one watch opened here, outside every envelope.
    const checkbox = child.components.GuiCheckbox!.id;
    let lifetimeChanges = 0;
    const lifetimeWatch = await child.watchLifecycle(
      [
        {
          target: { kind: "component", entity: control, component: checkbox },
          kinds: 8 | 32 | 64,
        },
      ],
      () => {
        lifetimeChanges += 1;
      },
    );
    const lifetime = lifetimeWatch.baselines[0]?.lifetime;
    check(
      lifetime?.kind === "component" &&
        lifetime.incarnation !== null &&
        child.worldReference,
      "Motion control has no live incarnation",
    );
    const actionTarget = {
      world: child.worldReference,
      entity: control,
      component: checkbox,
      incarnation: lifetime.incarnation,
    };

    async function reducedMotion(): Promise<boolean | undefined> {
      const page = await child.inspectPage({
        collection: "guiPreferences",
        limit: 1,
      });
      return page.guiPreferences?.reducedMotion;
    }

    async function checked(): Promise<boolean> {
      const page = await child.inspectPage({
        collection: "entities",
        target: control,
        limit: 1,
      });
      const fields = page.entities
        .find((entity) => entity.id === control)
        ?.components.find(
          (component) => component.component === checkbox,
        )?.fields;
      check(fields, "Live control missing");
      return fields.checked === true;
    }

    async function action(kind: "toggle" | "focus" | "blur") {
      const before = await checked();
      const changes = lifetimeChanges;
      const outcome = await guiAction(child, actionTarget, { kind });
      check(
        outcome.ok,
        `${kind} rejected: ${JSON.stringify(outcome, (_, value) => (typeof value === "bigint" ? value.toString() : value))}`,
      );
      await record({ action: kind, tick: outcome.tick });
      const after = await checked();
      check(
        lifetimeChanges === changes,
        "Motion action retargeted the control",
      );
      if (kind === "toggle")
        check(after !== before, "Toggle outcome and committed state disagree");
      else check(after === before, "Focus feedback changed the checked value");
      await mark(kind);
      return outcome;
    }

    async function interrupt(
      label: string,
      kind: "toggle" | "focus",
      channel: (image: PresentedCapture) => number,
      forbiddenRestart: 0 | 1,
    ) {
      const startedAt = performance.now();
      const lowerFrame = await child.waitForFrame();
      await mark(`${label}-origin`);
      const origin = await captureUntil(`${label}-origin`, () => true);
      const effect = await action(kind);
      const first = await captureUntil(label, () => true);
      const firstReceivedAt = performance.now();
      const upperFence = await mark(`${label}-timing-end`);
      const upperFrame = await child.waitForFrame(upperFence.tick);
      const originValue = linearChannel(channel(origin));
      const firstValue = linearChannel(channel(first));
      const elapsed = upperFrame.time - lowerFrame.time;
      const travel = elapsed / transitionDuration;
      const tolerance = 0.025;
      const allowance = travel + tolerance;
      const allowed = [originValue - allowance, originValue + allowance];
      const forbidden =
        forbiddenRestart === 1 ? [1 - allowance, 1] : [0, allowance];
      const discriminating =
        Math.abs(forbiddenRestart - originValue) > 2 * allowance;
      await record({
        label: `${label}-continuity`,
        lowerFrame,
        upperFrame,
        upperFenceTick: upperFence.tick,
        effect,
        originSequence: origin.sequence,
        firstSequence: first.sequence,
        wallElapsedMs: firstReceivedAt - startedAt,
        elapsed,
        transitionDuration,
        originValue,
        firstValue,
        allowed,
        forbidden,
        discriminating,
      });
      check(
        elapsed >= 0 && discriminating,
        `${label}: insufficient timing evidence to distinguish continuity from restart (Host envelope ${elapsed}s, origin ${originValue})`,
      );
      check(
        firstValue >= allowed[0]! && firstValue <= allowed[1]!,
        `${label}: FIRST included capture violates continuity: ${firstValue} outside ${allowed}`,
      );
      check(
        firstValue < forbidden[0]! || firstValue > forbidden[1]!,
        `${label}: FIRST included capture moves into forbidden restart range ${forbidden}`,
      );
    }

    await mark("ready");
    await captureUntil("ready-red", (image) =>
      near(background(image), [255, 0, 0, 255]),
    );
    await action("toggle");
    await captureUntil("intermediate-blue", (image) => {
      const blue = linearChannel(background(image)[2]!);
      return blue > 0.4 && blue < 0.55;
    });
    await interrupt(
      "interrupted-to-red",
      "toggle",
      (image) => background(image)[2]!,
      1,
    );
    await captureUntil("settled-red", (image) =>
      near(background(image), [255, 0, 0, 255]),
    );
    await action("focus");
    await captureUntil(
      "focus-visible",
      (image) => rgba(image, 20, 32)[1]! > 245,
    );
    await action("blur");
    await captureUntil("focus-fading", (image) => {
      const green = linearChannel(rgba(image, 20, 32)[1]!);
      return green > 0.45 && green < 0.6;
    });
    await interrupt(
      "focus-interrupted",
      "focus",
      (image) => rgba(image, 20, 32)[1]!,
      0,
    );
    await captureUntil(
      "focus-restored",
      (image) => rgba(image, 20, 32)[1]! > 250,
    );
    await action("blur");
    await captureUntil("focus-hidden", (image) => rgba(image, 20, 32)[1]! < 3);
    await action("toggle");
    await captureUntil("second-intermediate", (image) => {
      const color = background(image);
      return (
        color[0]! > 40 && color[0]! < 215 && color[2]! > 40 && color[2]! < 215
      );
    });
    await captureUntil("second-blue", (image) =>
      near(background(image), [0, 0, 255, 255]),
    );
    check(renderDiagnostics(host), "Render diagnostics unavailable");

    async function retained(label: string, mutate?: () => Promise<void>) {
      const firstFrame = await child.waitForFrame();
      let warmFrame = firstFrame;
      while (warmFrame.time < firstFrame.time + transitionDuration + 0.05)
        warmFrame = await child.waitForFrame(warmFrame.tick);
      await captureUntil("warm-settled", () => true, false);
      const deadline = performance.now() + 10_000;
      let before = await renderDiagnostics(host)!.statistics();
      while (before.gui.guiBatches !== 1 || before.frame.uploadedBytes !== 0) {
        check(
          performance.now() < deadline,
          `${label} never coalesced its single compatible retained run`,
        );
        await captureUntil("warm-coalescing", () => true, false);
        before = await renderDiagnostics(host)!.statistics();
      }
      await mutate?.();
      const frames = [];
      for (let repeat = 0; repeat < 5; repeat++) {
        frames.push(await child.waitForFrame());
        await captureUntil("unchanged-settled", () => true, false);
      }
      const after = await renderDiagnostics(host)!.statistics();
      check(
        before.gui && after.gui,
        "GUI retained rendering counters unavailable",
      );
      await record({ label, firstFrame, warmFrame, frames, before, after });
      check(
        before.gui.totalGuiAllocations === after.gui.totalGuiAllocations &&
          before.gui.totalGuiRebuilds === after.gui.totalGuiRebuilds &&
          before.frame.totalUploadedBytes === after.frame.totalUploadedBytes,
        `${label} rebuilt or uploaded retained paint`,
      );
    }

    await retained("settled-motion-retention");
    const original = await controlState(child, control);
    check(
      original?.value.kind === "bool" && original.value.value,
      "Motion changed the checked value",
    );

    const themed = successfulBatch(
      await child.batch([
        createEntity(5, "replacement-theme"),
        insertComponent(child, "GuiTheme", { kind: "alias", alias: 5 }),
        field(
          child,
          { kind: "alias", alias: 5 },
          "GuiTheme",
          "parts",
          rows("GuiTheme", appearance),
        ),
        insertComponent(child, "GuiThemeMotion", { kind: "alias", alias: 5 }),
        field(
          child,
          { kind: "alias", alias: 5 },
          "GuiThemeMotion",
          "parts",
          rows("GuiThemeMotion", motions()),
        ),
        field(child, handle(control), "GuiSkin", "theme", {
          kind: "entity",
          value: { kind: "alias", alias: 5 },
        }),
        { kind: "delete", entity: handle(theme) },
      ]),
    );
    // The new theme was created with its alias.
    aliasId(themed, 5);
    await mark("retarget-delete-old-same-batch");
    await captureUntil("retarget-blue", (image) =>
      near(background(image), [0, 0, 255, 255]),
    );
    const afterRetarget = await controlState(child, control);
    check(
      afterRetarget?.target.incarnation === original.target.incarnation &&
        afterRetarget.value.kind === "bool" &&
        afterRetarget.value.value,
      "Theme retarget changed control lifetime or value",
    );

    await action("toggle");
    await captureUntil("override-origin-red", (image) =>
      near(background(image), [255, 0, 0, 255]),
    );
    const beforeOverride = await controlState(child, control);
    check(
      beforeOverride?.value.kind === "bool" && !beforeOverride.value.value,
      "Override fixture must start from unchecked red",
    );
    // Restyling is immediate: a per-control override lands in the first
    // capture that includes it, and so does its removal.
    successfulBatch(
      await child.batch([
        field(
          child,
          handle(control),
          "GuiSkin",
          "parts",
          rows("GuiSkin", [{ part: key("background"), color: [0, 0, 1, 1] }]),
        ),
      ]),
    );
    await mark("override");
    const staticOverride = await captureUntil(
      "override-static-blue",
      () => true,
    );
    check(
      near(background(staticOverride), [0, 0, 255, 255]),
      "An override must restyle the settled control at once",
    );
    const afterOverride = await controlState(child, control);
    check(
      afterOverride?.target.incarnation === beforeOverride.target.incarnation &&
        afterOverride.value.kind === "bool" &&
        !afterOverride.value.value,
      "Override edits changed the control or its value",
    );
    successfulBatch(
      await child.batch([
        field(child, handle(control), "GuiSkin", "parts", rows("GuiSkin", [])),
      ]),
    );
    await mark("override-withdrawn");
    const withdrawn = await captureUntil("override-withdrawn-red", () => true);
    check(
      near(background(withdrawn), [255, 0, 0, 255]),
      "Removing the override must restore the theme at once",
    );

    // Reduced motion snaps a transition under way. The preference precedes
    // the witness at the mutation boundary, so the first capture including
    // the witness shows the destination.
    await action("toggle");
    await captureUntil("before-reduced-motion", (image) => {
      const color = background(image);
      return (
        color[0]! > 60 && color[0]! < 210 && color[2]! > 60 && color[2]! < 210
      );
    });
    const reducedWitness = nextWitness();
    check((await reducedMotion()) === false, "Reduced motion must start off");
    child.sendCommand({
      type: "GuiPreferencesUpdateCommand",
      reducedMotion: true,
    });
    const reduced = successfulBatch(
      await child.batch(reducedWitness.operations),
    );
    check(
      (await reducedMotion()) === true,
      "The GUI preferences query must read back the reduced-motion update",
    );
    await record({
      label: "reduced-motion",
      mutationTick: reduced.tick,
      futureWitness: reducedWitness.color,
    });
    expectedWitness = reducedWitness.color;
    const snapped = await captureUntil(
      "reduced-motion-snapped-blue",
      () => true,
    );
    check(
      near(background(snapped), [0, 0, 255, 255]),
      "Reduced motion must snap the transition under way to its destination",
    );
    child.sendCommand({
      type: "GuiPreferencesUpdateCommand",
      reducedMotion: false,
    });

    await lifetimeWatch.remove();
    check(lifetimeChanges === 0, "Motion edits changed the control's lifetime");
    const retired = await controlState(child, control);
    check(retired, "Retired control missing");
    successfulBatch(
      await child.batch([
        {
          kind: "removeComponent",
          entity: handle(control),
          component: child.components.GuiCheckbox!.id,
        },
        insertComponent(child, "GuiCheckbox", handle(control)),
      ]),
    );
    const stale = await guiAction(child, retired.target, { kind: "toggle" });
    check(
      !stale.ok && stale.error.reason === "StaleTarget",
      "Old motion control target was not fenced",
    );
    const recreated = await controlState(child, control);
    check(
      recreated?.target.incarnation !== retired.target.incarnation &&
        recreated?.value.kind === "bool" &&
        !recreated.value.value,
      "Recreated control revived the retired checked value",
    );
    await mark("recreated-control");
    await captureUntil("recreated-red", (image) =>
      near(background(image), [255, 0, 0, 255]),
    );
    // The child moves from the canvas slot to a spatial anchor under a
    // camera.
    successfulBatch(
      await parent.batch([
        { kind: "delete", entity: handle(aliasId(parentInitial, 2)) },
      ]),
    );
    const cameraParent = await host.openWorld(cameraWorld.reference);
    const cameraScene = successfulBatch(
      await cameraParent.batch([
        ...childSurface(cameraParent),
        insertComponent(cameraParent, "Transform", { kind: "alias", alias: 2 }),
        createEntity(3, "motion-camera"),
        insertComponent(
          cameraParent,
          "Transform",
          { kind: "alias", alias: 3 },
          { z: 3 },
        ),
        insertComponent(
          cameraParent,
          "Camera",
          { kind: "alias", alias: 3 },
          { projection: 1, near: 0.1, far: 10, ortho_height: height / 96 },
        ),
      ]),
    );
    const camera = aliasId(cameraScene, 3);
    const cameraOutput = await host.bindOutput(
      cameraWorld.reference,
      camera,
      "camera",
    );
    const cameraBinding = await host.setRootOutput(cameraOutput, {
      width,
      height,
      devicePixelRatio: 1,
    });
    view = await host.presentation.select(view.surface, cameraBinding);
    await captureUntil(
      "camera-baseline",
      (image) =>
        near(background(image), [255, 0, 0, 255]) &&
        near(rgba(image, 20, 32), [255, 0, 0, 255]),
    );
    await retained("camera-only-retention", async () => {
      const outcome = successfulBatch(
        await cameraParent.batch([
          field(cameraParent, handle(camera), "Transform", "x", {
            kind: "f32",
            value: -2 / 96,
          }),
        ]),
      );
      await record({ label: "camera-mutation", mutationTick: outcome.tick });
      await captureUntil(
        "camera-moved-two-pixels",
        (image) =>
          rgba(image, 20, 32)[0]! < 80 &&
          near(rgba(image, 22, 32), [255, 0, 0, 255]),
      );
    });
    completed = true;
    return {
      images,
      evidence,
      child: childWorld.reference,
      parent: parentWorld.reference,
    };
  } finally {
    const cleanup = await Promise.allSettled([
      host.destroyWorld(cameraWorld.reference),
      host.destroyWorld(parentWorld.reference),
      host.destroyWorld(childWorld.reference),
    ]);
    await record({ label: "world-cleanup", cleanup });
    if (completed)
      check(
        cleanup.every((result) => result.status === "fulfilled"),
        "Motion fixture Worlds did not cleanly destroy",
      );
  }
}

/** Canvas extent and viewport of the default-motion World. */
const DEFAULT_WIDTH = 480;
const DEFAULT_HEIGHT = 128;

/** The controls' inherited font size: four times the looks' em, so every
 * length of the built-in looks draws four times its sheet size. */
const DEFAULT_FONT = 64;

interface Rect {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
}

/** Logical rectangles of the unthemed button and the switch. */
const BUTTON: Rect = { x: 16, y: 16, width: 192, height: 96 };
const SWITCH: Rect = { x: 256, y: 16, width: 192, height: 96 };

/** A point on the canvas no control covers. */
const AWAY = [232, 120] as const;

/** One pointer for every physical event. */
const POINTER = 1n;

/** Linear tolerance of one sampled channel: 8-bit sRGB quantisation and
 * blending. */
const CHANNEL_TOLERANCE = 0.02;

/** Pixel tolerance of a measured block position. */
const POSITION_TOLERANCE = 1.5;

/** Linear green of the pixel at `x`, `y` of a default-motion capture. */
function green(capture: PresentedCapture, x: number, y: number): number {
  const pixel = new Uint8Array(capture.pixels, (y * DEFAULT_WIDTH + x) * 4, 4);
  return linearChannel(pixel[1]!);
}

/** Mean linear green over `[x0, x1) x [y0, y1)`. */
function meanGreen(
  capture: PresentedCapture,
  x0: number,
  y0: number,
  x1: number,
  y1: number,
): number {
  let sum = 0;
  for (let y = y0; y < y1; y++)
    for (let x = x0; x < x1; x++) sum += green(capture, x, y);
  return sum / ((x1 - x0) * (y1 - y0));
}

/** The button's fill, away from its line and its glow's reach. */
function buttonFill(capture: PresentedCapture): number {
  const x = BUTTON.x + BUTTON.width / 2;
  const y = BUTTON.y + BUTTON.height / 2;
  return meanGreen(capture, x - 8, y - 8, x + 8, y + 8);
}

/** The button's top line, inside both its idle and its lit width. */
function buttonLine(capture: PresentedCapture): number {
  const x = BUTTON.x + BUTTON.width / 2;
  return meanGreen(capture, x - 16, BUTTON.y + 1, x + 16, BUTTON.y + 4);
}

/** The switch block's horizontal centre: the centroid of the bright pixels
 * on the rail's middle rows, inside its line. */
function switchBlock(capture: PresentedCapture): number {
  const y = SWITCH.y + SWITCH.height / 2;
  let weight = 0;
  let sum = 0;
  for (let row = y - 4; row < y + 4; row++)
    for (let x = SWITCH.x + 8; x < SWITCH.x + SWITCH.width - 8; x++)
      if (green(capture, x, row) > 0.2) {
        weight += 1;
        sum += x + 0.5;
      }
  check(weight > 0, "The switch block is not painted");
  return sum / weight;
}

/**
 * The Host time of every frame event one World session receives. The Host
 * supersedes a frame event that would wait behind other output, so a tick
 * without its own event is bounded by the recorded frames around it.
 */
class FrameTimes {
  /** Recorded frames in tick order. */
  readonly frames: { tick: bigint; time: number }[] = [];
  private stopped = false;

  constructor(private readonly client: Client) {
    void this.run();
  }

  get latest(): { tick: bigint; time: number } | undefined {
    return this.frames.at(-1);
  }

  private async run(): Promise<void> {
    let after = 0n;
    while (!this.stopped) {
      try {
        const frame = await this.client.waitForFrame(after);
        this.frames.push(frame);
        after = frame.tick;
      } catch {
        if (this.stopped || this.client.closure) return;
      }
    }
  }

  /** Host-time bounds of `tick`: the latest recorded frame at or before it
   * and the earliest at or after it, once one has arrived. */
  async bounds(tick: bigint): Promise<{ early: number; late: number }> {
    const deadline = performance.now() + 5_000;
    while (!this.latest || this.latest.tick < tick) {
      check(
        performance.now() < deadline,
        `No frame event arrived at or after tick ${tick}`,
      );
      await new Promise((resolve) => setTimeout(resolve, 2));
    }
    let early = Number.NEGATIVE_INFINITY;
    let late = Number.POSITIVE_INFINITY;
    for (const frame of this.frames) {
      if (frame.tick <= tick) early = frame.time;
      if (frame.tick >= tick) {
        late = frame.time;
        break;
      }
    }
    return { early, late };
  }

  stop(): void {
    this.stopped = true;
  }
}

/** One capture of a transition with the Host-time bounds of its tick. */
interface TransitionSample {
  readonly tick: bigint;
  readonly early: number;
  readonly late: number;
  readonly value: number;
  readonly capture: PresentedCapture;
}

/** Pointer input, captures and Host-time transition checks of one World
 * whose canvas is presented as the root at the default-motion size. */
interface MotionProbe {
  readonly clock: FrameTimes;
  /** The next capture that names the World's canvas, with its tick's Host
   * time bounds. */
  capture(): Promise<TransitionSample>;
  save(name: string, sample: TransitionSample, extra?: object): Promise<void>;
  /** Captures until `measure` reads the same value twice in a row. */
  settled(
    measure: (image: PresentedCapture) => number,
    tolerance: number,
  ): Promise<TransitionSample>;
  send(
    event:
      | {
          kind: "pointerMove" | "pointerDown" | "pointerUp";
          at: readonly [number, number];
        }
      | { kind: "pointerCancel" },
  ): Promise<void>;
  /**
   * One transition: settle, send `input`, then capture until the Host has
   * run a quarter second past `seconds`. Samples before the first changed
   * capture may predate the input. From that one on, the transition
   * started no earlier than the last frame observed before the input and
   * no later than the frame before the first change, which bounds each
   * sample's elapsed time with the Host-time bounds of its own tick.
   */
  transition(
    label: string,
    measure: (image: PresentedCapture) => number,
    tolerance: number,
    seconds: number,
    ease: (t: number) => number,
    input: () => Promise<void>,
    keep?: boolean,
  ): Promise<{ from: number; to: number }>;
  /** Release the pointer, the input context and the frame clock. */
  close(): Promise<void>;
}

/** Present `world`'s canvas as the root and open physical input on it. */
async function presentMotionProbe(
  host: HostClientBase<MotionClient>,
  client: Client,
  world: WorldReference,
  name: string,
  record: (value: object) => Promise<void>,
  environment: MotionEnvironment,
): Promise<MotionProbe> {
  const clock = new FrameTimes(client);
  const output = canvasOutput(world);
  let presented: { view: PresentationView; input: GuiPhysicalContext };
  try {
    const binding = await host.setRootOutput(output, {
      width: DEFAULT_WIDTH,
      height: DEFAULT_HEIGHT,
      devicePixelRatio: 1,
    });
    const view = await host.presentation.select(
      await host.presentation.surface(),
      binding,
    );
    presented = { view, input: await host.input.open(view) };
  } catch (error) {
    clock.stop();
    throw error;
  }
  const { view, input: context } = presented;
  let sequence = 0n;

  async function capture(): Promise<TransitionSample> {
    const deadline = performance.now() + 10_000;
    for (;;) {
      // Naming the canvas output reports the World tick each capture drew.
      const image = await host.presentation.capture(view, {
        afterOutputs: [output],
        ...(sequence === 0n ? {} : { afterSequence: sequence }),
      });
      sequence = image.sequence;
      check(image.failedDrawCalls === 0, "A capture has failed draws");
      const source = image.sources.find(
        (entry) => entry.output.world.id === world.id,
      );
      if (source)
        return {
          tick: source.tick,
          ...(await clock.bounds(source.tick)),
          value: Number.NaN,
          capture: image,
        };
      check(
        performance.now() < deadline,
        `No capture names the ${name} publication`,
      );
    }
  }

  async function save(name: string, sample: TransitionSample, extra = {}) {
    await environment.capture({
      name,
      width: DEFAULT_WIDTH,
      height: DEFAULT_HEIGHT,
      pixels: [...new Uint8Array(sample.capture.pixels)],
      sequence: sample.capture.sequence,
      publication: sample.capture.publication,
      ...{
        tick: sample.tick,
        hostTime: [sample.early, sample.late],
        ...extra,
      },
    } as MotionImage);
  }

  async function settled(
    measure: (image: PresentedCapture) => number,
    tolerance: number,
  ): Promise<TransitionSample> {
    let previous = await capture();
    for (let attempt = 0; attempt < 120; attempt++) {
      const next = await capture();
      if (
        Math.abs(measure(next.capture) - measure(previous.capture)) <=
        tolerance / 4
      )
        return { ...next, value: measure(next.capture) };
      previous = next;
    }
    throw new Error(`${name} motion did not settle`);
  }

  async function send(
    event:
      | {
          kind: "pointerMove" | "pointerDown" | "pointerUp";
          at: readonly [number, number];
        }
      | { kind: "pointerCancel" },
  ) {
    const outcome = await context.send(
      event.kind === "pointerCancel"
        ? { kind: "pointerCancel", pointer: POINTER }
        : {
            kind: event.kind,
            pointer: POINTER,
            point: [event.at[0] / DEFAULT_WIDTH, event.at[1] / DEFAULT_HEIGHT],
          },
    );
    check(
      !outcome.error && outcome.disposition !== "blocked",
      `${event.kind} was not routed: ${JSON.stringify(outcome)}`,
    );
  }

  async function transition(
    label: string,
    measure: (image: PresentedCapture) => number,
    tolerance: number,
    seconds: number,
    ease: (t: number) => number,
    input: () => Promise<void>,
    keep = false,
  ) {
    const from = (await settled(measure, tolerance)).value;
    const before = clock.latest;
    check(before, "No frame observed before the input");
    await input();
    const samples: TransitionSample[] = [];
    for (;;) {
      const sample = await capture();
      samples.push({ ...sample, value: measure(sample.capture) });
      const last = samples.at(-1)!;
      const previous = samples.at(-2);
      if (
        last.early - before.time > seconds + 0.25 &&
        previous &&
        Math.abs(last.value - previous.value) <= tolerance / 4
      )
        break;
      check(samples.length < 400, `${label} never settled`);
    }
    const to = samples.at(-1)!.value;
    const changed = samples.findIndex(
      (sample) => Math.abs(sample.value - from) > tolerance,
    );
    check(changed >= 0, `${label}: the input changed nothing`);
    const startLatest = (await clock.bounds(samples[changed]!.tick - 1n)).late;
    const checked = samples.map((sample, index) => {
      const progress = (elapsed: number) =>
        seconds === 0 ? 1 : Math.min(1, Math.max(0, elapsed / seconds));
      const bounds =
        index < changed
          ? [from, from]
          : [
              from + (to - from) * ease(progress(sample.early - startLatest)),
              from + (to - from) * ease(progress(sample.late - before.time)),
            ];
      const low = Math.min(bounds[0]!, bounds[1]!) - tolerance;
      const high = Math.max(bounds[0]!, bounds[1]!) + tolerance;
      return {
        tick: sample.tick,
        hostTime: [sample.early, sample.late],
        value: sample.value,
        low,
        high,
        passed: sample.value >= low && sample.value <= high,
      };
    });
    const intermediate = samples.filter(
      (sample) =>
        Math.abs(sample.value - from) > tolerance &&
        Math.abs(sample.value - to) > tolerance,
    ).length;
    await record({
      label,
      seconds,
      from,
      to,
      inputAfter: before,
      firstChanged: changed,
      intermediate,
      samples: checked,
    });
    if (keep)
      for (const [index, sample] of samples.entries())
        await save(`${label}-${index}`, sample, {
          elapsed: [
            Math.max(0, sample.early - startLatest),
            Math.max(0, sample.late - before.time),
          ],
          value: sample.value,
          bounds: [checked[index]!.low, checked[index]!.high],
        });
    else await save(label, samples[changed]!);
    const failed = checked.find((sample) => !sample.passed);
    check(
      !failed,
      `${label}: a sample left the eased envelope ${JSON.stringify(failed)}`,
    );
    if (seconds === 0)
      check(
        intermediate === 0,
        `${label} must land at once, without an intermediate capture`,
      );
    else
      check(
        intermediate > 0,
        `${label}: no capture fell inside the ${seconds} s transition`,
      );
    return { from, to };
  }

  return {
    clock,
    capture,
    save,
    settled,
    send,
    transition,
    async close() {
      clock.stop();
      await context
        .send({ kind: "pointerCancel", pointer: POINTER })
        .catch(() => {});
      await context.close().catch(() => {});
    },
  };
}

const linear = (t: number) => t;
const easeOutCubic = (t: number) => 1 - (1 - t) ** 3;

/** The pointer positions over the button's and the switch's centres. */
const OVER_BUTTON: readonly [number, number] = [
  BUTTON.x + BUTTON.width / 2,
  BUTTON.y + BUTTON.height / 2,
];
const OVER_SWITCH: readonly [number, number] = [
  SWITCH.x + SWITCH.width / 2,
  SWITCH.y + SWITCH.height / 2,
];

/**
 * The built-in looks' transitions on an unthemed button and on a switch
 * themed from the exported switch look, driven by physical pointer input.
 * Each capture names the World tick it drew; the Host time of that tick and
 * of the last frame observed before the input bound the transition's
 * elapsed time, so each sample is checked against the eased interpolation of
 * the settled ends over that whole interval. No client timer measures motion.
 */
async function defaultSkinMotion(
  host: HostClientBase<MotionClient>,
  contract: MotionContract,
  environment: MotionEnvironment,
) {
  const created = await host.createWorld({
    selectedSystems: selectSystems(GUI, LIFECYCLE),
    symbolicId: "default-motion",
    canvas: { extent: [DEFAULT_WIDTH, DEFAULT_HEIGHT], unitsPerMetre: 96 },
  });
  const client = await host.openWorld(created.reference);
  const evidence: object[] = [];
  let completed = false;
  let probe: MotionProbe | undefined;

  async function record(value: object) {
    evidence.push(value);
    await environment.record(value);
  }

  function rows(component: string, parts: readonly Part[]): FieldValue {
    const layout = client.components[component]?.fields.parts?.rows;
    check(layout, `Missing target rows ${component}.parts`);
    return {
      kind: "rows",
      value: contract.encodeRowsTable(layout, {
        nextSlot: parts.length,
        rows: new Map(parts.map((part, index) => [index, part])),
      }),
    };
  }

  function placed(alias: number, control: string, rect: Rect): Command[] {
    return [
      createEntity(alias, control),
      insertComponent(client, control, { kind: "alias", alias }),
      insertComponent(
        client,
        "GuiLayout",
        { kind: "alias", alias },
        { width: rect.width, height: rect.height },
      ),
      insertComponent(
        client,
        "CanvasStyle",
        { kind: "alias", alias },
        { x: rect.x, y: rect.y },
      ),
      {
        kind: "placeEntity",
        entity: { kind: "alias", alias },
        placement: { parent: { kind: "alias", alias: 1 }, before: null },
      },
    ];
  }

  try {
    const look = contract.GUI_SKIN_LOOKS.switch;
    check(look, "The contract exports no switch look");
    const theme = { kind: "alias" as const, alias: 4 };
    successfulBatch(
      await client.batch([
        createEntity(1, "canvas"),
        insertComponent(
          client,
          "GuiFont",
          { kind: "alias", alias: 1 },
          { font_size: DEFAULT_FONT },
        ),
        ...placed(2, "GuiButton", BUTTON),
        ...placed(3, "GuiCheckbox", SWITCH),
        // A client theme made from the exported switch look: its rows and
        // its motion rows, as the generated contract carries them.
        createEntity(4, "switch-theme"),
        insertComponent(client, "GuiTheme", theme, { em: look.em }),
        {
          kind: "setField",
          entity: theme,
          component: client.components.GuiTheme!.id,
          field: {
            offset: client.components.GuiTheme!.fields.parts!.offset,
            value: rows("GuiTheme", look.parts),
          },
        },
        insertComponent(client, "GuiThemeMotion", theme),
        {
          kind: "setField",
          entity: theme,
          component: client.components.GuiThemeMotion!.id,
          field: {
            offset: client.components.GuiThemeMotion!.fields.parts!.offset,
            value: rows("GuiThemeMotion", look.motion),
          },
        },
        insertComponent(client, "GuiSkin", { kind: "alias", alias: 3 }),
        {
          kind: "setField",
          entity: { kind: "alias", alias: 3 },
          component: client.components.GuiSkin!.id,
          field: {
            offset: client.components.GuiSkin!.fields.theme!.offset,
            value: { kind: "entity", value: theme },
          },
        },
      ]),
    );
    probe = await presentMotionProbe(
      host,
      client,
      created.reference,
      "default-motion",
      record,
      environment,
    );
    const { send, transition } = probe;

    await send({ kind: "pointerMove", at: AWAY });
    const hoverIn = await transition(
      "hover-in",
      buttonLine,
      CHANNEL_TOLERANCE,
      0.08,
      linear,
      () => send({ kind: "pointerMove", at: OVER_BUTTON }),
    );
    check(hoverIn.to > hoverIn.from, "Hover lights the button's line");
    await transition(
      "hover-out",
      buttonLine,
      CHANNEL_TOLERANCE,
      0.12,
      linear,
      () => send({ kind: "pointerMove", at: AWAY }),
    );
    await send({ kind: "pointerMove", at: OVER_BUTTON });
    const press = await transition(
      "press",
      buttonFill,
      CHANNEL_TOLERANCE,
      0,
      linear,
      () => send({ kind: "pointerDown", at: OVER_BUTTON }),
    );
    check(press.to > press.from, "A press fills the button with the accent");
    await transition(
      "release",
      buttonFill,
      CHANNEL_TOLERANCE,
      0.1,
      linear,
      () => send({ kind: "pointerUp", at: OVER_BUTTON }),
    );

    // The switch: off to on and back over 160 ms with an ease-out cubic,
    // the value committing at the release that toggles it.
    await send({ kind: "pointerMove", at: OVER_SWITCH });
    await send({ kind: "pointerDown", at: OVER_SWITCH });
    const on = await transition(
      "switch-on",
      switchBlock,
      POSITION_TOLERANCE,
      0.16,
      easeOutCubic,
      () => send({ kind: "pointerUp", at: OVER_SWITCH }),
      true,
    );
    check(
      on.to - on.from > 80,
      `The block travels from its left to its right end: ${JSON.stringify(on)}`,
    );
    await send({ kind: "pointerDown", at: OVER_SWITCH });
    await transition(
      "switch-off",
      switchBlock,
      POSITION_TOLERANCE,
      0.16,
      easeOutCubic,
      () => send({ kind: "pointerUp", at: OVER_SWITCH }),
      true,
    );

    // Reduced motion: the hover lands at once.
    client.sendCommand({
      type: "GuiPreferencesUpdateCommand",
      reducedMotion: true,
    });
    await send({ kind: "pointerMove", at: AWAY });
    await transition(
      "reduced-motion-hover",
      buttonLine,
      CHANNEL_TOLERANCE,
      0,
      linear,
      () => send({ kind: "pointerMove", at: OVER_BUTTON }),
    );
    client.sendCommand({
      type: "GuiPreferencesUpdateCommand",
      reducedMotion: false,
    });
    completed = true;
    return { world: created.reference, evidence };
  } finally {
    await probe?.close();
    const cleanup = await Promise.allSettled([
      host.destroyWorld(created.reference),
    ]);
    await record({ label: "default-motion-cleanup", cleanup });
    if (completed)
      check(
        cleanup.every((result) => result.status === "fulfilled"),
        "The default-motion World did not cleanly destroy",
      );
  }
}

/** The kit World's body text size: twice the looks' em, so the spinner's
 * ring is 48 units across. */
const KIT_FONT = 32;

/** The spinner's ring, right of the button and the reach of its glow. */
const SPINNER: Rect = { x: 320, y: 40, width: 48, height: 48 };

/** The RGBA bytes of the spinner's ring in a default-motion-sized capture. */
function spinnerRing(capture: PresentedCapture): Uint8Array {
  const ring = new Uint8Array(SPINNER.width * SPINNER.height * 4);
  for (let row = 0; row < SPINNER.height; row++)
    ring.set(
      new Uint8Array(
        capture.pixels,
        ((SPINNER.y + row) * DEFAULT_WIDTH + SPINNER.x) * 4,
        SPINNER.width * 4,
      ),
      row * SPINNER.width * 4,
    );
  return ring;
}

/** The lit quarter's centroid from the ring's centre, in pixels, and its
 * size: the accent's bright green, where the quiet track is dark. */
function litQuarter(ring: Uint8Array): { x: number; y: number; lit: number } {
  let lit = 0;
  let x = 0;
  let y = 0;
  for (let index = 0; index < ring.length / 4; index++)
    if (linearChannel(ring[index * 4 + 1]!) > 0.3) {
      lit += 1;
      x += (index % SPINNER.width) + 0.5 - SPINNER.width / 2;
      y += Math.floor(index / SPINNER.width) + 0.5 - SPINNER.height / 2;
    }
  return lit ? { x: x / lit, y: y / lit, lit } : { x: 0, y: 0, lit };
}

function sameBytes(left: Uint8Array, right: Uint8Array): boolean {
  return left.length === right.length && left.every((v, i) => v === right[i]);
}

/**
 * One reduced-motion setting for the runtime and the kit, through the React
 * kit's `GuiKit` in a World whose React root declares an unthemed button and
 * a kit spinner. With the setting on, the root kit sends the World's
 * preference, which reads back through the `guiPreferences` query: a hover
 * lands at once and the spinner holds its rest pose, its lit quarter from
 * twelve to three o'clock, over more than one turn of Host time, with no
 * animation controller. With it off, the preference reads back off, the
 * spinner's single controller turns it, and the hover takes the look's 80 ms.
 */
async function kitReducedMotion(
  host: HostClientBase<MotionClient>,
  contract: GuiKitContract,
  fontBytes: ArrayBuffer,
  environment: MotionEnvironment,
) {
  const created = await host.createWorld({
    selectedSystems: selectSystems(GUI, LIFECYCLE),
    symbolicId: "kit-motion",
    canvas: { extent: [DEFAULT_WIDTH, DEFAULT_HEIGHT], unitsPerMetre: 96 },
  });
  const client = await host.openWorld(created.reference);
  const evidence: object[] = [];
  const errors: Error[] = [];
  let completed = false;
  let probe: MotionProbe | undefined;
  let root: ReactWorldRoot | undefined;

  async function record(value: object) {
    evidence.push(value);
    await environment.record(value);
  }

  /** Poll the `guiPreferences` query until it reads `expected`. */
  async function preference(expected: boolean) {
    const deadline = performance.now() + 5_000;
    for (;;) {
      const page = await client.inspectPage({
        collection: "guiPreferences",
        limit: 1,
      });
      if (page.guiPreferences?.reducedMotion === expected) return page.tick;
      check(
        performance.now() < deadline,
        `guiPreferences never read reducedMotion ${expected}: ${JSON.stringify(page.guiPreferences)}`,
      );
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
  }

  /** Poll the World's animation controllers until there are `expected`. */
  async function controllers(expected: number) {
    const deadline = performance.now() + 5_000;
    for (;;) {
      const page = await client.inspectPage({ collection: "controllers" });
      const count = page.controllers?.length ?? 0;
      if (count === expected) return count;
      check(
        performance.now() < deadline,
        `The kit World has ${count} animation controllers, not ${expected}`,
      );
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
  }

  try {
    const font = await (client as unknown as AssetWorldClient).createAsset(
      17,
      fontBytes,
    );
    const kitRoot = createRoot(client as unknown as ReactWorldClient, {
      onError: (error) => errors.push(error),
    });
    root = kitRoot;
    const declare = async (reducedMotion: boolean) => {
      await kitRoot.render(
        h(
          GuiKit,
          { contract, font: font.source, fontSize: KIT_FONT, reducedMotion },
          h(
            Entity,
            { id: "canvas" },
            h(Font, { font_size: DEFAULT_FONT }),
            h(Layout, {
              kind: 3,
              width: DEFAULT_WIDTH,
              height: DEFAULT_HEIGHT,
              align_x: -1,
              align_y: -1,
            }),
            h(
              Children,
              null,
              h(
                Entity,
                { id: "button" },
                h(Layout, {
                  width: BUTTON.width,
                  height: BUTTON.height,
                  margin_left: BUTTON.x,
                  margin_top: BUTTON.y,
                  align_x: -1,
                  align_y: -1,
                }),
                h(Button, {}),
              ),
              h(Spinner, {
                id: "spinner",
                label: "Busy",
                layout: {
                  margin_left: SPINNER.x,
                  margin_top: SPINNER.y,
                  align_x: -1,
                  align_y: -1,
                },
              }),
            ),
          ),
        ),
      );
      check(
        !errors.length,
        `The kit declarations failed: ${errors.map((error) => error.message).join("; ")}`,
      );
    };

    // On: the root kit sends the preference with its first declarations.
    await declare(true);
    const onTick = await preference(true);
    await controllers(0);
    probe = await presentMotionProbe(
      host,
      client,
      created.reference,
      "kit-motion",
      record,
      environment,
    );
    const { capture, save, send, transition, clock } = probe;

    // The spinner holds its rest pose over more than a turn of Host time.
    const rest = await probe.settled(
      (image) => litQuarter(spinnerRing(image)).lit,
      1,
    );
    const pose = litQuarter(spinnerRing(rest.capture));
    check(
      pose.lit > 20 && pose.x > 4 && pose.y < -4,
      `The still spinner's lit quarter is not from twelve to three o'clock: ${JSON.stringify(pose)}`,
    );
    await save("kit-reduced-spinner", rest);
    let still = rest;
    const restRing = spinnerRing(rest.capture);
    for (let attempt = 0; still.early - rest.late < 1.25; attempt++) {
      check(attempt < 400, "Host time did not pass over the still spinner");
      still = await capture();
      check(
        sameBytes(spinnerRing(still.capture), restRing),
        `The spinner moved under reduced motion at tick ${still.tick}`,
      );
    }
    await save("kit-reduced-spinner-later", still);
    await record({
      label: "kit-reduced-spinner",
      preferenceTick: onTick,
      pose,
      hostTime: [rest.early, still.late],
      ticks: [rest.tick, still.tick],
    });

    // A hover lands at once.
    await send({ kind: "pointerMove", at: AWAY });
    await transition(
      "kit-reduced-hover",
      buttonLine,
      CHANNEL_TOLERANCE,
      0,
      linear,
      () => send({ kind: "pointerMove", at: OVER_BUTTON }),
    );

    // Off: the root kit sends the change, and both move again.
    await declare(false);
    const offTick = await preference(false);
    await controllers(1);
    const before = await capture();
    const beforeRing = spinnerRing(before.capture);
    let turned = before;
    for (
      let attempt = 0;
      sameBytes(spinnerRing(turned.capture), beforeRing);
      attempt++
    ) {
      check(
        attempt < 400 && clock.latest!.time - before.late < 3,
        "The spinner did not turn once reduced motion was off",
      );
      turned = await capture();
    }
    await save("kit-turning-spinner", turned);
    await record({
      label: "kit-turning-spinner",
      preferenceTick: offTick,
      from: { tick: before.tick, pose: litQuarter(beforeRing) },
      to: {
        tick: turned.tick,
        pose: litQuarter(spinnerRing(turned.capture)),
      },
    });
    await send({ kind: "pointerMove", at: AWAY });
    await transition(
      "kit-hover-in",
      buttonLine,
      CHANNEL_TOLERANCE,
      0.08,
      linear,
      () => send({ kind: "pointerMove", at: OVER_BUTTON }),
    );
    completed = true;
    return { world: created.reference, evidence };
  } finally {
    await probe?.close();
    // Unmounting the root leaves its declarations and the preference to the
    // World, which goes with them.
    const cleanup = [
      ...(root ? await Promise.allSettled([root.unmount()]) : []),
      ...(await Promise.allSettled([host.destroyWorld(created.reference)])),
    ];
    await record({ label: "kit-motion-cleanup", cleanup });
    if (completed)
      check(
        cleanup.every((result) => result.status === "fulfilled"),
        "The kit-motion World did not cleanly destroy",
      );
  }
}
