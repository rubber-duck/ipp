/** Mounted browser GUI fixture: React DOM -> IppCanvas -> generated worker
 * client, with ordinary GUI entities in attached child Worlds. */
import { useCallback, useState, type ReactElement } from "react";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import type {
  Client,
  ClientAssetSource,
  ComponentFieldValue,
  GuiNativeTextState,
  GuiPhysicalContext,
  HostPhysicalInput,
  OutputReference,
} from "@ipp/client";
import {
  CanvasWorld,
  Children,
  Entity,
  FlatSurface,
  Transform,
  type CanvasWorldHandle,
} from "../../../packages/ipp-react/src/index.js";
import {
  Button,
  Checkbox,
  Drawing,
  Font,
  Layout,
  Skin,
  Slider,
  Style,
  TextInput,
  Theme,
  type GuiControlHandle,
} from "../../../packages/ipp-react/src/gui.js";
import {
  IppCanvas,
  World,
  type CanvasRuntimeConfiguration,
  type IppCanvasHandle,
} from "../../../packages/ipp-react/src/web.js";
import {
  HEIGHT,
  PANEL_SYSTEMS,
  UNITS_PER_METRE,
  WIDTH,
  assetsLoaded,
  attachedSession,
  averageRgb,
  encodeTheme,
  loadAsset,
  presentCamera,
  presentedFrame,
  until,
  type GuiPanelContract,
} from "../support/mounted-panel.js";
import {
  ATTACHMENTS,
  LIFECYCLE,
  CAMERA,
  SURFACE,
  selectSystems,
} from "../../fixtures/system-selections.js";

/** Background tones the named control theme switches between while text
 * editing continues. */
const TONES = [
  [0.08, 0.18, 0.42, 1],
  [0.62, 0.14, 0.08, 1],
] as const;

interface FixtureAssets {
  readonly font: ClientAssetSource;
  readonly icon: ClientAssetSource;
}

type ControlRef = { current: GuiControlHandle | null };

const textRef: ControlRef = { current: null };
const buttonRef: ControlRef = { current: null };
const checkboxRef: ControlRef = { current: null };
const sliderRef: ControlRef = { current: null };
/** Buttons of the optional keyboard-order panels. */
const farButtonRef: ControlRef = { current: null };
const backButtonRef: ControlRef = { current: null };

let contract: GuiPanelContract | undefined;
let assets: FixtureAssets | undefined;
let output: OutputReference | undefined;
let root: Root | undefined;
let handle: IppCanvasHandle | undefined;
let panelWorld: CanvasWorldHandle | undefined;
let rerenderEquivalent: (() => void) | undefined;
let editorIdentity: HTMLTextAreaElement | undefined;
let physical: GuiPhysicalContext | undefined;
let releaseText: (() => void) | undefined;
let commits = 0;
/** Text values the onTextCommit callback observed: the current value first,
 * then each changed value at the end of its frame. */
let callbackValues: string[] = [];
let submissions: string[] = [];
let callbackRenders: number[] = [];
let presses = 0;
let errors: string[] = [];
let keyboardPanels = false;
let textTrace: string[] = [];
let renderedRevision = 0;
let setCommitRevision: ((value: number) => void) | undefined;
let setThemeTone: ((tone: number) => void) | undefined;
let releaseCommitReply: (() => void) | undefined;
let heldCommitStarted: Promise<void> | undefined;
let heldCommitBatches = 0;
let restoreBatch: (() => void) | undefined;

function fixtureContract(): GuiPanelContract {
  if (!contract) throw new Error("GUI fixture contract is not loaded");
  return contract;
}

function fixtureAssets(): FixtureAssets {
  if (!assets) throw new Error("GUI fixture assets are not loaded");
  return assets;
}

/** Hold one real, already-applied React batch reply while runtime input continues. */
export function holdReactCommitReply(): void {
  const current = handle;
  if (!current || restoreBatch)
    throw new Error("GUI commit gate is unavailable");
  const client = current.client;
  const original = client.batch.bind(client);
  let started!: () => void;
  heldCommitStarted = new Promise<void>((resolve) => {
    started = resolve;
  });
  const released = new Promise<void>((resolve) => {
    releaseCommitReply = resolve;
  });
  heldCommitBatches = 0;
  let held = false;
  client.batch = async (...args) => {
    heldCommitBatches += 1;
    const outcome = await original(...args);
    if (!held) {
      held = true;
      started();
      await released;
    }
    return outcome;
  };
  restoreBatch = () => {
    client.batch = original;
    restoreBatch = undefined;
  };
}

export function renderCommitRevision(value: number): void {
  if (!setCommitRevision) throw new Error("GUI commit fixture is not mounted");
  flushSync(() => setCommitRevision!(value));
}

export async function waitForHeldReactCommit(): Promise<void> {
  if (!heldCommitStarted) throw new Error("GUI commit gate is not armed");
  await heldCommitStarted;
}

export async function releaseHeldReactCommit(): Promise<number> {
  const current = handle;
  if (!current || !releaseCommitReply)
    throw new Error("GUI commit gate is not armed");
  releaseCommitReply();
  try {
    await current.flush();
    return heldCommitBatches;
  } finally {
    restoreBatch?.();
    releaseCommitReply = undefined;
    heldCommitStarted = undefined;
  }
}

export async function committedRevision(): Promise<unknown> {
  const current = handle;
  if (!current) throw new Error("GUI commit fixture is not mounted");
  const transform = current.client.components.Transform!;
  const entity = (await current.client.inspect()).entities.find(
    (entry) => entry.metadata.symbolicId === "mounted-gui-commit-revision",
  );
  return entity?.components.find((entry) => entry.component === transform.id)
    ?.fields.x;
}

/** Encoded part tables by tone and indicator; unchanged props keep bytes. */
const encodedThemes = new Map<string, Uint8Array<ArrayBuffer>>();

/**
 * Theme part rows at one background tone for flat controls: plain boxes
 * without the default look's line, corner cuts or glow, a white label and
 * caret, a translucent blue selection and a plain focus ring. `thumb` adds
 * the slider's white thumb; `indicator` the checkbox's untinted drawing.
 */
function themeParts(
  tone: number,
  indicator?: ClientAssetSource,
  thumb = false,
) {
  const key = `${tone}:${indicator?.source ?? ""}:${thumb}`;
  const cached = encodedThemes.get(key);
  if (cached) return cached;
  const plain = {
    border_width: 0,
    corner_cut: [0, 0, 0, 0],
    glow_intensity: 0,
  };
  const parts: [string, string | undefined, Record<string, unknown>][] = [
    [
      "background",
      undefined,
      { color: TONES[tone]!, opacity: 1, scale: [1, 1], ...plain },
    ],
    ["background", "hovered", { color: [0.14, 0.32, 0.7, 1] }],
    ["background", "pressed", { color: [0.04, 0.1, 0.28, 1] }],
    ["fill", undefined, { color: [0.15, 0.9, 0.55, 1], glow_intensity: 0 }],
    ["label", undefined, { color: [0.96, 0.98, 1, 1] }],
    ["caret", undefined, { color: [1, 1, 1, 1] }],
    ["selection", undefined, { color: [0.2, 0.45, 0.85, 0.5] }],
    [
      "focusRing",
      undefined,
      {
        color: [1, 0.72, 0.08, 1],
        opacity: 1,
        corner_cut: [0, 0, 0, 0],
        glow_intensity: 0,
      },
    ],
  ];
  if (thumb) parts.push(["icon", undefined, { color: [1, 1, 1, 1], ...plain }]);
  if (indicator)
    parts.push([
      "icon",
      undefined,
      {
        asset: { kind: 18, source: indicator.source },
        color: [1, 1, 1, 1],
        opacity: 1,
        scale: [1, 1],
      },
    ]);
  const encoded = encodeTheme(fixtureContract(), parts);
  encodedThemes.set(key, encoded);
  return encoded;
}

async function loadAssets(client: Client): Promise<FixtureAssets> {
  const [font, icon] = await Promise.all([
    loadAsset(client, 17, "/target/font-assets/shure-tech-mono.ippf"),
    loadAsset(client, 18, "/target/surface-assets/icon.ippd"),
  ]);
  return { font, icon };
}

/** The Host-owned physical context IppCanvas opens for its presented view,
 * observed read-only for runtime text focus and selection. */
function observePhysicalInput(input: HostPhysicalInput): void {
  const open = input.open.bind(input);
  input.open = async (...args) => {
    const context = await open(...args);
    releaseText?.();
    physical = context;
    releaseText = context.onText((state: GuiNativeTextState | null) => {
      textTrace.push(
        state === null
          ? "text:none"
          : `text:${state.text}:${state.selectionStart}-${state.selectionEnd}${state.composition ? `:composing:${state.composition.text}` : ""}`,
      );
      if (textTrace.length > 60) textTrace.shift();
    });
    // Native edits and their settlements, kept as failure context.
    const edit = context.editText.bind(context);
    context.editText = async (...edited) => {
      const [fence, command] = edited;
      const label = `edit:${JSON.stringify(command, (key, value: unknown) =>
        key === "fence"
          ? undefined
          : typeof value === "bigint"
            ? `${value}`
            : value,
      )}@g${fence.generation}`;
      try {
        const outcome = await edit(...edited);
        textTrace.push(
          `${label}:${outcome.applied}/${outcome.rejected}/${outcome.cancelled}${outcome.error ? `:${outcome.error}` : ""}`,
        );
        return outcome;
      } catch (error) {
        textTrace.push(`${label}:threw:${String(error)}`);
        throw error;
      } finally {
        if (textTrace.length > 60) textTrace.shift();
      }
    };
    return context;
  };
}

function PanelTheme({
  tone,
  indicator,
}: {
  readonly tone: number;
  readonly indicator?: ClientAssetSource;
}): ReactElement {
  return (
    <>
      <Entity id="mounted-controls">
        <Theme parts={themeParts(tone)} />
      </Entity>
      <Entity id="slider-controls">
        <Theme parts={themeParts(tone, undefined, true)} />
      </Entity>
      {indicator ? (
        <Entity id="indicator-controls">
          <Theme parts={themeParts(0, indicator)} />
        </Entity>
      ) : null}
    </>
  );
}

function Application({
  runtime,
}: {
  readonly runtime: CanvasRuntimeConfiguration;
}): ReactElement {
  const [renderRevision, setRenderRevision] = useState(0);
  const [commitRevision, updateCommitRevision] = useState(0);
  const [tone, setTone] = useState(0);
  const [presented, setPresented] = useState<OutputReference | null>(null);
  setCommitRevision = updateCommitRevision;
  setThemeTone = setTone;
  renderedRevision = renderRevision;
  rerenderEquivalent = () => setRenderRevision((value) => value + 1);
  const initialize = useCallback(
    async (
      client: Client,
      _signal: AbortSignal,
      host: IppCanvasHandle["host"],
    ) => {
      observePhysicalInput(host.input);
      assets = await loadAssets(client);
      output = await presentCamera(client, host, "mounted-gui-camera");
    },
    [],
  );
  const ready = useCallback((next: IppCanvasHandle) => {
    handle = next;
    setPresented(output ?? null);
  }, []);
  const current = assets;
  return (
    <IppCanvas
      runtime={runtime}
      world={{
        create: {
          selectedSystems: selectSystems(
            ATTACHMENTS,
            CAMERA,
            SURFACE,
            LIFECYCLE,
          ),
        },
      }}
      output={presented}
      initialize={initialize}
      width={WIDTH}
      height={HEIGHT}
      canvasProps={{ id: "mounted-gui-canvas" }}
      // A fresh blockers array each render must not reattach live input: the
      // equivalent rerender keeps text focus and selection.
      guiInput={{ blockers: [] }}
      onReady={ready}
      onError={(error) => errors.push(error.message)}
    >
      {current && presented ? (
        <World
          onCommit={() => {
            commits += 1;
          }}
          onError={(error) => errors.push(error.message)}
        >
          <Entity id="mounted-gui-commit-revision">
            <Transform x={commitRevision} />
          </Entity>
          {keyboardPanels ? <KeyboardOrderPanels tone={tone} /> : null}
          <Entity id="mounted-gui-panel">
            <Transform />
            <FlatSurface
              width={WIDTH / UNITS_PER_METRE}
              height={HEIGHT / UNITS_PER_METRE}
            />
          </Entity>
          <CanvasWorld
            presentation={{ anchor: "mounted-gui-panel" }}
            create={{
              symbolicId: "mounted-gui-panel-world",
              selectedSystems: PANEL_SYSTEMS,
            }}
            extent={[WIDTH, HEIGHT]}
            unitsPerMetre={UNITS_PER_METRE}
            onReady={(attached) => {
              panelWorld = attached;
            }}
            onError={(error) => errors.push(error.message)}
          >
            <PanelTheme tone={tone} indicator={current.icon} />
            <Entity id="canvas">
              <Font source={current.font.source} font_size={30} />
              <Layout kind={2} width={WIDTH} height={HEIGHT} />
              <Children>
                <Entity id="text">
                  <Layout width={240} height={75} />
                  <Skin theme="mounted-controls" />
                  <TextInput
                    ref={textRef}
                    text="a😀b"
                    placeholder="Edit"
                    onTextCommit={(event) => {
                      callbackValues.push(event.value);
                      callbackRenders.push(renderRevision);
                    }}
                    onSubmit={(event) => {
                      submissions.push(event.value);
                    }}
                  />
                </Entity>
                <Entity id="row">
                  <Layout kind={1} width={240} height={30} />
                  <Children>
                    <Entity id="icon">
                      <Layout width={60} height={30} />
                      <Style
                        scale_x={1.25}
                        scale_y={1.25}
                        red={0.15}
                        green={0.9}
                        blue={0.55}
                      />
                      <Drawing source={current.icon.source} />
                    </Entity>
                    <Entity id="checkbox">
                      <Layout width={45} height={30} />
                      <Skin theme="indicator-controls" />
                      <Checkbox ref={checkboxRef} checked={false} />
                    </Entity>
                    <Entity id="slider">
                      <Layout width={135} height={30} />
                      <Skin theme="slider-controls" />
                      <Slider ref={sliderRef} value={0.2} min={0} max={1} />
                    </Entity>
                  </Children>
                </Entity>
                <Entity id="button">
                  <Layout width={240} height={75} />
                  <Skin theme="mounted-controls" />
                  <Button
                    ref={buttonRef}
                    label="Done"
                    onPress={() => {
                      presses += 1;
                    }}
                  />
                </Entity>
              </Children>
            </Entity>
          </CanvasWorld>
        </World>
      ) : null}
    </IppCanvas>
  );
}

/**
 * Panels that order keyboard traversal around the main panel without
 * appearing in its frame: off to the side of the orthographic view, a
 * front-facing panel 2 m deeper than the main panel and a back-facing panel
 * 3 m nearer the camera. Traversal must visit the main panel, then the
 * deeper panel, then the back-facing one.
 */
function KeyboardOrderPanels({
  tone,
}: {
  readonly tone: number;
}): ReactElement {
  const panel = (
    name: string,
    label: string,
    ref: ControlRef,
    placement: ReactElement,
  ) => (
    <>
      <Entity id={`mounted-gui-${name}-panel`}>
        {placement}
        <FlatSurface width={2} height={1} />
      </Entity>
      <CanvasWorld
        presentation={{ anchor: `mounted-gui-${name}-panel` }}
        create={{
          symbolicId: `mounted-gui-${name}-world`,
          selectedSystems: PANEL_SYSTEMS,
        }}
        extent={[120, 60]}
        unitsPerMetre={UNITS_PER_METRE}
        onError={(error) => errors.push(error.message)}
      >
        <PanelTheme tone={tone} />
        <Entity id="canvas">
          <Font source={fixtureAssets().font.source} font_size={30} />
          <Layout kind={2} width={120} height={60} />
          <Children>
            <Entity id="button">
              <Layout width={120} height={60} />
              <Skin theme="mounted-controls" />
              <Button ref={ref} label={label} />
            </Entity>
          </Children>
        </Entity>
      </CanvasWorld>
    </>
  );
  return (
    <>
      {panel(
        "back",
        "Back",
        backButtonRef,
        <Transform x={-12} z={3} qy={1} qw={0} />,
      )}
      {panel("far", "Far", farButtonRef, <Transform x={12} z={-2} />)}
    </>
  );
}

function mounted(): IppCanvasHandle {
  if (!handle) throw new Error("GUI fixture is not mounted");
  return handle;
}

/** The attached main panel World's authoring session. */
function panelClient() {
  if (!handle || !panelWorld)
    throw new Error("GUI panel World is not attached");
  return attachedSession(handle, panelWorld.world);
}

export async function mountGuiCanvas(
  runtime: CanvasRuntimeConfiguration,
  options: { readonly keyboardPanels?: boolean } = {},
): Promise<void> {
  await closeGuiCanvas();
  contract = (await import(runtime.generatedModuleUrl)) as GuiPanelContract;
  encodedThemes.clear();
  keyboardPanels = options.keyboardPanels ?? false;
  handle = undefined;
  panelWorld = undefined;
  assets = undefined;
  output = undefined;
  physical = undefined;
  commits = 0;
  callbackValues = [];
  submissions = [];
  callbackRenders = [];
  presses = 0;
  errors = [];
  textTrace = [];
  renderedRevision = 0;
  textRef.current = null;
  buttonRef.current = null;
  checkboxRef.current = null;
  sliderRef.current = null;
  farButtonRef.current = null;
  backButtonRef.current = null;
  const host = document.createElement("div");
  host.id = "mounted-gui-host";
  document.body.replaceChildren(host);
  root = createRoot(host);
  root.render(<Application runtime={runtime} />);
  await until(
    () =>
      handle !== undefined &&
      commits > 0 &&
      panelWorld !== undefined &&
      textRef.current !== null &&
      buttonRef.current !== null &&
      checkboxRef.current !== null &&
      sliderRef.current !== null &&
      (!keyboardPanels ||
        (farButtonRef.current !== null && backButtonRef.current !== null)) &&
      physical !== undefined &&
      document.querySelector("textarea[data-ipp-native-text]") !== null,
    () => `mounted IppCanvas GUI did not acknowledge: ${errors.join("; ")}`,
  );
  await handle!.flush();
  await assetsLoaded(
    handle!.client,
    Object.values(fixtureAssets()).map((asset) => asset.source),
  );
  await handle!.frame();
  editorIdentity =
    document.querySelector<HTMLTextAreaElement>(
      "textarea[data-ipp-native-text]",
    ) ?? undefined;
  if (editorIdentity) traceEditor(editorIdentity);
}

/** Native buffer events after the adapter handled them, kept as failure
 * context beside the runtime edits they produced. */
function traceEditor(area: HTMLTextAreaElement): void {
  const record = (event: Event) => {
    const detail =
      event instanceof KeyboardEvent
        ? event.key
        : event instanceof InputEvent
          ? `${event.inputType}:${event.data ?? ""}`
          : event instanceof CompositionEvent
            ? event.data
            : event instanceof ClipboardEvent
              ? (event.clipboardData?.getData("text/plain") ?? "no-data")
              : "";
    textTrace.push(
      `dom:${event.type}:${detail}${event.defaultPrevented ? ":prevented" : ""}|${area.value}|${area.selectionStart}-${area.selectionEnd}`,
    );
    if (textTrace.length > 60) textTrace.shift();
  };
  for (const type of [
    "keydown",
    "beforeinput",
    "compositionstart",
    "compositionupdate",
    "compositionend",
    "select",
    "paste",
    "copy",
    "cut",
  ])
    area.addEventListener(type, record);
}

/** The control component's fields, read through its ref. */
async function controlFields(
  ref: ControlRef,
): Promise<Readonly<Record<string, ComponentFieldValue>>> {
  const control = ref.current;
  if (!control) throw new Error("GUI control ref is not acknowledged");
  return control.read();
}

/** Whether `ref`'s control holds its World's logical focus. */
async function controlFocused(ref: ControlRef): Promise<boolean> {
  const control = ref.current;
  if (!control || !handle) return false;
  const { target } = control;
  const page = await attachedSession(handle, target.world).inspectPage({
    collection: "guiFocus",
    target: target.entity,
  });
  return (page.guiFocus ?? []).some(
    (record) =>
      record.target.entity === target.entity &&
      record.target.component === target.component &&
      record.target.incarnation === target.incarnation,
  );
}

export async function controlPaintObservation(flush = true): Promise<{
  readonly checked: boolean;
  readonly slider: number;
  readonly drawCalls: number;
  readonly failedDrawCalls: number;
  readonly checkboxIndicator: readonly [number, number, number];
  readonly checkboxOutsideIndicator: readonly [number, number, number];
  readonly sliderInitialThumb: readonly [number, number, number];
  readonly sliderMovedThumb: readonly [number, number, number];
  readonly sliderFill: readonly [number, number, number];
}> {
  const current = handle;
  if (current === undefined)
    throw new Error("GUI control paint fixture is not ready");
  if (flush) await current.flush();
  const [checkbox, slider] = await Promise.all([
    controlFields(checkboxRef),
    controlFields(sliderRef),
  ]);
  const frame = await presentedFrame(current, flush);
  if (typeof checkbox.checked !== "boolean" || typeof slider.value !== "number")
    throw new Error("GUI control paint values disappeared");
  const { pixels, width, height } = frame;
  return {
    checked: checkbox.checked,
    slider: slider.value,
    drawCalls: frame.drawCalls,
    failedDrawCalls: frame.failedDrawCalls,
    checkboxIndicator: averageRgb(pixels, width, height, 83, 90),
    checkboxOutsideIndicator: averageRgb(pixels, width, height, 98, 90),
    sliderInitialThumb: averageRgb(pixels, width, height, 139, 90),
    sliderMovedThumb: averageRgb(pixels, width, height, 216, 90),
    sliderFill: averageRgb(pixels, width, height, 184, 90),
  };
}

type PartsTable = {
  readonly nextSlot: number;
  readonly rows: ReadonlyMap<number, Readonly<Record<string, unknown>>>;
};

/** Theme entities of the main panel World: their identities and part rows. */
async function panelThemes(): Promise<
  readonly {
    readonly id: bigint;
    readonly name: string;
    readonly parts: PartsTable;
  }[]
> {
  const client = panelClient();
  const theme = client.components.GuiTheme!;
  return (await client.inspect()).entities.flatMap((entity) => {
    const parts = entity.components.find(
      (component) => component.component === theme.id,
    )?.fields.parts as PartsTable | undefined;
    return parts
      ? [{ id: entity.id, name: entity.metadata.symbolicId ?? "", parts }]
      : [];
  });
}

export async function captureThemeEvidence(): Promise<{
  readonly width: number;
  readonly height: number;
  readonly drawCalls: number;
  readonly coloredPixels: number;
  readonly parts: readonly string[];
}> {
  const frame = await presentedFrame(mounted());
  let coloredPixels = 0;
  for (let index = 0; index < frame.pixels.length; index += 4)
    if (
      frame.pixels[index] !== 0 ||
      frame.pixels[index + 1] !== 0 ||
      frame.pixels[index + 2] !== 0
    )
      coloredPixels += 1;
  // Base parts the panel's themes author, from their part rows.
  const keys = fixtureContract().GUI_PAINT_PART_KEYS;
  const parts = new Set<string>();
  for (const theme of await panelThemes())
    for (const row of theme.parts.rows.values()) {
      const key = keys.find((entry) => entry.index === row.part);
      if (key) parts.add(key.part);
    }
  return {
    width: frame.width,
    height: frame.height,
    drawCalls: frame.drawCalls,
    coloredPixels,
    parts: [...parts].sort(),
  };
}

/** Canvas rows the text input occupies: the top 75 of the panel's 180. */
const TEXT_INPUT_ROWS = 75;

/** Completed-frame RGBA pixels of the text input's rows, top row first. */
export async function textInputPaint(): Promise<{
  readonly width: number;
  readonly height: number;
  readonly pixels: readonly number[];
}> {
  const frame = await presentedFrame(mounted());
  return {
    width: frame.width,
    height: TEXT_INPUT_ROWS,
    pixels: Array.from(
      frame.pixels.subarray(0, frame.width * TEXT_INPUT_ROWS * 4),
    ),
  };
}

/** Switch the named control theme's background tone. */
export function retoneTheme(tone: number): void {
  if (setThemeTone === undefined) throw new Error("GUI fixture is not mounted");
  setThemeTone(tone);
}

/** Painted Done-button background beside its label, the controls' skin
 * rows and the named theme's identity, read from one completed frame and
 * the authoritative panel World. */
export async function themeEditEvidence(): Promise<{
  readonly button: readonly [number, number, number];
  readonly skins: string;
  readonly theme: string;
  readonly background: readonly number[];
}> {
  const frame = await presentedFrame(mounted());
  const client = panelClient();
  const skin = client.components.GuiSkin!;
  const skins = (await client.inspect()).entities
    .flatMap((entity) => {
      const fields = entity.components.find(
        (component) => component.component === skin.id,
      )?.fields;
      return fields
        ? [[entity.metadata.symbolicId, entity.id, fields] as const]
        : [];
    })
    .sort(([left], [right]) => String(left).localeCompare(String(right)));
  const named = (await panelThemes()).find(
    (theme) => theme.name === "mounted-controls",
  );
  if (!named) throw new Error("Named control theme is missing");
  const background = named.parts.rows.get(0)?.color;
  if (!Array.isArray(background))
    throw new Error("Named theme omitted its background row");
  return {
    button: averageRgb(frame.pixels, frame.width, frame.height, 200, 150),
    skins: JSON.stringify(skins, (_, value: unknown) =>
      typeof value === "bigint"
        ? value.toString()
        : value instanceof Map
          ? [...value]
          : value,
    ),
    theme: `${named.id}:${named.parts.nextSlot}:${[...named.parts.rows.keys()].join(",")}`,
    background: background as number[],
  };
}

export function equivalentRerender(): void {
  if (rerenderEquivalent === undefined)
    throw new Error("GUI fixture is not mounted");
  rerenderEquivalent();
}

/** Replace the mounted text input's value from outside, as a
 * controlled-value pattern or machine client would: compare-and-set on its
 * text field against the value read just before. */
export async function replaceText(value: string): Promise<void> {
  const control = textRef.current;
  if (control === null) throw new Error("GUI fixture is not ready");
  const current = (await control.read()).text;
  if (typeof current !== "string")
    throw new Error("mounted text input disappeared");
  if (!(await control.compareAndSet("text", current, value)))
    throw new Error("Text replacement lost a race with another write");
}

function nativeEditor(): HTMLTextAreaElement | null {
  return document.querySelector<HTMLTextAreaElement>(
    "textarea[data-ipp-native-text]",
  );
}

/** Toggle presentation on the same runtime input. */
export async function setMasked(masked: boolean): Promise<void> {
  const control = textRef.current;
  if (control === null) throw new Error("GUI fixture is not ready");
  const current = (await control.read()).masked;
  if (
    typeof current !== "boolean" ||
    !(await control.compareAndSet("masked", current, masked))
  )
    throw new Error("Mask update failed");
  await handle?.flush();
}

export async function observation(): Promise<{
  readonly text: string;
  readonly editorValue: string | null;
  readonly callbackValues: readonly string[];
  readonly submissions: readonly string[];
  readonly callbackRenders: readonly number[];
  readonly presses: number;
  readonly commits: number;
  readonly renderRevision: number;
  readonly errors: readonly string[];
  readonly activeEditor: boolean;
  readonly sameEditor: boolean;
  readonly editorCount: number;
  readonly selectionDirection: string | null;
  readonly domSelection: readonly [number, number] | null;
  readonly focusSelection: readonly [number, number] | null;
  readonly textTrace: readonly string[];
  readonly masked: boolean;
  readonly nativeMask: string;
  readonly textEntity: string;
  readonly focusGeneration: string | null;
}> {
  const current = handle;
  const control = textRef.current;
  if (current === undefined || control === null)
    throw new Error("GUI fixture is not ready");
  await current.flush();
  const text = (await control.read()).text;
  if (typeof text !== "string")
    throw new Error("mounted text input disappeared");
  const editor = nativeEditor();
  const focus = physical?.nativeText ?? null;
  return {
    text,
    editorValue: editor?.value ?? null,
    callbackValues: [...callbackValues],
    submissions: [...submissions],
    callbackRenders: [...callbackRenders],
    presses,
    commits,
    renderRevision: renderedRevision,
    errors: [...errors],
    activeEditor: editor !== null && document.activeElement === editor,
    sameEditor: editor !== null && editor === editorIdentity,
    editorCount: document.querySelectorAll("textarea").length,
    selectionDirection: editor?.selectionDirection ?? null,
    domSelection:
      editor === null ? null : [editor.selectionStart, editor.selectionEnd],
    focusSelection:
      focus === null ? null : [focus.selectionStart, focus.selectionEnd],
    textTrace: [...textTrace],
    masked: focus?.masked ?? false,
    nativeMask: editor?.style.getPropertyValue("-webkit-text-security") ?? "",
    textEntity: control.target.entity.toString(),
    focusGeneration: focus?.fence.generation.toString() ?? null,
  };
}

/** Frames a focus read across Worlds may take to settle. */
const FOCUS_REREADS = 5;

/** Runtime keyboard focus, DOM key ownership and control outcomes for
 * keyboard-only operation. */
export async function keyboardObservation(): Promise<{
  readonly focused: string | null;
  readonly keyOwner: "canvas" | "editor" | "other";
  readonly presses: number;
  readonly text: string;
  readonly errors: readonly string[];
}> {
  const current = handle;
  const controls = {
    text: textRef.current,
    checkbox: checkboxRef.current,
    slider: sliderRef.current,
    button: buttonRef.current,
    farButton: farButtonRef.current,
    backButton: backButtonRef.current,
  };
  if (current === undefined || controls.text === null)
    throw new Error("GUI keyboard fixture is not ready");
  await current.flush();
  // Focus may sit in any attached panel World; each World's focus query
  // reports its own. Each World answers at its own boundary, so the reads can
  // fall on either side of the frame that moves focus between Worlds: two
  // focused controls are read again once each of their Worlds completes a
  // frame, and fail only when they persist over several frames.
  let focused: string[] = [];
  for (let attempt = 0; ; attempt++) {
    const focusedNames = await Promise.all(
      Object.entries(controls).map(async ([name, control]) =>
        control !== null && (await controlFocused({ current: control }))
          ? name
          : null,
      ),
    );
    focused = focusedNames.filter((name) => name !== null);
    if (focused.length <= 1) break;
    if (attempt === FOCUS_REREADS)
      throw new Error(`Several controls report focus: ${focused}`);
    await Promise.all(
      focused.map((name) =>
        attachedSession(
          current,
          controls[name as keyof typeof controls]!.target.world,
        ).waitForFrame(),
      ),
    );
  }
  const text = (await controls.text.read()).text;
  if (typeof text !== "string")
    throw new Error("mounted text input disappeared");
  const active = document.activeElement;
  return {
    focused: focused[0] ?? null,
    keyOwner:
      active === document.querySelector("#mounted-gui-canvas")
        ? "canvas"
        : active !== null && active === nativeEditor()
          ? "editor"
          : "other",
    presses,
    text,
    errors: [...errors],
  };
}

export async function closeGuiCanvas(): Promise<{
  readonly canvasCount: number;
  readonly editorCount: number;
}> {
  releaseCommitReply?.();
  restoreBatch?.();
  releaseCommitReply = undefined;
  heldCommitStarted = undefined;
  setCommitRevision = undefined;
  releaseText?.();
  releaseText = undefined;
  physical = undefined;
  const closing = handle;
  handle = undefined;
  panelWorld = undefined;
  rerenderEquivalent = undefined;
  const activeRoot = root;
  root = undefined;
  activeRoot?.unmount();
  if (closing !== undefined) await closing.closed;
  return {
    canvasCount: document.querySelectorAll("canvas").length,
    editorCount: document.querySelectorAll("textarea").length,
  };
}
