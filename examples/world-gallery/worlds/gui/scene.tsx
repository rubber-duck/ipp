import type {
  AnimationClipSource,
  AnimationControllerState,
  AnimationTrack,
  AnimationWorldClient,
  AssetResourceSnapshot,
  Client,
  ClientAssetSource,
  Command,
  GuiPickingBlocker,
  GuiWorldClient,
  Inspection,
  WorldReference,
} from "@ipp/client";
import {
  CanvasWorld,
  Entity,
  FragmentShader,
  ShaderAsset,
  Surface,
  SurfaceCache,
  Transform,
  VertexShader,
  type CanvasWorldHandle,
} from "@ipp/react";
import { World, type IppCanvasHandle } from "@ipp/react/web";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  HolographicProjector,
  PROJECTED_PANEL_TRANSFORM,
  projectorResourceSources,
  loadProjectorBeamSection,
  type ProjectorBeamSection,
  type ProjectorMotionAssets,
} from "./projector.js";
import BEAM_SHADER from "./projector-beam.glsl";
import BACKGROUND_SHADER from "./projector-background.glsl";
import BACKGROUND_VERTEX_SHADER from "./projector-background-vertex.glsl";
import BEAM_VERTEX_SHADER from "./projector-beam-vertex.glsl";
import GLOW_SHADER from "./projector-glow.glsl";
import METAL_SHADER from "./projector-metal.glsl";
import SHIELD_SHADER from "./input-shield.glsl";
import SHIELD_VERTEX_SHADER from "./input-shield-vertex.glsl";
import { InputShield, SHIELD_ENTITY } from "./shield.js";
import {
  INITIAL_AUTOSCAN,
  INITIAL_CALLSIGN,
  INITIAL_GAIN,
  PALETTES,
  ProjectorDashboard,
  SURFACE_HEIGHT,
  SURFACE_WIDTH,
  SWITCH_KNOB,
  THEME_ENTITIES,
  dashboardThemes,
  switchKnobColor,
  type Palette,
  type ThemeName,
} from "./dashboard.js";
import {
  WAVEFORM_ENTITIES,
  WaveformAnimations,
  waveformClips,
  waveformResourceSources,
  type WaveformMotionAssets,
} from "./waveform.js";

const FONT_URL = "/target/font-assets/shure-tech-mono.ippf";
/** Event log history bound: long enough that the log's VirtualList holds
 * many viewports of items while it declares only the visible few. */
const MAX_EVENTS = 256;
const STAGING_X = 1_000;

/** Symbolic ID of the Surface entity that presents the panel World. */
export const PANEL_ENTITY = "gui-demo";

/** Symbolic ID of the World whose canvas holds the panel's controls. */
export const PANEL_WORLD = "gui-demo-panel";

/** The panel World evaluates Canvas, GUI and animation; it does not render
 * 3D content of its own. */
const PANEL_SYSTEMS = [
  "ipp.animation",
  "ipp.gui",
  "ipp.gui-layout",
  "ipp.canvas",
  "ipp.asset-dependencies",
  "ipp.lifecycle-publisher",
] as const;

export { SHIELD_ENTITY };

export type GuiDemoSkin = "aurora" | "ember" | "neon";

/**
 * Presentation of the GUI Surface. `automatic` opts into distance-based
 * whole-Surface caching, `cached` caches at every distance for comparisons
 * at the authored camera, and `direct` removes the opt-in.
 */
export type GuiSurfaceCacheMode = "automatic" | "cached" | "direct";

/**
 * Whole-Surface cache policy for the GUI panel. The authored camera sits
 * about 16.7 m from the panel centre, so it presents directly; dollying out
 * past 22 m (the 20 m boundary plus hysteresis) caches the panel. 80 texels
 * per content metre (about 592x384 texels, 0.87 MiB) roughly matches the on-screen
 * density of a 720-pixel-high canvas at that boundary, and each further
 * distance doubling halves density and refresh. The scanning trace changes
 * paint every frame, so a cap near the host frame rate would repaint the
 * image on every frame at more than the cost of drawing directly; 15 Hz
 * keeps the distant trace readable at about half the repaints.
 */
const GUI_SURFACE_CACHE = {
  direct_distance: 20,
  texels_per_metre: 80,
  max_refresh_hz: 15,
} as const;

/** Archived operator messages. Short ones fit one event log line; the
 * longer ones wrap into two, so the log's items measure different extents. */
const ARCHIVE_MESSAGES = [
  "SUBSYSTEM CLOCKS SYNCED",
  "NAV LATTICE ALIGNED",
  "COOLANT LOOP B BALANCED AFTER PRESSURE DRIFT",
  "ARCHIVE CHANNEL SEALED",
  "SPECTRUM SWEEP NOMINAL",
  "RELAY HANDOFF TO GROUND STATION KESTREL COMPLETE",
  "ROUTE N7 ACQUIRED",
  "BEACON 12 SILENT",
  "STAR TRACKER REACQUIRED GUIDE STAR AFTER GLINT",
  "DEEP ARRAY LINK STABLE",
  "LENS HEATER CYCLED",
  "SPARE BUS ISOLATED WHILE FUSE F3 COOLS DOWN",
  "UPLINK WINDOW OPEN",
  "GAIN TRIM LOGGED",
] as const;

/** Archived entries the event log starts with, newest first. */
const ARCHIVE_LENGTH = 96;

function eventEntry(sequence: number, message: string): string {
  return `${String(sequence).padStart(3, "0")} // ${message}`;
}

const INITIAL_EVENTS: readonly string[] = Array.from(
  { length: ARCHIVE_LENGTH },
  (_, age) => {
    const sequence = ARCHIVE_LENGTH - age;
    return eventEntry(
      sequence,
      ARCHIVE_MESSAGES[(sequence * 5) % ARCHIVE_MESSAGES.length]!,
    );
  },
);

/** The values the dashboard's controls declare, as their f32 fields report
 * them. */
const INITIAL_CONTROL_VALUES = {
  autoscan: INITIAL_AUTOSCAN,
  gain: Math.fround(INITIAL_GAIN),
  callsign: INITIAL_CALLSIGN,
};

type GuiControlValues = typeof INITIAL_CONTROL_VALUES;

/** Items of the event log a VirtualList currently declares, `[first,
 * last)`, as its wanted-range callback last reported them. */
export interface GuiEventWindow {
  readonly first: number;
  readonly last: number;
}

interface MotionAssets extends ProjectorMotionAssets, WaveformMotionAssets {
  readonly aurora: ClientAssetSource;
  readonly ember: ClientAssetSource;
  readonly neon: ClientAssetSource;
}

interface MotionOwnership {
  readonly client: AnimationWorldClient;
  readonly assets: MotionAssets;
  released: boolean;
}

type PanelClient = AnimationWorldClient & GuiWorldClient;

/** An observation session on the panel World, opened after its attachment
 * is ready and closed with it. */
interface PanelSession {
  readonly world: WorldReference;
  readonly client: PanelClient;
}

export interface GuiSceneState {
  readonly ready: boolean;
  readonly vectorOnly: boolean;
  readonly prepared: boolean;
  readonly revealed: boolean;
  readonly error?: string;
  readonly skin: GuiDemoSkin;
  readonly autoscan: boolean;
  readonly wide: boolean;
  readonly surfaceCache: GuiSurfaceCacheMode;
  readonly gain: number;
  readonly callsign: string;
  readonly pulseSequence: number;
  readonly pulseActive: boolean;
  readonly lastCommand: string;
  /** Event log entries, newest first. */
  readonly events: readonly string[];
  /** Event log items the VirtualList declares for its wanted range. */
  readonly eventWindow: GuiEventWindow;
  readonly setEventWindow: (range: GuiEventWindow) => void;
  /** Whether the input shield in front of PURGE is armed. */
  readonly shieldArmed: boolean;
  /** Scene blockers for `IppCanvas.guiInput`: the armed, mounted shield's
   * exact picking geometry. */
  readonly blockers: readonly GuiPickingBlocker[];
  readonly motions?: MotionAssets;
  readonly beamSection?: ProjectorBeamSection;
  readonly font: ClientAssetSource;
  readonly onCommit: () => void;
  readonly attachPanel: (handle: CanvasWorldHandle) => void;
  readonly pulse: () => void;
  readonly uplink: () => void;
  readonly purge: () => void;
  readonly toggleShield: () => void;
  readonly toggleSpan: () => void;
  readonly selectSkin: (skin: GuiDemoSkin) => void;
  readonly selectSurfaceCache: (mode: GuiSurfaceCacheMode) => void;
  readonly toggleVectorOnly: () => void;
  readonly setAutoscan: (value: boolean) => void;
  readonly setGain: (value: number) => void;
  readonly setCallsign: (value: string) => void;
  readonly setPulseActive: (active: boolean) => void;
  readonly readWaveformPulse: () => Promise<AnimationControllerState>;
  readonly reportFailure: (failure: unknown) => void;
}

function absoluteAsset(kind: number, path: string): ClientAssetSource {
  return {
    kind,
    source: new URL(path, globalThis.location.href).href,
  };
}

function errorMessage(failure: unknown): string {
  return failure instanceof Error ? failure.message : String(failure);
}

/** Skin motion channels in track order. The clip tracks carry typed samples
 * only: the runtime binds consecutive tracks to each transitioning control's
 * own colour, opacity, scale and alignment channels. */
const SKIN_CHANNELS = ["color", "opacity", "scale", "align_x"] as const;

function channelTrack(
  material: number,
  channel: (typeof SKIN_CHANNELS)[number],
  values: readonly (readonly [number, unknown])[],
): AnimationTrack {
  return {
    property: { component: material, name: `skin_${channel}` },
    keys: values.map(([time, value], index) => ({
      time,
      value: {
        kind: "dynamic" as const,
        value:
          channel === "color"
            ? {
                kind: "vec4" as const,
                value: value as readonly [number, number, number, number],
              }
            : channel === "scale"
              ? {
                  kind: "vec2" as const,
                  value: value as readonly [number, number],
                }
              : { kind: "f32" as const, value: value as number },
      },
      ...(index + 1 < values.length
        ? { interpolation: { kind: "linear" as const } }
        : {}),
    })),
  };
}

/** Complete color/opacity/scale samples for every interaction destination,
 * then the SCAN switch knob's two ends, which also animate alignment. Other
 * controls hold alignment at 0 and never sample the knob times. */
function skinMotion(material: number, palette: Palette): AnimationClipSource {
  const knob = [SWITCH_KNOB.scale, SWITCH_KNOB.scale] as const;
  const samples = [
    { time: 0, color: palette.button, opacity: 1, scale: [1, 1] as const },
    {
      time: 0.1,
      color: palette.hovered,
      opacity: 1,
      scale: [1.025, 1.025] as const,
    },
    {
      time: 0.2,
      color: palette.pressed,
      opacity: 1,
      scale: [0.985, 0.985] as const,
    },
    {
      time: 0.3,
      color: palette.disabled,
      opacity: 0.45,
      scale: [1, 1] as const,
    },
    {
      time: SWITCH_KNOB.unchecked.time,
      color: switchKnobColor(palette, false),
      opacity: 1,
      scale: knob,
      alignX: SWITCH_KNOB.unchecked.alignX,
    },
    {
      time: SWITCH_KNOB.checked.time,
      color: switchKnobColor(palette, true),
      opacity: 1,
      scale: knob,
      alignX: SWITCH_KNOB.checked.alignX,
    },
  ] as const;
  return {
    duration: SWITCH_KNOB.checked.time,
    tracks: [
      channelTrack(
        material,
        "color",
        samples.map(({ time, color }) => [time, color] as const),
      ),
      channelTrack(
        material,
        "opacity",
        samples.map(({ time, opacity }) => [time, opacity] as const),
      ),
      channelTrack(
        material,
        "scale",
        samples.map(({ time, scale }) => [time, scale] as const),
      ),
      channelTrack(
        material,
        "align_x",
        samples.map(
          (sample) =>
            [sample.time, "alignX" in sample ? sample.alignX : 0] as const,
        ),
      ),
    ],
  };
}

async function createMotionAssets(
  client: AnimationWorldClient,
): Promise<MotionAssets> {
  const canvasStyle = client.components.CanvasStyle;
  const offset = canvasStyle?.fields.x?.offset;
  const material = client.components.CustomMaterial?.id;
  if (canvasStyle === undefined || offset === undefined)
    throw new Error("The gallery GUI profile does not expose CanvasStyle");
  const translation = { component: canvasStyle.id, offset };
  if (material === undefined)
    throw new Error("The gallery GUI profile does not expose CustomMaterial");
  const created: ClientAssetSource[] = [];
  const create = async (clip: AnimationClipSource) => {
    const bytes = client.encodeAnimationClip(clip);
    created.push(await client.createAsset(10, bytes.slice().buffer));
  };
  try {
    for (const name of ["aurora", "ember", "neon"] as const)
      await create(skinMotion(material, PALETTES[name]));
    for (const clip of waveformClips(translation)) await create(clip);
    await create({
      duration: 40,
      tracks: [
        {
          property: { component: material, name: "phase" },
          keys: [
            {
              time: 0,
              value: { kind: "dynamic", value: { kind: "f32", value: 0 } },
              interpolation: { kind: "linear" },
            },
            {
              time: 40,
              value: {
                kind: "dynamic",
                value: { kind: "f32", value: Math.PI * 2 },
              },
            },
          ],
        },
      ],
    });
    return {
      aurora: created[0]!,
      ember: created[1]!,
      neon: created[2]!,
      scan: created[3]!,
      wavePulse: created[4]!,
      dust: created[5]!,
      translation,
      materialComponent: material,
    };
  } catch (failure) {
    await Promise.allSettled(
      created.map((asset) => client.releaseAsset(asset)),
    );
    throw failure;
  }
}

async function releaseMotionOwnership(
  ownership: MotionOwnership,
): Promise<void> {
  if (ownership.released) return;
  ownership.released = true;
  await Promise.allSettled(
    [
      ownership.assets.aurora,
      ownership.assets.ember,
      ownership.assets.neon,
      ownership.assets.scan,
      ownership.assets.wavePulse,
      ownership.assets.dust,
    ].map((asset) => ownership.client.releaseAsset(asset)),
  );
}

/**
 * Wait until the projector resources are loaded, then for a completed frame
 * that drew them without failed draws. Readiness comes from inspection and
 * the presented frame summary; no pixels are read back.
 */
async function awaitCompleteProjectorFrame(
  canvas: IppCanvasHandle,
  active: () => boolean,
): Promise<void> {
  const client = canvas.client;
  for (let attempt = 0; attempt < 120; attempt += 1) {
    if (!active()) return;
    await canvas.flush();
    const inspection = await client.inspect();
    if (!active()) return;
    const resources = projectorResourceSources();
    const projector = resources.map(({ kind, source }) =>
      inspection.resources.find(
        (resource) => resource.kind === kind && resource.source === source,
      ),
    );
    const material = client.components.CustomMaterial!.id;
    const shaderSources = [
      "gui-projector-background",
      "gui-projector-core",
      "gui-projector-emitter",
      "gui-projector-beam",
      SHIELD_ENTITY,
    ].map(
      (symbolicId) =>
        inspection.entities
          .find((entity) => entity.metadata.symbolicId === symbolicId)
          ?.components.find(({ component }) => component === material)?.fields
          .source,
    );
    const shaders = shaderSources.map((source) =>
      typeof source === "string"
        ? inspection.resources.find(
            (resource) => resource.kind === 13 && resource.source === source,
          )
        : undefined,
    );
    const failed = [...projector, ...shaders].find(
      (resource) => resource?.status === "failed",
    );
    if (failed)
      throw new Error(
        `Projector resource ${failed.source} failed: ${failed.error ?? "unknown error"}`,
      );
    if (
      projector.every((resource) => resource?.status === "loaded") &&
      shaders.every((resource) => resource?.status === "loaded")
    ) {
      const frame = await canvas.frame();
      if (!active()) return;
      if (frame.failedDrawCalls === 0 && frame.drawCalls > 0) return;
    }
    await client.waitForFrame(inspection.tick);
  }
  throw new Error("Projector resources did not produce a complete frame");
}

/**
 * Author each skinned theme's motion rows beside its GuiTheme: the
 * transitions sample the selected skin's clip. The theme entities are
 * declared by the panel's React root; their skin motion is an ordinary
 * component this observation session inserts once and then rewrites as a
 * whole table when the skin changes.
 */
async function writeThemeMotion(
  panel: PanelClient,
  skin: GuiDemoSkin,
  motion: string,
  active: () => boolean,
): Promise<void> {
  const component = panel.components.GuiThemeMotion;
  const parts = component?.fields.parts;
  if (!component || !parts)
    throw new Error("The gallery GUI profile does not expose GuiThemeMotion");
  const themes = dashboardThemes(skin, motion);
  for (let attempt = 0; attempt < 120; attempt += 1) {
    if (!active()) return;
    const inspection = await panel.inspect();
    if (!active()) return;
    const commands: Command[] = [];
    let declared = true;
    for (const name of Object.keys(THEME_ENTITIES) as ThemeName[]) {
      const rows = themes[name].motion;
      if (!rows) continue;
      const entity = inspection.entities.find(
        ({ metadata }) => metadata.symbolicId === THEME_ENTITIES[name],
      );
      if (!entity) {
        declared = false;
        break;
      }
      const field = {
        offset: parts.offset,
        value: { kind: "rows" as const, value: rows },
      };
      const target = { kind: "handle" as const, id: entity.id };
      commands.push(
        entity.components.some(({ component: id }) => id === component.id)
          ? { kind: "setField", entity: target, component: component.id, field }
          : {
              kind: "insertComponent",
              entity: target,
              component: component.id,
              fields: [field],
            },
      );
    }
    if (declared) {
      const outcome = await panel.batch(commands);
      if (!outcome.ok)
        throw new Error(`Skin motion was rejected: ${outcome.error.reason}`);
      return;
    }
    await panel.waitForFrame(inspection.tick);
  }
  throw new Error("The GUI theme entities were not declared");
}

/** GUI control components, whose entities also carry GuiBehavior. */
const CONTROL_COMPONENTS = [
  "GuiButton",
  "GuiCheckbox",
  "GuiSlider",
  "GuiTextInput",
  "GuiScrollView",
  "GuiVirtualList",
] as const;

/** Whether the panel World has controls and each is evaluated visible. */
function controlsVisible(panel: PanelClient, inspection: Inspection): boolean {
  const controls = new Set(
    CONTROL_COMPONENTS.map((name) => panel.components[name]?.id),
  );
  const behavior = panel.components.GuiBehavior?.id;
  let found = false;
  for (const entity of inspection.entities) {
    if (!entity.components.some(({ component }) => controls.has(component)))
      continue;
    found = true;
    const fields = entity.components.find(
      ({ component }) => component === behavior,
    )?.fields;
    if (fields?.effective_visible !== true) return false;
  }
  return found;
}

/**
 * Wait until the panel World has committed its prepared controls and bound
 * both waveform controllers, so the reveal never places a panel whose
 * controls are still hidden.
 */
async function awaitPreparedPanel(
  panel: PanelClient,
  motions: MotionAssets,
  active: () => boolean,
): Promise<boolean> {
  for (let attempt = 0; attempt < 120; attempt += 1) {
    if (!active()) return false;
    const inspection = await panel.inspect();
    if (!active()) return false;
    const targets = [WAVEFORM_ENTITIES.signal, WAVEFORM_ENTITIES.pulse].map(
      (symbolicId) =>
        inspection.entities.find(
          ({ metadata }) => metadata.symbolicId === symbolicId,
        )?.id,
    );
    const bound = [motions.scan, motions.wavePulse].every((clip, index) =>
      inspection.controllers?.some(({ description }) =>
        description.drivers.some(
          (driver) =>
            driver.source === clip.source &&
            targets[index] !== undefined &&
            driver.target === targets[index],
        ),
      ),
    );
    if (bound && controlsVisible(panel, inspection)) return true;
    await panel.waitForFrame(inspection.tick);
  }
  throw new Error("The GUI panel did not commit its prepared controls");
}

/** Page through the entity collection for one symbolic ID. */
async function findEntity(
  client: Client,
  symbolicId: string,
): Promise<bigint | undefined> {
  let after = 0n;
  do {
    const page = await client.inspectPage({ collection: "entities", after });
    const entity = page.entities.find(
      ({ metadata }) => metadata.symbolicId === symbolicId,
    );
    if (entity) return entity.id;
    after = page.next;
  } while (after !== 0n);
  return undefined;
}

/** The mounted shield's exact PickingGeometry incarnation, read from the
 * baseline of a one-shot lifecycle watch. */
async function resolveShieldBlocker(
  client: Client,
): Promise<GuiPickingBlocker | undefined> {
  const world = client.worldReference;
  const picking = client.components.PickingGeometry?.id;
  if (!world || picking === undefined)
    throw new Error("The gallery World does not expose picking geometry");
  const entity = await findEntity(client, SHIELD_ENTITY);
  if (entity === undefined) return undefined;
  const watch = await client.watchLifecycle(
    [{ target: { kind: "component", entity, component: picking }, kinds: 8 }],
    () => {},
  );
  try {
    const lifetime = watch.baselines[0]?.lifetime;
    return lifetime?.kind === "component" && lifetime.incarnation !== null
      ? { world, entity, incarnation: lifetime.incarnation }
      : undefined;
  } finally {
    await watch.remove();
  }
}

function assetKey(asset: ClientAssetSource): string {
  return `${asset.kind}:${asset.variant ?? 0}:${asset.source}`;
}

/** Subscribe before inspection so readiness cannot race the initial snapshot.
 * Each World reports the resources it demands. */
function observeEssentialResources(
  demands: readonly {
    readonly client: AnimationWorldClient;
    readonly assets: readonly ClientAssetSource[];
  }[],
  active: () => boolean,
  ready: () => void,
  failed: (message: string) => void,
): () => void {
  const expected = new Set(
    demands.flatMap(({ assets }, index) =>
      assets.map((asset) => `${index}/${assetKey(asset)}`),
    ),
  );
  const observed = new Map<string, AssetResourceSnapshot>();
  const publish = () => {
    if (!active()) return;
    const resources = [...expected].map((key) => observed.get(key));
    const failure = resources.find((resource) => resource?.status === "failed");
    if (failure) {
      failed(
        `GUI demo resource ${failure.source} failed: ${failure.error ?? "unknown error"}`,
      );
    } else if (
      resources.length === expected.size &&
      resources.every((resource) => resource?.status === "loaded")
    ) {
      ready();
    }
  };
  const unsubscribers = demands.map(({ client }, index) => {
    const record = (resource: AssetResourceSnapshot, replace: boolean) => {
      const key = `${index}/${assetKey(resource)}`;
      if (!expected.has(key) || (!replace && observed.has(key))) return;
      observed.set(key, resource);
    };
    const unsubscribe = client.onResourceChange((resource) => {
      record(resource, true);
      publish();
    });
    void client.inspect().then(
      (inspection) => {
        if (!active()) return;
        for (const resource of inspection.resources) record(resource, false);
        publish();
      },
      (failure: unknown) => {
        if (active()) failed(errorMessage(failure));
      },
    );
    return unsubscribe;
  });
  return () => {
    for (const unsubscribe of unsubscribers) unsubscribe();
  };
}

export function useGuiScene(
  canvas: IppCanvasHandle | undefined,
  active: boolean,
): GuiSceneState {
  const [ready, setReady] = useState(false);
  const [vectorOnly, setVectorOnly] = useState(false);
  const [prepared, setPrepared] = useState(false);
  const [revealed, setRevealed] = useState(false);
  const [treeCommitted, setTreeCommitted] = useState(false);
  const [panel, setPanel] = useState<PanelSession>();
  const [error, setError] = useState<string>();
  const [motions, setMotions] = useState<MotionAssets>();
  const [beamSection, setBeamSection] = useState<ProjectorBeamSection>();
  const [skin, setSkin] = useState<GuiDemoSkin>("aurora");
  const [autoscan, setAutoscanState] = useState(INITIAL_AUTOSCAN);
  const [wide, setWide] = useState(false);
  const [surfaceCache, setSurfaceCache] =
    useState<GuiSurfaceCacheMode>("automatic");
  const [gain, setGainState] = useState(INITIAL_GAIN);
  const [callsign, setCallsignState] = useState(INITIAL_CALLSIGN);
  const [lastCommand, setLastCommand] = useState("Awaiting command");
  const [pulseSequence, setPulseSequence] = useState(0);
  const [pulseActive, setPulseActive] = useState(false);
  const [events, setEvents] = useState<readonly string[]>(INITIAL_EVENTS);
  const [eventWindow, setEventWindowState] = useState<GuiEventWindow>({
    first: 0,
    last: 0,
  });
  const [shieldArmed, setShieldArmed] = useState(true);
  const [shieldBlocker, setShieldBlocker] = useState<GuiPickingBlocker>();
  const shieldRequest = useRef(0);
  const sequence = useRef(INITIAL_EVENTS.length);
  // Value callbacks report a control's current value when they register and
  // then each change; only a value that differs from the one the scene holds
  // is an operator event for the log.
  const controlValues = useRef<GuiControlValues>(INITIAL_CONTROL_VALUES);
  const generation = useRef(0);
  const finishingFrame = useRef(false);
  const motionOwnership = useRef<MotionOwnership | undefined>(undefined);
  const panelRequest = useRef(0);

  const releaseMotions = useCallback(async () => {
    const ownership = motionOwnership.current;
    if (!ownership || ownership.released) return;
    await releaseMotionOwnership(ownership);
    if (motionOwnership.current === ownership)
      motionOwnership.current = undefined;
  }, []);

  const record = useCallback((message: string) => {
    sequence.current += 1;
    // Number the entry now: records batched before the next render each
    // keep their own sequence.
    const entry = eventEntry(sequence.current, message);
    setEvents((current) => [entry, ...current].slice(0, MAX_EVENTS));
  }, []);

  useEffect(() => {
    const request = ++generation.current;
    setReady(false);
    setVectorOnly(false);
    setPrepared(false);
    setRevealed(false);
    setTreeCommitted(false);
    setPanel(undefined);
    setError(undefined);
    setMotions(undefined);
    setBeamSection(undefined);
    setShieldBlocker(undefined);
    shieldRequest.current += 1;
    finishingFrame.current = false;
    if (!canvas || !active) return;
    setSkin("aurora");
    setAutoscanState(INITIAL_AUTOSCAN);
    setWide(false);
    setSurfaceCache("automatic");
    setGainState(INITIAL_GAIN);
    setCallsignState(INITIAL_CALLSIGN);
    setLastCommand("Awaiting command");
    setPulseSequence(0);
    setPulseActive(false);
    setEvents(INITIAL_EVENTS);
    setEventWindowState({ first: 0, last: 0 });
    setShieldArmed(true);
    sequence.current = INITIAL_EVENTS.length;
    controlValues.current = INITIAL_CONTROL_VALUES;
    const client = canvas.client as AnimationWorldClient;
    if (
      !client.capabilities.gui ||
      !client.capabilities.surfaces ||
      !client.capabilities.animation
    ) {
      setError("This gallery runtime does not include the GUI demo");
      return;
    }
    let disposed = false;
    const controller = new AbortController();
    let owned: MotionOwnership | undefined;
    void loadProjectorBeamSection(controller.signal)
      .then(async (section) => ({
        section,
        created: await createMotionAssets(client),
      }))
      .then(({ section, created }) => {
        const ownership = { client, assets: created, released: false };
        owned = ownership;
        if (disposed || generation.current !== request) {
          void releaseMotionOwnership(ownership);
          return;
        }
        motionOwnership.current = ownership;
        setBeamSection(section);
        setMotions(created);
      })
      .catch((failure: unknown) => {
        if (!disposed) setError(errorMessage(failure));
      });
    return () => {
      disposed = true;
      controller.abort();
      if (generation.current === request) generation.current += 1;
      if (owned) {
        void releaseMotionOwnership(owned);
        if (motionOwnership.current === owned)
          motionOwnership.current = undefined;
      }
    };
  }, [canvas, active, releaseMotions]);

  useEffect(() => {
    if (!error) return;
    setMotions(undefined);
    void releaseMotions();
  }, [error, releaseMotions]);

  // The observation session closes with the panel World's attachment, or
  // when the scene stops observing it.
  useEffect(() => {
    if (!panel) return;
    return () => {
      void panel.client.close().catch(() => {});
    };
  }, [panel]);

  const attachPanel = useCallback(
    (handle: CanvasWorldHandle) => {
      if (!canvas || !active) return;
      const request = ++panelRequest.current;
      const current = generation.current;
      void (async () => {
        const client = (await canvas.host.openWorld(
          handle.world,
        )) as PanelClient;
        if (
          panelRequest.current !== request ||
          generation.current !== current
        ) {
          await client.close();
          return;
        }
        setPanel({ world: handle.world, client });
        void handle.closed.then(() => {
          if (panelRequest.current === request) setPanel(undefined);
        });
      })().catch((failure: unknown) => {
        if (generation.current === current) setError(errorMessage(failure));
      });
    },
    [canvas, active],
  );

  useEffect(() => {
    if (
      !canvas ||
      !active ||
      !motions ||
      !treeCommitted ||
      !panel ||
      error ||
      prepared
    )
      return;
    const request = generation.current;
    return observeEssentialResources(
      [
        {
          client: canvas.client as AnimationWorldClient,
          assets: [motions.dust, ...projectorResourceSources()],
        },
        {
          client: panel.client,
          assets: [
            absoluteAsset(17, FONT_URL),
            ...waveformResourceSources(),
            motions.scan,
            motions.wavePulse,
          ],
        },
      ],
      () => generation.current === request,
      () => setPrepared(true),
      (message) => setError(message),
    );
  }, [canvas, active, motions, treeCommitted, panel, error, prepared]);

  // Reveal the staged assembly once the panel World has committed its
  // prepared controls and both waveform controllers.
  useEffect(() => {
    if (!prepared || revealed || !panel || !motions || error) return;
    const request = generation.current;
    const current = () => generation.current === request;
    void awaitPreparedPanel(panel.client, motions, current).then(
      (bound) => {
        if (bound && current()) setRevealed(true);
      },
      (failure: unknown) => {
        if (current()) setError(errorMessage(failure));
      },
    );
  }, [prepared, revealed, panel, motions, error]);

  // Skin transitions follow the selected skin's clip.
  useEffect(() => {
    if (!panel || !motions || error) return;
    let active = true;
    void writeThemeMotion(
      panel.client,
      skin,
      motions[skin].source,
      () => active,
    ).catch((failure: unknown) => {
      if (active) setError(errorMessage(failure));
    });
    return () => {
      active = false;
    };
  }, [panel, motions, skin, error]);

  const readWaveformPulse = useCallback(async () => {
    if (!panel || !motions) throw new Error("The waveform World is not active");
    const inspection = await panel.client.inspect();
    const controller = inspection.controllers?.find(({ description }) =>
      description.drivers.some(
        ({ source }) => source === motions.wavePulse.source,
      ),
    );
    if (!controller)
      throw new Error("The waveform pulse controller is no longer mounted");
    return controller;
  }, [panel, motions]);

  const onCommit = useCallback(() => {
    if (!canvas || !active || error) return;
    if (revealed && !vectorOnly && shieldBlocker === undefined) {
      // This commit mounted the shield with the revealed assembly; name its
      // exact picking geometry so the canvas can mark it as a GUI input
      // blocker.
      const request = ++shieldRequest.current;
      void resolveShieldBlocker(canvas.client).then(
        (blocker) => {
          if (shieldRequest.current !== request) return;
          if (blocker === undefined)
            setError("The GUI input shield is not mounted");
          else setShieldBlocker(blocker);
        },
        (failure: unknown) => {
          if (shieldRequest.current === request)
            setError(errorMessage(failure));
        },
      );
    }
    if (ready) return;
    if (!revealed) {
      if (!treeCommitted) setTreeCommitted(true);
      return;
    }
    if (finishingFrame.current) return;
    finishingFrame.current = true;
    const request = generation.current;
    void awaitCompleteProjectorFrame(
      canvas,
      () => generation.current === request,
    ).then(
      () => {
        if (generation.current === request) setReady(true);
      },
      (failure: unknown) => {
        if (generation.current === request) setError(errorMessage(failure));
      },
    );
  }, [
    canvas,
    active,
    error,
    ready,
    revealed,
    treeCommitted,
    vectorOnly,
    shieldBlocker,
  ]);

  const blockers = useMemo<readonly GuiPickingBlocker[]>(
    () =>
      shieldArmed && shieldBlocker !== undefined && !vectorOnly
        ? [shieldBlocker]
        : [],
    [shieldArmed, shieldBlocker, vectorOnly],
  );

  const pulse = useCallback(() => {
    setPulseSequence((current) => current + 1);
    setLastCommand("Pulse sent");
    record("PULSE BURST COMMITTED");
  }, [record]);
  const uplink = useCallback(() => {
    setLastCommand("Uplink sent");
    record("UPLINK PACKET QUEUED");
  }, [record]);
  // PURGE leaves one entry. The runtime clamps the log's scroll position to
  // the remaining content, and later growth never returns to the discarded
  // position.
  const purge = useCallback(() => {
    sequence.current += 1;
    setEvents([eventEntry(sequence.current, "LOG PURGED")]);
    setLastCommand("Log purged");
  }, []);
  const setEventWindow = useCallback((range: GuiEventWindow) => {
    setEventWindowState((current) =>
      current.first === range.first && current.last === range.last
        ? current
        : { first: range.first, last: range.last },
    );
  }, []);
  const toggleShield = useCallback(() => {
    setShieldArmed(!shieldArmed);
    record(shieldArmed ? "SHIELD LIFTED" : "SHIELD ARMED");
  }, [shieldArmed, record]);
  const toggleSpan = useCallback(() => {
    setWide(!wide);
    record(`SPAN ${wide ? "NARROW" : "WIDE"}`);
  }, [wide, record]);
  const toggleVectorOnly = useCallback(() => {
    // Isolation unmounts the shield with the projector; its next mount is a
    // new entity, resolved again after that commit.
    shieldRequest.current += 1;
    setShieldBlocker(undefined);
    setVectorOnly((current) => !current);
  }, []);
  const selectSkin = useCallback(
    (next: GuiDemoSkin) => {
      setSkin(next);
      record(`${next.toUpperCase()} SKIN ONLINE`);
    },
    [record],
  );
  const changedValue = useCallback(
    <Key extends keyof GuiControlValues>(
      key: Key,
      value: GuiControlValues[Key],
    ) => {
      if (controlValues.current[key] === value) return false;
      controlValues.current = { ...controlValues.current, [key]: value };
      return true;
    },
    [],
  );
  const setAutoscan = useCallback(
    (value: boolean) => {
      setAutoscanState(value);
      if (changedValue("autoscan", value))
        record(`AUTOSCAN ${value ? "ENABLED" : "STANDBY"}`);
    },
    [changedValue, record],
  );
  const setGain = useCallback(
    (value: number) => {
      setGainState(value);
      if (changedValue("gain", value))
        record(`SIGNAL GAIN ${Math.round(value * 100)} PERCENT`);
    },
    [changedValue, record],
  );
  const setCallsign = useCallback(
    (value: string) => {
      setCallsignState(value);
      if (changedValue("callsign", value))
        record(`CALLSIGN ${value || "CLEARED"}`);
    },
    [changedValue, record],
  );
  const reportFailure = useCallback((failure: unknown) => {
    setError(errorMessage(failure));
  }, []);

  return {
    ready,
    vectorOnly,
    prepared,
    revealed,
    ...(error ? { error } : {}),
    skin,
    autoscan,
    wide,
    surfaceCache,
    gain,
    callsign,
    pulseSequence,
    pulseActive,
    lastCommand,
    events,
    eventWindow,
    setEventWindow,
    shieldArmed,
    blockers,
    ...(motions ? { motions } : {}),
    ...(beamSection ? { beamSection } : {}),
    font: absoluteAsset(17, FONT_URL),
    onCommit,
    attachPanel,
    pulse,
    uplink,
    purge,
    toggleShield,
    toggleSpan,
    toggleVectorOnly,
    selectSkin,
    selectSurfaceCache: setSurfaceCache,
    setAutoscan,
    setGain,
    setCallsign,
    setPulseActive,
    readWaveformPulse,
    reportFailure,
  };
}

/**
 * The GUI page's World root. Unmounting a React root deletes nothing, so once
 * the page has shown its content the root stays mounted and leaving the page
 * removes its declarations, which deletes the entities, assets and panel World
 * they created. Before the content first shows, nothing mounts, so the scene's
 * commit callback only ever sees commits of the page's own declarations.
 */
export function GuiWorld({
  scene,
  active,
}: {
  scene: GuiSceneState;
  active: boolean;
}) {
  const shown =
    active &&
    scene.motions !== undefined &&
    scene.beamSection !== undefined &&
    !scene.error;
  const [mounted, setMounted] = useState(false);
  if (shown && !mounted) setMounted(true);
  if (!shown && !mounted) return null;
  return (
    <World onCommit={scene.onCommit}>
      {shown ? <GuiWorldContent scene={scene} /> : null}
    </World>
  );
}

function GuiWorldContent({ scene }: { scene: GuiSceneState }) {
  if (!scene.motions || !scene.beamSection || scene.error) return null;
  const stagingX = scene.revealed ? 0 : STAGING_X;
  return (
    <>
      <ShaderAsset
        id="gui-projector-background-shader"
        recipe={{}}
        parameters={{ visible: "f32" }}
      >
        <VertexShader>{BACKGROUND_VERTEX_SHADER}</VertexShader>
        <FragmentShader>{BACKGROUND_SHADER}</FragmentShader>
      </ShaderAsset>
      <ShaderAsset
        id="gui-projector-metal"
        recipe={{ normals: true, lighting: true }}
        parameters={{ base: "texture2D", accent: "vec4", energy: "f32" }}
      >
        <FragmentShader requiredAttributes={6}>{METAL_SHADER}</FragmentShader>
      </ShaderAsset>
      <ShaderAsset
        id="gui-projector-glow"
        recipe={{ normals: true, lighting: true }}
        parameters={{ accent: "vec4", energy: "f32" }}
      >
        <FragmentShader requiredAttributes={6}>{GLOW_SHADER}</FragmentShader>
      </ShaderAsset>
      <ShaderAsset
        id="gui-projector-beam-shader"
        recipe={{ lighting: true }}
        parameters={{
          accent: "vec4",
          energy: "f32",
          section: "vec4",
          depth: "vec2",
          phase: "f32",
        }}
      >
        <VertexShader requiredAttributes={2}>{BEAM_VERTEX_SHADER}</VertexShader>
        <FragmentShader requiredAttributes={2}>{BEAM_SHADER}</FragmentShader>
      </ShaderAsset>
      <ShaderAsset
        id="gui-input-shield-shader"
        recipe={{}}
        parameters={{ size: "vec2", color: "vec4", hatch: "f32" }}
      >
        <VertexShader>{SHIELD_VERTEX_SHADER}</VertexShader>
        <FragmentShader>{SHIELD_SHADER}</FragmentShader>
      </ShaderAsset>
      {!scene.vectorOnly && (
        <>
          <HolographicProjector scene={scene} stagingX={stagingX} />
          <InputShield armed={scene.shieldArmed} stagingX={stagingX} />
        </>
      )}
      <ProjectorPanel scene={scene} stagingX={stagingX} />
    </>
  );
}

/**
 * The projected panel: a Surface on a parent entity presents the canvas of
 * a separate panel World. The parent owns the Surface size, placement and
 * cache policy; the panel World owns the canvas density and content.
 */
function ProjectorPanel({
  scene,
  stagingX,
}: {
  scene: GuiSceneState;
  stagingX: number;
}) {
  const { x: panelX, ...panelTransform } = PROJECTED_PANEL_TRANSFORM;
  return (
    <>
      <Entity id={PANEL_ENTITY}>
        <Transform {...panelTransform} x={panelX + stagingX} />
        <Surface width={SURFACE_WIDTH} height={SURFACE_HEIGHT} />
        {scene.surfaceCache !== "direct" && (
          <SurfaceCache
            {...GUI_SURFACE_CACHE}
            direct_distance={
              scene.surfaceCache === "cached"
                ? 0
                : GUI_SURFACE_CACHE.direct_distance
            }
          />
        )}
      </Entity>
      <CanvasWorld
        presentation={{ anchor: PANEL_ENTITY }}
        create={{ symbolicId: PANEL_WORLD, selectedSystems: PANEL_SYSTEMS }}
        extent={[SURFACE_WIDTH, SURFACE_HEIGHT]}
        unitsPerMetre={1}
        onReady={scene.attachPanel}
        onError={scene.reportFailure}
      >
        <ProjectorDashboard scene={scene} />
        <WaveformAnimations scene={scene} />
      </CanvasWorld>
    </>
  );
}
