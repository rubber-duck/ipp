import type {
  AnimationClipSource,
  AnimationControllerState,
  AnimationWorldClient,
  AssetResourceSnapshot,
  Client,
  ClientAssetSource,
  GuiPickingBlocker,
  GuiWorldClient,
  Inspection,
  WorldReference,
} from "@ipp/client";
import {
  Animation,
  CanvasWorld,
  Entity,
  FragmentShader,
  ShaderAsset,
  Surface,
  SurfaceCache,
  Transform,
  VertexShader,
  type AnimationHandle,
  type CanvasWorldHandle,
} from "@ipp/react";
import type { GuiControlHandle } from "@ipp/react/gui";
import { World, type IppCanvasHandle } from "@ipp/react/web";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ACCENT_HSV,
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
import DUST_SHADER from "./projector-dust.glsl";
import DUST_VERTEX_SHADER from "./projector-dust-vertex.glsl";
import GLOW_SHADER from "./projector-glow.glsl";
import METAL_SHADER from "./projector-metal.glsl";
import SHIELD_SHADER from "./input-shield.glsl";
import SHIELD_VERTEX_SHADER from "./input-shield-vertex.glsl";
import { InputShield, SHIELD_ENTITY } from "./shield.js";
import { CANVAS_ENTITY, ProjectorDashboard } from "./dashboard.js";
import { INITIAL_AUTOSCAN, INITIAL_CALLSIGN, INITIAL_GAIN } from "./monitor.js";
import {
  CANVAS_HEIGHT,
  CANVAS_WIDTH,
  SURFACE_HEIGHT,
  SURFACE_WIDTH,
  UNITS_PER_METRE,
} from "./presentation.js";
import { useStation, type HostFrames, type Station } from "./station.js";
import { useTuning, type TuningState } from "./tuning.js";
import {
  WAVEFORM_ENTITIES,
  WaveformAnimations,
  sweepClip,
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

/** The primary actions' accent: the button look or the amber look. */
export type Accent = "cyan" | "amber";

/** The signal monitor's window: open, minimised to its title bar, or closed. */
export type MonitorWindow = "normal" | "minimized" | "closed";

/** The right column's tabs. */
export type WorkbenchTab = "nodes" | "controls" | "colour";

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

/**
 * The exploded view: Surface metres per canvas plane id, and the seconds
 * the panel takes to separate or close. A plane stands its id times the
 * spacing in front of the base plane, so the toasts' plane stays in frame
 * while the anchored overlays' plane separates visibly. The Surface's scale
 * applies to the spacing, so planes stand 0.7 times as far apart in the
 * World.
 */
export const LAYER_SPACING = 0.2;
export const LAYER_SECONDS = 0.6;

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
  "STAR TRACKER REACQUIRED GUIDE STAR",
  "DEEP ARRAY LINK STABLE",
  "LENS HEATER CYCLED",
  "SPARE BUS ISOLATED WHILE FUSE F3 COOLS",
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
  exploded: false,
  reducedMotion: false,
};

type GuiControlValues = typeof INITIAL_CONTROL_VALUES;

/** Items of the event log a VirtualList currently declares, `[first,
 * last)`, as its wanted-range callback last reported them. */
export interface GuiEventWindow {
  readonly first: number;
  readonly last: number;
}

/** A field of a component, as an animation binding names it. */
interface FieldTarget {
  readonly component: number;
  readonly offset: number;
}

/** The exploded view's clip: the Surface's layer spacing. */
interface LayerAssets {
  readonly spacing: ClientAssetSource;
  readonly spacingField: FieldTarget;
}

type MotionAssets = ProjectorMotionAssets & WaveformMotionAssets & LayerAssets;

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
  readonly accent: Accent;
  readonly exploded: boolean;
  readonly reducedMotion: boolean;
  readonly monitorWindow: MonitorWindow;
  readonly advancedOpen: boolean;
  readonly workbenchTab: WorkbenchTab;
  readonly autoscan: boolean;
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
  readonly station: Station;
  /** The projection's tuning: the CONTROLS and COLOUR tabs, the scope
   * popover and the scene tree. */
  readonly tuning: TuningState;
  /** Handles of controls the scene writes as the operator would. */
  readonly scanControl: (handle: GuiControlHandle | null) => void;
  readonly callsignControl: (handle: GuiControlHandle | null) => void;
  readonly explodeControl: (handle: GuiControlHandle | null) => void;
  readonly onCommit: () => void;
  readonly attachPanel: (handle: CanvasWorldHandle) => void;
  readonly pulse: () => void;
  readonly clearLog: () => void;
  readonly toggleShield: () => void;
  readonly toggleExplode: () => void;
  readonly setAccent: (accent: Accent) => void;
  readonly setExploded: (exploded: boolean) => void;
  readonly setReducedMotion: (reduced: boolean) => void;
  readonly setMonitorWindow: (window: MonitorWindow) => void;
  readonly setAdvancedOpen: (open: boolean) => void;
  readonly setWorkbenchTab: (tab: WorkbenchTab) => void;
  readonly selectSurfaceCache: (mode: GuiSurfaceCacheMode) => void;
  readonly toggleVectorOnly: () => void;
  readonly setAutoscan: (value: boolean) => void;
  readonly setGain: (value: number) => void;
  /** Whether a pointer holds the gain slider, which defers its log entry. */
  readonly holdGain: (held: boolean) => void;
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

/** Smoothstep keys of one linear track from 0 to `to` over the layer
 * duration: the planes ease out of the panel and settle. */
function easedTrack(
  property: { readonly component: number; readonly offsets: readonly number[] },
  to: number,
): AnimationClipSource["tracks"][number] {
  const steps = 8;
  return {
    property,
    keys: Array.from({ length: steps + 1 }, (_, step) => {
      const t = step / steps;
      return {
        time: t * LAYER_SECONDS,
        value: { kind: "f32" as const, value: to * t * t * (3 - 2 * t) },
        ...(step < steps ? { interpolation: { kind: "linear" as const } } : {}),
      };
    }),
  };
}

async function createMotionAssets(
  client: AnimationWorldClient,
): Promise<MotionAssets> {
  const canvasStyle = client.components.CanvasStyle;
  const offset = canvasStyle?.fields.x?.offset;
  const material = client.components.CustomMaterial?.id;
  const surface = client.components.Surface;
  const spacingOffset = surface?.fields.layer_spacing?.offset;
  const transform = client.components.Transform;
  const paint = client.components.CanvasPaint?.id;
  if (canvasStyle === undefined || offset === undefined)
    throw new Error("The gallery GUI profile does not expose CanvasStyle");
  const translation = { component: canvasStyle.id, offset };
  if (material === undefined)
    throw new Error("The gallery GUI profile does not expose CustomMaterial");
  if (surface === undefined || spacingOffset === undefined)
    throw new Error("The gallery GUI profile does not expose layer spacing");
  if (transform === undefined)
    throw new Error("The gallery GUI profile does not expose Transform");
  if (paint === undefined)
    throw new Error("The gallery GUI profile does not expose CanvasPaint");
  const created: ClientAssetSource[] = [];
  const create = async (clip: AnimationClipSource) => {
    const bytes = client.encodeAnimationClip(clip);
    created.push(await client.createAsset(10, bytes.slice().buffer));
  };
  try {
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
    // The Surface's spacing: each plane stands its id times the spacing
    // in front of the base plane.
    await create({
      duration: LAYER_SECONDS,
      tracks: [
        easedTrack(
          { component: surface.id, offsets: [spacingOffset] },
          LAYER_SPACING,
        ),
      ],
    });
    await create(sweepClip(paint));
    return {
      scan: created[0]!,
      wavePulse: created[1]!,
      dust: created[2]!,
      spacing: created[3]!,
      sweep: created[4]!,
      paintComponent: paint,
      translation,
      materialComponent: material,
      spacingField: { component: surface.id, offset: spacingOffset },
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
      ownership.assets.scan,
      ownership.assets.wavePulse,
      ownership.assets.dust,
      ownership.assets.spacing,
      ownership.assets.sweep,
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
      "gui-projector-dust",
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

/** GUI control components, whose entities also carry GuiBehavior. */
const CONTROL_COMPONENTS = [
  "GuiButton",
  "GuiCheckbox",
  "GuiSlider",
  "GuiTextInput",
  "GuiScrollView",
  "GuiVirtualList",
] as const;

/**
 * Whether the dashboard's canvas has controls and each is evaluated visible.
 * The overlays' controls, in closed lists, popovers and dialogs, stay hidden
 * until they open and do not count.
 */
function controlsVisible(panel: PanelClient, inspection: Inspection): boolean {
  const controls = new Set(
    CONTROL_COMPONENTS.map((name) => panel.components[name]?.id),
  );
  const behavior = panel.components.GuiBehavior?.id;
  const overlay = panel.components.GuiOverlay?.id;
  const entities = new Map(
    inspection.entities.map((entity) => [entity.id, entity]),
  );
  const canvas = inspection.entities.find(
    ({ metadata }) => metadata.symbolicId === CANVAS_ENTITY,
  )?.id;
  // On the canvas and outside every overlay.
  const resting = (id: bigint) => {
    for (let at: bigint | null | undefined = id; at != null; ) {
      if (at === canvas) return true;
      const entity = entities.get(at);
      if (entity?.components.some(({ component }) => component === overlay))
        return false;
      at = entity?.link.parent;
    }
    return false;
  };
  let found = false;
  for (const entity of inspection.entities) {
    if (!entity.components.some(({ component }) => controls.has(component)))
      continue;
    if (!resting(entity.id)) continue;
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
  const [accent, setAccentState] = useState<Accent>("cyan");
  const [exploded, setExplodedState] = useState(false);
  const [reducedMotion, setReducedMotionState] = useState(false);
  const [monitorWindow, setMonitorWindowState] =
    useState<MonitorWindow>("normal");
  const [advancedOpen, setAdvancedOpen] = useState(true);
  const [workbenchTab, setWorkbenchTab] = useState<WorkbenchTab>("nodes");
  const [autoscan, setAutoscanState] = useState(INITIAL_AUTOSCAN);
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
  // A held gain slider defers its log entry to the release.
  const gainHeld = useRef(false);
  const loggedGain = useRef(INITIAL_CONTROL_VALUES.gain);
  const generation = useRef(0);
  const finishingFrame = useRef(false);
  const motionOwnership = useRef<MotionOwnership | undefined>(undefined);
  const panelRequest = useRef(0);
  // The log as CLEAR found it, which its toast's UNDO restores.
  const clearedEvents = useRef<readonly string[]>([]);
  const controls = useRef<{
    scan: GuiControlHandle | undefined;
    callsign: GuiControlHandle | undefined;
    explode: GuiControlHandle | undefined;
  }>({ scan: undefined, callsign: undefined, explode: undefined });

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
  const tuning = useTuning(record, ACCENT_HSV.cyan);
  const resetTuning = tuning.reset;

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
    setAccentState("cyan");
    setExplodedState(false);
    setReducedMotionState(false);
    setMonitorWindowState("normal");
    setAdvancedOpen(true);
    setWorkbenchTab("nodes");
    setAutoscanState(INITIAL_AUTOSCAN);
    setSurfaceCache("automatic");
    setGainState(INITIAL_GAIN);
    setCallsignState(INITIAL_CALLSIGN);
    setLastCommand("Awaiting command");
    setPulseSequence(0);
    setPulseActive(false);
    setEvents(INITIAL_EVENTS);
    resetTuning(ACCENT_HSV.cyan);
    setEventWindowState({ first: 0, last: 0 });
    setShieldArmed(true);
    sequence.current = INITIAL_EVENTS.length;
    controlValues.current = INITIAL_CONTROL_VALUES;
    gainHeld.current = false;
    loggedGain.current = INITIAL_CONTROL_VALUES.gain;
    clearedEvents.current = [];
    const client = canvas.client as AnimationWorldClient;
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

  const reportFailure = useCallback((failure: unknown) => {
    setError(errorMessage(failure));
  }, []);

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

  // The station paces its operations by the panel World's Host frames.
  const frames = useMemo<HostFrames | undefined>(
    () => (panel ? () => panel.client.waitForFrame() : undefined),
    [panel],
  );
  const station = useStation({
    frames,
    ready,
    gain,
    autoscan,
    callsign,
    record,
    reportFailure,
    stopScan: () => {
      void controls.current.scan
        ?.compareAndSet("checked", true, false)
        .catch(reportFailure);
    },
    focusCallsign: () => {
      void controls.current.callsign
        ?.action({ kind: "focus" })
        .catch(reportFailure);
    },
  });
  const { startPulse, toast } = station;

  const pulse = useCallback(() => {
    setPulseSequence((current) => current + 1);
    setLastCommand("Pulse sent");
    record("PULSE BURST COMMITTED");
    startPulse();
  }, [record, startPulse]);
  // CLEAR leaves one entry. The runtime clamps the log's scroll position to
  // the remaining content, and later growth never returns to the discarded
  // position.
  const clearLog = useCallback(() => {
    sequence.current += 1;
    const marker = eventEntry(sequence.current, "LOG CLEARED");
    setEvents((current) => {
      clearedEvents.current = current;
      return [marker];
    });
    setLastCommand("Log cleared");
    toast({
      severity: "information",
      text: "Log cleared.",
      action: {
        label: "UNDO",
        onPress: () => {
          const restored = clearedEvents.current;
          clearedEvents.current = [];
          sequence.current += 1;
          const entry = eventEntry(sequence.current, "LOG RESTORED");
          // Entries since the clear stay newest; the marker gives way to
          // the restored history.
          setEvents((current) =>
            [
              entry,
              ...current.filter((item) => item !== marker),
              ...restored,
            ].slice(0, MAX_EVENTS),
          );
          setLastCommand("Log restored");
        },
      },
    });
  }, [toast]);
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
  const toggleVectorOnly = useCallback(() => {
    // Isolation unmounts the shield with the projector; its next mount is a
    // new entity, resolved again after that commit.
    shieldRequest.current += 1;
    setShieldBlocker(undefined);
    setVectorOnly((current) => !current);
  }, []);
  // Choosing an accent also sets the projection colour to the accent's,
  // which the COLOUR tab's picker, when it is declared, writes to its control.
  const setColorTuning = tuning.setColor;
  const setAccent = useCallback(
    (next: Accent) => {
      setAccentState(next);
      setLastCommand(`Accent ${next}`);
      record(`ACCENT ${next.toUpperCase()}`);
      setColorTuning(ACCENT_HSV[next]);
    },
    [record, setColorTuning],
  );
  const setExploded = useCallback(
    (value: boolean) => {
      setExplodedState(value);
      if (changedValue("exploded", value))
        record(value ? "LAYERS EXPLODED" : "LAYERS FLATTENED");
    },
    [changedValue, record],
  );
  // The sidebar writes the EXPLODE LAYERS switch as the operator would; its
  // value callback then explodes the panel, so the switch stays the one
  // source of the setting.
  const toggleExplode = useCallback(() => {
    const handle = controls.current.explode;
    if (!handle) return;
    void handle
      .compareAndSet("checked", exploded, !exploded)
      .catch(reportFailure);
  }, [exploded, reportFailure]);
  const setReducedMotion = useCallback(
    (value: boolean) => {
      setReducedMotionState(value);
      if (changedValue("reducedMotion", value))
        record(value ? "REDUCED MOTION ON" : "REDUCED MOTION OFF");
    },
    [changedValue, record],
  );
  const setMonitorWindow = useCallback(
    (next: MonitorWindow) => {
      setMonitorWindowState(next);
      record(`MONITOR ${next === "normal" ? "RESTORED" : next.toUpperCase()}`);
      if (next === "closed")
        toast({
          severity: "information",
          text: "Monitor closed.",
          action: {
            label: "REOPEN",
            onPress: () => {
              setMonitorWindowState("normal");
              record("MONITOR REOPENED");
            },
          },
        });
    },
    [record, toast],
  );
  const setAutoscan = useCallback(
    (value: boolean) => {
      setAutoscanState(value);
      if (changedValue("autoscan", value))
        record(`AUTOSCAN ${value ? "ENABLED" : "STANDBY"}`);
    },
    [changedValue, record],
  );
  // A drag commits the gain on every frame; the log records where an
  // adjustment ends: at the release of a drag, or at once from the keyboard
  // or a machine client.
  const logGain = useCallback(
    (value: number) => {
      if (value === loggedGain.current) return;
      loggedGain.current = value;
      record(`SIGNAL GAIN ${Math.round(value * 100)} PERCENT`);
    },
    [record],
  );
  const setGain = useCallback(
    (value: number) => {
      setGainState(value);
      if (changedValue("gain", value) && !gainHeld.current) logGain(value);
    },
    [changedValue, logGain],
  );
  const holdGain = useCallback(
    (held: boolean) => {
      gainHeld.current = held;
      if (!held) logGain(controlValues.current.gain);
    },
    [logGain],
  );
  const setCallsign = useCallback(
    (value: string) => {
      setCallsignState(value);
      if (changedValue("callsign", value))
        record(`CALLSIGN ${value || "CLEARED"}`);
    },
    [changedValue, record],
  );
  const scanControl = useCallback((handle: GuiControlHandle | null) => {
    controls.current.scan = handle ?? undefined;
  }, []);
  const callsignControl = useCallback((handle: GuiControlHandle | null) => {
    controls.current.callsign = handle ?? undefined;
  }, []);
  const explodeControl = useCallback((handle: GuiControlHandle | null) => {
    controls.current.explode = handle ?? undefined;
  }, []);

  return {
    ready,
    vectorOnly,
    prepared,
    revealed,
    ...(error ? { error } : {}),
    accent,
    exploded,
    reducedMotion,
    monitorWindow,
    advancedOpen,
    workbenchTab,
    autoscan,
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
    station,
    tuning,
    scanControl,
    callsignControl,
    explodeControl,
    onCommit,
    attachPanel,
    pulse,
    clearLog,
    toggleShield,
    toggleExplode,
    setAccent,
    setExploded,
    setReducedMotion,
    setMonitorWindow,
    setAdvancedOpen,
    setWorkbenchTab,
    selectSurfaceCache: setSurfaceCache,
    toggleVectorOnly,
    setAutoscan,
    setGain,
    holdGain,
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
        }}
      >
        <VertexShader requiredAttributes={2}>{BEAM_VERTEX_SHADER}</VertexShader>
        <FragmentShader requiredAttributes={2}>{BEAM_SHADER}</FragmentShader>
      </ShaderAsset>
      <ShaderAsset
        id="gui-projector-dust-shader"
        recipe={{ lighting: true }}
        parameters={{
          accent: "vec4",
          energy: "f32",
          section: "vec4",
          depth: "vec2",
          phase: "f32",
        }}
      >
        <VertexShader>{DUST_VERTEX_SHADER}</VertexShader>
        <FragmentShader>{DUST_SHADER}</FragmentShader>
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
 * a separate panel World. The parent owns the Surface size, placement, cache
 * policy and layer spacing; the panel World owns the canvas density and
 * content. The exploded view is an ordinary animation of the Surface's layer
 * spacing on the Host clock, played forward to separate the planes and
 * backward to close them.
 */
function ProjectorPanel({
  scene,
  stagingX,
}: {
  scene: GuiSceneState;
  stagingX: number;
}) {
  const { x: panelX, ...panelTransform } = PROJECTED_PANEL_TRANSFORM;
  const motions = scene.motions!;
  const layers = useRef<AnimationHandle>(null);
  const { exploded, reducedMotion, reportFailure } = scene;
  const started = useRef(false);
  useEffect(() => {
    const handle = layers.current;
    if (!handle) return;
    if (!started.current && !exploded) return;
    started.current = true;
    const action = reducedMotion
      ? handle.seek(exploded ? LAYER_SECONDS : 0)
      : handle.playAtSpeed(exploded ? 1 : -1);
    void action.catch(reportFailure);
  }, [exploded, reducedMotion, reportFailure]);
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
        <Animation
          ref={layers}
          source={motions.spacing.source}
          bindings={[
            {
              track: 0,
              property: {
                component: motions.spacingField.component,
                offsets: [motions.spacingField.offset],
              },
            },
          ]}
          autoPlay={false}
        />
      </Entity>
      <CanvasWorld
        presentation={{ anchor: PANEL_ENTITY }}
        create={{ symbolicId: PANEL_WORLD, selectedSystems: PANEL_SYSTEMS }}
        extent={[CANVAS_WIDTH, CANVAS_HEIGHT]}
        unitsPerMetre={UNITS_PER_METRE}
        onReady={scene.attachPanel}
        onError={scene.reportFailure}
      >
        <ProjectorDashboard scene={scene} />
        <WaveformAnimations scene={scene} />
      </CanvasWorld>
    </>
  );
}
