import {
  guiPartProperty,
  guiProperty,
  type AnimationClipSource,
  type AnimationControllerState,
  type AnimationTrack,
  type AnimationWorldClient,
  type AssetResourceSnapshot,
  type ClientAssetSource,
  type GuiNodeHandle,
} from "@ipp/client";
import {
  Entity,
  FragmentShader,
  ShaderAsset,
  Surface,
  SurfaceCache,
  Transform,
  VertexShader,
} from "@ipp/react";
import { GuiRoot } from "@ipp/react/gui";
import { World, type IppCanvasHandle } from "@ipp/react/web";
import { useCallback, useEffect, useRef, useState } from "react";
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

import {
  PALETTES,
  ProjectorDashboard,
  SURFACE_WIDTH,
  SURFACE_HEIGHT,
  type Palette,
} from "./dashboard.js";
import {
  BoundWaveformAnimations,
  useWaveformNode,
  waveformClips,
  waveformResourceSources,
  type WaveformMotionAssets,
} from "./waveform.js";

const FONT_URL = "/target/font-assets/shure-tech-mono.ippf";
const INITIAL_GAIN = 0.64;
const INITIAL_CALLSIGN = "VESPER-7";
const INITIAL_AUTOSCAN = true;
const MAX_EVENTS = 14;
const STAGING_X = 1_000;

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
 * per content metre (about 593x385 texels, 0.87 MiB) roughly matches the on-screen
 * density of a 720-pixel-high canvas at that boundary, and each further
 * distance doubling halves density and refresh. 30 Hz keeps the scanning
 * trace smooth at a distance while bounding repaints.
 */
const GUI_SURFACE_CACHE = {
  direct_distance: 20,
  texels_per_metre: 80,
  max_refresh_hz: 30,
} as const;

const INITIAL_EVENTS = [
  "06 // DEEP ARRAY LINK STABLE",
  "05 // ROUTE N7 ACQUIRED",
  "04 // SPECTRUM SWEEP NOMINAL",
  "03 // ARCHIVE CHANNEL SEALED",
  "02 // NAV LATTICE ALIGNED",
  "01 // SUBSYSTEM CLOCKS SYNCED",
] as const;

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
  readonly lastCommand: string;
  readonly events: readonly string[];
  readonly motions?: MotionAssets;
  readonly beamSection?: ProjectorBeamSection;
  readonly font: ClientAssetSource;
  readonly onCommit: () => void;
  readonly pulse: () => void;
  readonly uplink: () => void;
  readonly toggleSpan: () => void;
  readonly selectSkin: (skin: GuiDemoSkin) => void;
  readonly selectSurfaceCache: (mode: GuiSurfaceCacheMode) => void;
  readonly toggleVectorOnly: () => void;
  readonly setAutoscan: (value: boolean) => void;
  readonly setGain: (value: number) => void;
  readonly setCallsign: (value: string) => void;
  readonly readWaveformPulse: () => Promise<AnimationControllerState>;
  readonly prepareWaveform: (
    signal: GuiNodeHandle,
    pulse: GuiNodeHandle,
  ) => Promise<void>;
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

function dynamicTrack(
  component: number,
  lane: "color" | "opacity" | "scale",
  values: readonly (readonly [number, unknown])[],
): AnimationTrack {
  return {
    property: {
      component,
      name: guiPartProperty(1, "background", lane),
    },
    keys: values.map(([time, value], index) => ({
      time,
      value: {
        kind: "dynamic" as const,
        value:
          lane === "color"
            ? {
                kind: "vec4" as const,
                value: value as readonly [number, number, number, number],
              }
            : lane === "scale"
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

/** Complete color/opacity/scale samples for every interaction destination. */
function skinMotion(component: number, palette: Palette): AnimationClipSource {
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
      time: 0.4,
      color: palette.secondary,
      opacity: 1,
      scale: [1, 1] as const,
    },
    {
      time: 0.5,
      color: palette.button,
      opacity: 0.8,
      scale: [1, 1] as const,
    },
  ] as const;
  return {
    duration: 0.5,
    tracks: [
      dynamicTrack(
        component,
        "color",
        samples.map(({ time, color }) => [time, color] as const),
      ),
      dynamicTrack(
        component,
        "opacity",
        samples.map(({ time, opacity }) => [time, opacity] as const),
      ),
      dynamicTrack(
        component,
        "scale",
        samples.map(({ time, scale }) => [time, scale] as const),
      ),
    ],
  };
}

async function createMotionAssets(
  client: AnimationWorldClient,
): Promise<MotionAssets> {
  const component = client.components.GuiRoot?.id;
  const material = client.components.CustomMaterial?.id;
  if (component === undefined)
    throw new Error("The gallery GUI profile does not expose GuiRoot");
  if (material === undefined)
    throw new Error("The gallery GUI profile does not expose CustomMaterial");
  const created: ClientAssetSource[] = [];
  try {
    for (const name of ["aurora", "ember", "neon"] as const) {
      const bytes = client.encodeAnimationClip(
        skinMotion(component, PALETTES[name]),
      );
      created.push(await client.createAsset(10, bytes.slice().buffer));
    }
    for (const clip of waveformClips(component)) {
      const bytes = client.encodeAnimationClip(clip);
      created.push(await client.createAsset(10, bytes.slice().buffer));
    }
    const dust = client.encodeAnimationClip({
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
    created.push(await client.createAsset(10, dust.slice().buffer));
    return {
      aurora: created[0]!,
      ember: created[1]!,
      neon: created[2]!,
      scan: created[3]!,
      wavePulse: created[4]!,
      dust: created[5]!,
      guiComponent: component,
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

async function captureCompleteProjectorFrame(
  canvas: IppCanvasHandle,
  active: () => boolean,
): Promise<void> {
  const client = canvas.client as AnimationWorldClient;
  for (let attempt = 0; attempt < 120; attempt += 1) {
    if (!active()) return;
    const frame = await canvas.capture();
    if (!active()) return;
    const inspection = await client.inspect();
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
    ].map(
      (symbolicId) =>
        inspection.entities
          .find((entity) => entity.metadata.symbolicId === symbolicId)
          ?.effective.find(({ component }) => component === material)?.fields
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
      shaders.every((resource) => resource?.status === "loaded") &&
      Number(frame.backend.failedDrawCalls ?? 0) === 0 &&
      frame.drawCalls > 0
    )
      return;
    await client.waitForFrame(inspection.tick);
  }
  throw new Error("Projector resources did not produce a complete frame");
}

function assetKey(asset: ClientAssetSource): string {
  return `${asset.kind}:${asset.variant ?? 0}:${asset.source}`;
}

/** Subscribe before inspection so readiness cannot race the initial snapshot. */
function observeEssentialResources(
  client: AnimationWorldClient,
  assets: readonly ClientAssetSource[],
  active: () => boolean,
  ready: () => void,
  failed: (message: string) => void,
): () => void {
  const expected = new Set(assets.map(assetKey));
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
  const observe = (resource: AssetResourceSnapshot) => {
    const key = assetKey(resource);
    if (!expected.has(key)) return;
    observed.set(key, resource);
    publish();
  };
  const unsubscribe = client.onResourceChange(observe);
  void client.inspect().then(
    (inspection) => {
      if (!active()) return;
      for (const resource of inspection.resources) {
        const key = assetKey(resource);
        if (expected.has(key) && !observed.has(key))
          observed.set(key, resource);
      }
      publish();
    },
    (failure: unknown) => {
      if (active()) failed(errorMessage(failure));
    },
  );
  return unsubscribe;
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
  const [events, setEvents] = useState<readonly string[]>(INITIAL_EVENTS);
  const sequence = useRef(INITIAL_EVENTS.length);
  const generation = useRef(0);
  const finishingFrame = useRef(false);
  const motionOwnership = useRef<MotionOwnership | undefined>(undefined);

  const releaseMotions = useCallback(async () => {
    const ownership = motionOwnership.current;
    if (!ownership || ownership.released) return;
    await releaseMotionOwnership(ownership);
    if (motionOwnership.current === ownership)
      motionOwnership.current = undefined;
  }, []);

  const record = useCallback((message: string) => {
    sequence.current += 1;
    setEvents((current) =>
      [
        `${String(sequence.current).padStart(2, "0")} // ${message}`,
        ...current,
      ].slice(0, MAX_EVENTS),
    );
  }, []);

  useEffect(() => {
    const request = ++generation.current;
    setReady(false);
    setVectorOnly(false);
    setPrepared(false);
    setRevealed(false);
    setTreeCommitted(false);
    setError(undefined);
    setMotions(undefined);
    setBeamSection(undefined);
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
    setEvents(INITIAL_EVENTS);
    sequence.current = INITIAL_EVENTS.length;
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

  useEffect(() => {
    if (!canvas || !active || !motions || !treeCommitted || error || prepared)
      return;
    const request = generation.current;
    const client = canvas.client as AnimationWorldClient;
    return observeEssentialResources(
      client,
      [
        absoluteAsset(17, FONT_URL),
        ...waveformResourceSources(),
        motions.aurora,
        motions.ember,
        motions.neon,
        motions.scan,
        motions.wavePulse,
        motions.dust,
        ...projectorResourceSources(),
      ],
      () => generation.current === request,
      () => setPrepared(true),
      (message) => setError(message),
    );
  }, [canvas, active, motions, treeCommitted, error, prepared]);

  const prepareWaveform = useCallback(
    async (signal: GuiNodeHandle, pulse: GuiNodeHandle) => {
      if (!canvas || !active)
        throw new Error("The waveform World is not active");
      const client = canvas.client;
      if (signal.session !== client.session || pulse.session !== client.session)
        throw new Error("The waveform nodes belong to a different session");
      const result = await client.batch(
        [signal, pulse].map((node) => ({
          kind: "setDynamicProperty" as const,
          entity: { kind: "handle" as const, id: node.entity },
          component: client.components.GuiRoot!.id,
          name: guiProperty(node.nodeId, "position"),
          value: { kind: "vec2" as const, value: [0, 0] as const },
        })),
      );
      if (!result.ok)
        throw new Error("Could not initialize waveform positions");
    },
    [canvas, active],
  );

  const readWaveformPulse = useCallback(async () => {
    if (!canvas || !active || !motions)
      throw new Error("The waveform World is not active");
    const inspection = await canvas.client.inspect();
    const controller = inspection.controllers?.find(({ description }) =>
      description.drivers.some(
        ({ source }) => source === motions.wavePulse.source,
      ),
    );
    if (!controller)
      throw new Error("The waveform pulse controller is no longer mounted");
    return controller;
  }, [canvas, active, motions]);

  const onCommit = useCallback(() => {
    if (!canvas || !active || error || ready) return;
    if (!revealed) {
      if (!treeCommitted) setTreeCommitted(true);
      else if (prepared) {
        const request = generation.current;
        void canvas.client.inspect().then(
          (inspection) => {
            if (generation.current !== request) return;
            const panel = inspection.entities.find(
              ({ metadata }) => metadata.symbolicId === "gui-demo",
            );
            const waves = inspection.controllers?.filter(({ description }) =>
              description.drivers.some(
                (driver) =>
                  driver.target === panel?.id &&
                  [motions?.scan.source, motions?.wavePulse.source].includes(
                    driver.source,
                  ) &&
                  driver.property.name?.endsWith("_position"),
              ),
            );
            if (waves?.length === 2) setRevealed(true);
          },
          (failure: unknown) => {
            if (generation.current === request) setError(errorMessage(failure));
          },
        );
      }
      return;
    }
    if (finishingFrame.current) return;
    finishingFrame.current = true;
    const request = generation.current;
    void captureCompleteProjectorFrame(
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
    prepared,
    revealed,
    treeCommitted,
    motions,
  ]);

  const pulse = useCallback(() => {
    setPulseSequence((current) => current + 1);
    setLastCommand("Pulse sent");
    record("PULSE BURST COMMITTED");
  }, [record]);
  const uplink = useCallback(() => {
    setLastCommand("Uplink sent");
    record("UPLINK PACKET QUEUED");
  }, [record]);
  const toggleSpan = useCallback(() => {
    setWide(!wide);
    record(`SPAN ${wide ? "NARROW" : "WIDE"}`);
  }, [wide, record]);
  const toggleVectorOnly = useCallback(() => {
    setVectorOnly((current) => !current);
  }, []);
  const selectSkin = useCallback(
    (next: GuiDemoSkin) => {
      setSkin(next);
      record(`${next.toUpperCase()} SKIN ONLINE`);
    },
    [record],
  );
  const setAutoscan = useCallback(
    (value: boolean) => {
      setAutoscanState(value);
      record(`AUTOSCAN ${value ? "ENABLED" : "STANDBY"}`);
    },
    [record],
  );
  const setGain = useCallback(
    (value: number) => {
      setGainState(value);
      record(`SIGNAL GAIN ${Math.round(value * 100)} PERCENT`);
    },
    [record],
  );
  const setCallsign = useCallback(
    (value: string) => {
      setCallsignState(value);
      record(`CALLSIGN ${value || "CLEARED"}`);
    },
    [record],
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
    lastCommand,
    events,
    ...(motions ? { motions } : {}),
    ...(beamSection ? { beamSection } : {}),
    font: absoluteAsset(17, FONT_URL),
    onCommit,
    pulse,
    uplink,
    toggleSpan,
    toggleVectorOnly,
    selectSkin,
    selectSurfaceCache: setSurfaceCache,
    setAutoscan,
    setGain,
    setCallsign,
    reportFailure,
    prepareWaveform,
    readWaveformPulse,
  };
}

export function GuiWorld({ scene }: { scene: GuiSceneState }) {
  if (!scene.motions || !scene.beamSection || scene.error) return null;
  const stagingX = scene.revealed ? 0 : STAGING_X;
  return (
    <World onCommit={scene.onCommit}>
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
      {!scene.vectorOnly && (
        <HolographicProjector scene={scene} stagingX={stagingX} />
      )}
      <ProjectorPanel scene={scene} stagingX={stagingX} />
    </World>
  );
}

function ProjectorPanel({
  scene,
  stagingX,
}: {
  scene: GuiSceneState;
  stagingX: number;
}) {
  const [signal, setSignal] = useWaveformNode();
  const [pulse, setPulse] = useWaveformNode();
  const [pulseActive, setPulseActive] = useState(false);
  const { x: panelX, ...panelTransform } = PROJECTED_PANEL_TRANSFORM;
  return (
    <Entity id="gui-demo">
      <Transform bound={false} {...panelTransform} x={panelX + stagingX} />
      <Surface bound={false} width={SURFACE_WIDTH} height={SURFACE_HEIGHT} />
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
      <GuiRoot>
        <ProjectorDashboard
          scene={scene}
          waveform={{ signal: setSignal, pulse: setPulse, pulseActive }}
        />
      </GuiRoot>
      {signal && pulse && (
        <BoundWaveformAnimations
          scene={scene}
          signal={signal}
          pulse={pulse}
          onPulseActive={setPulseActive}
        />
      )}
    </Entity>
  );
}
