/**
 * One specimen in its own canvas World on a presenting Host: declare it,
 * reapply edited themes as row writes on the live theme entities, and
 * capture its states. Every capture runs inside the caller's `present`
 * section, which selects this World's canvas alone on the Host surface; the
 * environment decides how that selection is shared between clients.
 */
import {
  createElement,
  createRef,
  type ReactNode,
  type RefObject,
} from "react";
import {
  canvasOutput,
  type Client,
  type GuiFocusRecord,
  type GuiInputRoutingOutcome,
  type GuiPointerRecord,
  type GuiPhysicalContext,
  type GuiPhysicalInput,
  type HostClientBase,
  type OutputReference,
  type PresentationView,
  type PresentedCapture,
  type RootBinding,
  type WorldReference,
} from "@ipp/client";
import {
  Asset,
  Children,
  Entity,
  assetRef,
  createRoot,
  type ReactWorldClient,
  type ReactWorldRoot,
} from "@ipp/react";
import {
  Layout,
  Skin,
  Theme,
  ThemeMotion,
  type GuiControlHandle,
} from "@ipp/react/gui";
import { GuiKit, type GuiKitContract } from "@ipp/react/gui-kit";
import {
  blit,
  type PixelRect,
  type RgbaImage,
} from "../../../tools/shared-host/images.js";
import { image, settled } from "../../../tools/shared-host/presentation.js";
import {
  CAPTURE_SCALE,
  type PinStep,
  type Point,
  type SkinSpecimen,
  type SpecimenContext,
  type SpecimenState,
} from "./specimen.js";
import {
  encodeTheme,
  type EncodedTheme,
  type SkinThemes,
  type ThemeContract,
} from "./theme.js";
import { check } from "../../harness/page/checks.js";

const FONT_ASSET = "skin-lab/font";
const FONT_KIND = 17;
const POINTER = 1n;

export const SPECIMEN_SYSTEMS = [
  "ipp.animation",
  "ipp.gui",
  "ipp.gui-layout",
  "ipp.canvas",
  "ipp.asset-dependencies",
  "ipp.lifecycle-publisher",
] as const;

interface BatchCount {
  started: number;
  pending: number;
  /** Commands by kind since the last update, so edits can be seen to apply in place. */
  commands: Record<string, number>;
}

/** Selects `binding` alone on the Host surface while `section` runs. */
export type Present = <T>(
  binding: RootBinding,
  section: (view: PresentationView) => Promise<T>,
) => Promise<T>;

export interface SpecimenSessionOptions {
  readonly host: HostClientBase<Client>;
  /** The Host's generated contract: theme encoders, looks and tokens. */
  readonly contract: ThemeContract & GuiKitContract;
  /** Encoded `.ippf` bytes of the shared GUI font. */
  readonly font: Uint8Array<ArrayBuffer>;
  readonly specimen: SkinSpecimen;
  readonly themes: SkinThemes;
  /** Unique World symbolic id on the Host. */
  readonly world: string;
}

export interface CapturedState {
  readonly name: string;
  /** The state's cell in capture pixels. */
  readonly cell: PixelRect;
  /** The whole capture the cell was taken from. */
  readonly image: RgbaImage;
  readonly sequence: bigint;
  /**
   * Routing outcomes of its input steps in step order, then the focus and
   * pointer feedback observed before its capture.
   */
  readonly routing: readonly string[];
}

export interface SpecimenCapture {
  /** The canvas with every captured state's cell taken from its own capture. */
  readonly image: RgbaImage;
  readonly states: readonly CapturedState[];
  /** Focus or pointer feedback still live after every pin was released. */
  readonly leftover: readonly string[];
  readonly timings: Readonly<Record<string, number>>;
}

function identity(bytes: Uint8Array<ArrayBuffer>): Uint8Array<ArrayBuffer> {
  return bytes;
}

function pixelRect(cell: readonly number[]): PixelRect {
  const [x, y, width, height] = cell.map((value) =>
    Math.round(value * CAPTURE_SCALE),
  ) as [number, number, number, number];
  return [x, y, width, height];
}

export class SpecimenSession {
  private readonly controls = new Map<
    string,
    RefObject<GuiControlHandle | null>
  >();
  /** Batches started and still unacknowledged, which settled paint must outlast. */
  private batches: BatchCount = { started: 0, pending: 0, commands: {} };
  private encoded = new Map<string, EncodedTheme>();
  private binding!: RootBinding;

  private constructor(
    private readonly options: SpecimenSessionOptions,
    readonly world: WorldReference,
    private readonly client: Client,
    private readonly root: ReactWorldRoot,
    /** Failures the React root reports outside a render's own result. */
    private readonly errors: Error[],
    private specimen: SkinSpecimen,
    readonly output: OutputReference,
  ) {}

  static async open(options: SpecimenSessionOptions): Promise<SpecimenSession> {
    const { host, specimen } = options;
    const created = await host.createWorld({
      symbolicId: options.world,
      selectedSystems: SPECIMEN_SYSTEMS,
      // The World and everything in it end with this connection.
      temporary: true,
      canvas: { extent: [...specimen.extent], unitsPerMetre: 1 },
    });
    const client = await host.openWorld(created.reference);
    // Count the root's writes: a VirtualList declares its items only after
    // the runtime reports their range, so paint is not settled while a
    // write is in flight or starts during the settling frames.
    const batches: BatchCount = { started: 0, pending: 0, commands: {} };
    const batch = client.batch.bind(client);
    client.batch = async (commands) => {
      batches.started++;
      batches.pending++;
      for (const { kind } of commands)
        batches.commands[kind] = (batches.commands[kind] ?? 0) + 1;
      try {
        return await batch(commands);
      } finally {
        batches.pending--;
      }
    };
    const errors: Error[] = [];
    const root = createRoot(client as unknown as ReactWorldClient, {
      onError: (error) => errors.push(error),
    });
    const session = new SpecimenSession(
      options,
      created.reference,
      client,
      root,
      errors,
      specimen,
      canvasOutput(created.reference),
    );
    session.batches = batches;
    try {
      await session.update({ specimen, themes: options.themes });
      await session.fontReady();
    } catch (error) {
      await session.close().catch(() => {});
      throw error;
    }
    return session;
  }

  /**
   * Declare a changed specimen or theme module and return the commands that
   * declared it, by kind. Theme edits rewrite the `parts` rows and `em` of
   * the existing theme entities in place (`setField`); a changed specimen
   * module re-declares only the entities its components own.
   */
  async update(next: {
    specimen?: SkinSpecimen;
    themes?: SkinThemes;
  }): Promise<Readonly<Record<string, number>>> {
    this.batches.commands = {};
    if (next.themes) {
      const encoded = new Map<string, EncodedTheme>();
      for (const [name, theme] of Object.entries(next.themes)) {
        const fields = encodeTheme(this.options.contract, theme);
        if (fields) encoded.set(name, fields);
      }
      this.encoded = encoded;
    }
    const extentChanged =
      !this.binding ||
      (next.specimen &&
        next.specimen.extent.some(
          (value, axis) => value !== this.specimen.extent[axis],
        ));
    if (next.specimen) this.specimen = next.specimen;
    await this.root.render(this.declarations());
    // A specimen may show an asset that fails, such as a paint that does not
    // compile; its probes judge that failure, not the declaration.
    const failing = this.specimen.failingAssets ?? [];
    const failures = this.errors
      .splice(0)
      .filter(
        (error) =>
          !failing.some((id) => error.message.startsWith(`Asset ${id} failed`)),
      );
    check(
      !failures.length,
      `Declaring the specimen failed: ${failures.map((error) => error.message).join("; ")}`,
    );
    if (extentChanged) {
      const [width, height] = this.specimen.extent;
      this.binding = await this.options.host.setRootOutput(this.output, {
        width: Math.round(width * CAPTURE_SCALE),
        height: Math.round(height * CAPTURE_SCALE),
        devicePixelRatio: CAPTURE_SCALE,
      });
    }
    return { ...this.batches.commands };
  }

  /** Capture `names`, or every state, and compose their cells. */
  async capture(
    present: Present,
    names?: readonly string[],
  ): Promise<SpecimenCapture> {
    const unknown = names?.filter(
      (name) => !this.specimen.states.some((state) => state.name === name),
    );
    if (unknown?.length)
      throw new Error(
        `Unknown states ${unknown.join(", ")}; the specimen has ${this.specimen.states.map((state) => state.name).join(", ")}`,
      );
    const selected = this.specimen.states.filter(
      (state) => !names || names.includes(state.name),
    );
    const timings: Record<string, number> = {};
    const requested = performance.now();
    return present(this.binding, async (view) => {
      const started = performance.now();
      timings.waitMs = started - requested;
      const base = await this.settled(view);
      timings.baseMs = performance.now() - started;
      const whole = image(base);
      // Pinned cells are drawn into a copy; `whole` stays the base capture.
      const composite: RgbaImage = {
        ...whole,
        pixels: Uint8Array.from(whole.pixels),
      };
      const states: CapturedState[] = [];
      for (const state of selected) {
        const before = performance.now();
        const captured = state.pin?.length
          ? await this.pinned(view, state)
          : { capture: base, routing: [] };
        const cell = pixelRect(state.cell);
        const frame =
          captured.capture === base ? whole : image(captured.capture);
        if (captured.capture !== base)
          blit(composite, frame, cell, cell[0], cell[1]);
        states.push({
          name: state.name,
          cell,
          image: frame,
          sequence: captured.capture.sequence,
          routing: captured.routing,
        });
        timings[`${state.name}Ms`] = performance.now() - before;
      }
      const leftover = selected.some((state) => state.pin?.length)
        ? await this.observed()
        : [];
      timings.totalMs = performance.now() - started;
      return { image: composite, states, leftover, timings };
    });
  }

  /** Unmount the declarations and destroy the World. */
  async close(): Promise<void> {
    try {
      await this.root.unmount();
    } finally {
      await this.client.close();
      await this.options.host.destroyWorld(this.world);
    }
  }

  private declarations(): ReactNode {
    const lab: SpecimenContext = {
      font: assetRef(FONT_ASSET),
      skin: (theme) =>
        this.encoded.has(theme)
          ? createElement(Skin, { theme: `skin-lab/theme/${theme}` })
          : null,
      control: (name) => {
        let ref = this.controls.get(name);
        if (!ref) {
          ref = createRef<GuiControlHandle>();
          this.controls.set(name, ref);
        }
        return ref;
      },
    };
    const [width, height] = this.specimen.extent;
    const { contract } = this.options;
    // The kit draws at the body type size of sheet a, as the lab's own
    // themes do; a specimen of another sheet nests a GuiKit at its scale.
    return createElement(
      GuiKit,
      {
        contract,
        font: lab.font,
        fontSize: contract.GUI_SKIN_TOKENS.textBody,
      },
      createElement(Asset<Uint8Array<ArrayBuffer>>, {
        id: FONT_ASSET,
        kind: FONT_KIND,
        data: this.options.font,
        encode: identity,
      }),
      // One child slot for every theme, so adding the first rows of a theme
      // never moves the specimen to another slot and re-declares it.
      [...this.encoded].map(([name, { parts, em, motion }]) =>
        createElement(
          Entity,
          { key: name, id: `skin-lab/theme/${name}` },
          createElement(Theme, { parts, em }),
          motion && createElement(ThemeMotion, { parts: motion }),
        ),
      ),
      createElement(
        Entity,
        { id: "skin-lab/specimen" },
        createElement(Layout, {
          kind: 3,
          width,
          height,
          align_x: -1,
          align_y: -1,
        }),
        createElement(Children, null, this.specimen.render(lab)),
      ),
    );
  }

  /**
   * Wait until the declared asset `id` has loaded or failed on the Host and
   * return that status with its failure message, for specimens whose paint
   * depends on assets beyond the font, such as paint shaders.
   */
  async assetSettled(
    id: string,
  ): Promise<{ status: "loaded" | "failed"; error?: string }> {
    const deadline = performance.now() + 30_000;
    for (;;) {
      const state = this.root.getAsset(id);
      if (state?.status === "loaded") return { status: "loaded" };
      if (state?.status === "failed")
        return { status: "failed", error: state.error ?? "unknown" };
      if (performance.now() > deadline)
        throw new Error(
          `Asset ${id} not settled: ${state?.status ?? "absent"}`,
        );
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
  }

  private async fontReady(): Promise<void> {
    const deadline = performance.now() + 30_000;
    for (;;) {
      const state = this.root.getAsset(FONT_ASSET);
      if (state?.status === "loaded") return;
      if (state?.status === "failed")
        throw new Error(`Font asset failed: ${state.error ?? "unknown"}`);
      if (performance.now() > deadline)
        throw new Error(`Font asset not loaded: ${state?.status ?? "absent"}`);
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
  }

  /**
   * Settled paint: two identical frames with none of this root's writes
   * started or pending between them, because a VirtualList declares its
   * items only after the runtime reports their range.
   */
  private settled(view: PresentationView): Promise<PresentedCapture> {
    let started = this.batches.started;
    return settled(this.options.host, view, [this.output], () => {
      const quiet =
        this.batches.started === started && this.batches.pending === 0;
      started = this.batches.started;
      return quiet;
    });
  }

  /** Live focus and pointer feedback, so a pin is visible even where paint is not. */
  private async observed(): Promise<string[]> {
    const collection = async (name: "guiFocus" | "guiPointers") => {
      const records = [];
      let after = 0n;
      do {
        const page = await this.client.inspectPage({ collection: name, after });
        records.push(
          ...((name === "guiFocus" ? page.guiFocus : page.guiPointers) ?? []),
        );
        after = page.next;
      } while (after !== 0n);
      return records;
    };
    const focus = (await collection("guiFocus")) as GuiFocusRecord[];
    const pointers = (await collection("guiPointers")) as GuiPointerRecord[];
    if (!focus.length && !pointers.length) return [];
    const { entities } = await this.client.inspect();
    const name = (entity: bigint) =>
      entities.find((entry) => entry.id === entity)?.metadata.symbolicId ??
      String(entity);
    return [
      ...focus.map(
        (focus) =>
          `focus=${name(focus.target.entity)}${focus.visible ? "(visible)" : ""}`,
      ),
      ...pointers.map(
        (pointer) =>
          `pointer=${name(pointer.target.entity)}(${Object.entries(
            pointer.state,
          )
            .filter(([, value]) => value)
            .map(([key]) => key)
            .join(",")})`,
      ),
    ];
  }

  private async pinned(
    view: PresentationView,
    state: SpecimenState,
  ): Promise<{ capture: PresentedCapture; routing: string[] }> {
    const input = this.options.host.input;
    let context: GuiPhysicalContext | undefined;
    const focused = new Set<string>();
    const routing: string[] = [];
    const [width, height] = this.specimen.extent;
    const point = (at: Point): readonly [number, number] => [
      at[0] / width,
      at[1] / height,
    ];
    const physical = async (event: GuiPhysicalInput) => {
      context ??= await input.open(view);
      const outcome: GuiInputRoutingOutcome = await context.send(event);
      routing.push(
        `${event.kind}:${outcome.disposition}${outcome.error ? `(${outcome.error})` : ""}`,
      );
    };
    const apply = async (step: PinStep) => {
      switch (step.kind) {
        case "hover":
          return physical({
            kind: "pointerMove",
            pointer: POINTER,
            point: point(step.at),
          });
        case "press":
          await physical({
            kind: "pointerMove",
            pointer: POINTER,
            point: point(step.at),
          });
          return physical({
            kind: "pointerDown",
            pointer: POINTER,
            point: point(step.at),
          });
        case "click":
          await apply({ kind: "press", at: step.at });
          return physical({
            kind: "pointerUp",
            pointer: POINTER,
            point: point(step.at),
          });
        case "drag": {
          await apply({ kind: "press", at: step.from });
          for (let index = 1; index <= 4; index++)
            await physical({
              kind: "pointerMove",
              pointer: POINTER,
              point: point([
                step.from[0] + ((step.to[0] - step.from[0]) * index) / 4,
                step.from[1] + ((step.to[1] - step.from[1]) * index) / 4,
              ]),
            });
          return;
        }
        case "key":
          return physical({ kind: "key", key: step.key });
        case "settle":
          await this.settled(view);
          return;
        case "wait": {
          const start = await this.client.waitForFrame();
          for (let frame = start; frame.time - start.time < step.seconds; )
            frame = await this.client.waitForFrame(frame.tick);
          routing.push(`wait:${step.seconds}s`);
          return;
        }
        case "action": {
          const handle = this.controls.get(step.control)?.current;
          check(handle, `No declared control named ${step.control}`);
          const outcome = await handle.action(step.action);
          check(
            outcome.ok,
            `${step.action.kind} on ${step.control} refused: ${outcome.ok ? "" : outcome.error.reason}`,
          );
          if (step.action.kind === "focus") focused.add(step.control);
          return;
        }
        case "selectText": {
          const text = context?.nativeText;
          check(
            text,
            "selectText needs a text input focused by an earlier click step",
          );
          const outcome = await context!.editText(text.fence, {
            kind: "selection",
            start: step.start,
            end: step.end,
          });
          routing.push(`selectText:${outcome.disposition}`);
          return;
        }
        case "typeText": {
          const text = context?.nativeText;
          check(
            text,
            "typeText needs a text input focused by an earlier click step",
          );
          const outcome = await context!.editText(text.fence, {
            kind: "text",
            text: step.text,
          });
          routing.push(`typeText:${outcome.disposition}`);
          return;
        }
      }
    };
    try {
      for (const step of state.pin ?? []) await apply(step);
      routing.push(...(await this.observed()));
      return { capture: await this.settled(view), routing };
    } finally {
      if (context) {
        // Release pointer, capture and native focus before the context.
        for (const event of [
          { kind: "pointerCancel", pointer: POINTER },
          { kind: "blur" },
        ] as const)
          await context.send(event).catch(() => {});
        await context.close().catch(() => {});
      }
      for (const control of focused) {
        await this.controls
          .get(control)
          ?.current?.action({ kind: "blur" })
          .catch(() => {});
      }
      for (const step of state.restore ?? []) {
        check(step.kind === "action", "Restore steps must be actions");
        await apply(step);
      }
    }
  }
}
