import type {
  AnimationWorldClient,
  Client,
  Command,
  EntityRef,
  FieldValue,
  GuiWorldClient,
  HostClientBase,
  PresentedCapture,
  RowPropertyValue,
  RowsInput,
  RowsLayoutDescriptor,
} from "@ipp/client";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../camera-fixtures.js";
import { canvasOutput } from "../../../packages/ipp-client/src/references.js";
import { clientAssetSource } from "../../../packages/ipp-client/src/asset-sources.js";
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
  pending?: {
    source: string;
    stage(bytes: number[]): Promise<void>;
    requested(): Promise<void>;
    release(): Promise<void>;
  };
}
const width = 96;
const height = 64;
const transitionDuration = 2;

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

export async function ordinarySkinMotion(
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
  const sourceA = clientAssetSource(child.session, 10, "ordinary-motion-A");
  const sourceB = environment.pending
    ? { kind: 10, source: environment.pending.source }
    : clientAssetSource(child.session, 10, "ordinary-motion-B");
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

  const appearance = [
    { part: key("background"), color: [1, 0, 0, 1], opacity: 1, scale: [1, 1] },
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
    },
  ];
  function motions(source: typeof sourceA): Part[] {
    return appearance.map((part, index) => ({
      part: part.part,
      source,
      duration: transitionDuration,
      easing: 0,
      track: index === 2 ? 3 : 0,
      time: index === 1 ? 1 : 0,
    }));
  }

  function clip() {
    const values = [
      [
        [1, 0, 0, 1],
        [0, 0, 1, 1],
      ],
      [1, 1],
      [
        [1, 1],
        [1, 1],
      ],
      [
        [0, 1, 0, 1],
        [0, 1, 0, 1],
      ],
      [1, 1],
      [
        [1, 1],
        [1, 1],
      ],
    ];
    return child.encodeAnimationClip({
      duration: 1,
      tracks: values.map(([first, last], index) => ({
        property: {
          component: child.components.CustomMaterial!.id,
          name: `motion_${index}`,
        },
        keys: [first, last].map((value, time) => ({
          time,
          value: {
            kind: "dynamic" as const,
            value:
              typeof value === "number"
                ? { kind: "f32" as const, value }
                : value!.length === 2
                  ? { kind: "vec2" as const, value: value as [number, number] }
                  : {
                      kind: "vec4" as const,
                      value: value as [number, number, number, number],
                    },
          },
          interpolation:
            time === 0
              ? { kind: "linear" as const }
              : { kind: "step" as const },
        })),
      })),
    });
  }

  try {
    await child.registerAsset(sourceA, clip().buffer);
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
        rows("GuiThemeMotion", motions(sourceA)),
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
        "Surface",
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
    if (environment.pending) await environment.pending.stage([...clip()]);
    else await child.registerAsset(sourceB, clip().buffer);
    successfulBatch(
      await child.batch([
        field(
          child,
          handle(theme),
          "GuiThemeMotion",
          "parts",
          rows("GuiThemeMotion", motions(sourceB)),
        ),
      ]),
    );
    await action("toggle");
    if (environment.pending) {
      await environment.pending.requested();
      for (let repeat = 0; repeat < 5; repeat++) {
        await captureUntil(
          `pending-holds-${repeat}`,
          (image) => near(background(image), [255, 0, 0, 255]),
          repeat === 4,
        );
      }
      await environment.pending.release();
    }
    await captureUntil("replacement-intermediate", (image) => {
      const color = background(image);
      return (
        color[0]! > 40 && color[0]! < 215 && color[2]! > 40 && color[2]! < 215
      );
    });
    await captureUntil("replacement-blue", (image) =>
      near(background(image), [0, 0, 255, 255]),
    );
    check(host.renderDiagnostics, "Render diagnostics unavailable");

    async function retained(label: string, mutate?: () => Promise<void>) {
      const firstFrame = await child.waitForFrame();
      let warmFrame = firstFrame;
      while (warmFrame.time < firstFrame.time + transitionDuration + 0.05)
        warmFrame = await child.waitForFrame(warmFrame.tick);
      await captureUntil("warm-settled", () => true, false);
      const deadline = performance.now() + 10_000;
      let before = await host.renderDiagnostics!.statistics();
      while (before.gui?.guiBatches !== 1 || before.frame.uploadedBytes !== 0) {
        check(
          performance.now() < deadline,
          `${label} never coalesced its single compatible retained run`,
        );
        await captureUntil("warm-coalescing", () => true, false);
        before = await host.renderDiagnostics!.statistics();
      }
      await mutate?.();
      const frames = [];
      for (let repeat = 0; repeat < 5; repeat++) {
        frames.push(await child.waitForFrame());
        await captureUntil("unchanged-settled", () => true, false);
      }
      const after = await host.renderDiagnostics!.statistics();
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
          rows("GuiThemeMotion", motions(sourceA)),
        ),
        field(child, handle(control), "GuiSkin", "theme", {
          kind: "entity",
          value: { kind: "alias", alias: 5 },
        }),
        { kind: "delete", entity: handle(theme) },
      ]),
    );
    const replacementTheme = aliasId(themed, 5);
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
    const overrideMotion = motions(sourceA).map((part) =>
      part.part === key("focusRing") ? part : { ...part, time: 1 },
    );
    successfulBatch(
      await child.batch([
        field(
          child,
          handle(control),
          "GuiSkin",
          "parts",
          rows("GuiSkin", [{ part: key("background"), color: [0, 0, 1, 1] }]),
        ),
        field(
          child,
          handle(replacementTheme),
          "GuiThemeMotion",
          "parts",
          rows("GuiThemeMotion", overrideMotion),
        ),
      ]),
    );
    await mark("override-motion");
    await captureUntil("override-motion-intermediate", (image) => {
      const color = background(image);
      return (
        color[0]! > 60 && color[0]! < 215 && color[2]! > 60 && color[2]! < 215
      );
    });
    successfulBatch(
      await child.batch([
        {
          kind: "removeComponent",
          entity: handle(replacementTheme),
          component: child.components.GuiThemeMotion!.id,
        },
      ]),
    );
    await mark("override-motion-withdrawn");
    const staticOverride = await captureUntil(
      "override-retained-static-blue",
      () => true,
    );
    check(
      near(background(staticOverride), [0, 0, 255, 255]),
      "Withdrawing only motion must retain the authored blue override",
    );
    const afterOverride = await controlState(child, control);
    check(
      afterOverride?.target.incarnation === beforeOverride.target.incarnation &&
        afterOverride.value.kind === "bool" &&
        !afterOverride.value.value,
      "Motion/override edits changed the control or its value",
    );
    successfulBatch(
      await child.batch([
        field(child, handle(control), "GuiSkin", "parts", rows("GuiSkin", [])),
      ]),
    );
    await mark("override-withdrawn");
    await captureUntil("override-withdrawn-red", (image) =>
      near(background(image), [255, 0, 0, 255]),
    );

    successfulBatch(
      await child.batch([
        insertComponent(child, "GuiThemeMotion", handle(replacementTheme)),
        field(
          child,
          handle(replacementTheme),
          "GuiThemeMotion",
          "parts",
          rows("GuiThemeMotion", motions(sourceA)),
        ),
      ]),
    );

    await action("toggle");
    await captureUntil("before-gate-active", (image) => {
      const color = background(image);
      return (
        color[0]! > 60 && color[0]! < 210 && color[2]! > 60 && color[2]! < 210
      );
    });
    const afterGateWitness = nextWitness();
    const withdrawal = successfulBatch(
      await child.batch([
        {
          kind: "removeComponent",
          entity: handle(replacementTheme),
          component: child.components.GuiThemeMotion!.id,
        },
        ...afterGateWitness.operations,
      ]),
    );
    await record({
      label: "gated-withdrawal",
      mutationTick: withdrawal.tick,
      futureWitness: afterGateWitness.color,
    });
    expectedWitness = afterGateWitness.color;
    await captureUntil("resumed-static-blue", (image) =>
      near(background(image), [0, 0, 255, 255]),
    );

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
    successfulBatch(
      await child.batch([
        insertComponent(child, "GuiThemeMotion", handle(replacementTheme)),
        field(
          child,
          handle(replacementTheme),
          "GuiThemeMotion",
          "parts",
          rows("GuiThemeMotion", motions(sourceA)),
        ),
      ]),
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
      pendingResourceCoverage: environment.pending
        ? "controlled-http"
        : "not-provided-by-native-host",
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
