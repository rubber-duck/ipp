import type { IppCanvasHandle } from "@ipp/react/canvas";
import { createRef } from "react";
import {
  Animation,
  AnimationAsset,
  assetRef,
  type AnimationHandle,
  type ReactWorldRoot,
} from "@ipp/react";
import type {
  AnimationClipSource,
  AnimationDriverDescription,
  AnimationKeyframe,
  AnimationTrack,
  AnimationWorldClient,
  OutputReference,
} from "@ipp/client";
import type { BlenderDiskManifest } from "../../../../integrations/blender/client/disk-import.js";
import type { GalleryAssets } from "../../shared/scene.js";
import { PLATFORMER_ASSETS, PLATFORMER_ROUTE } from "./scene-file.js";
import {
  POSE_ORIGIN_HOLD_TIME,
  poseOriginClip,
  type PlatformerPoseOrigins,
} from "./pose-origins.js";

export type PlatformerMode = "walk" | "run" | "crawl";

interface RoutePoint {
  position: readonly [number, number, number];
  headingRadians: number;
}

interface RouteManifest {
  version: 1;
  coordinateSystem: "ipp-runtime-y-up";
  closed: true;
  rootEntityId: "platformer-root";
  meshForward: readonly [number, number, number];
  waypoints: readonly RoutePoint[];
  modes: Record<PlatformerMode, { clip: string; speed: number }>;
}

export interface PlatformerPlayback {
  mode: PlatformerMode;
  direction: 1 | -1;
  playing: boolean;
}

const TRANSITION_SECONDS = 0.32;
const TURN_DISTANCE = 0.24;
/** Facing clips hold their heading from this time; the facing clock stays there. */
const FACING_HOLD_TIME = 1;

function rotationValue(heading: number) {
  return {
    kind: "rotation" as const,
    value: [0, Math.sin(heading / 2), 0, Math.cos(heading / 2)] as const,
  };
}

function routeClip(
  client: AnimationWorldClient,
  route: RouteManifest,
): AnimationClipSource {
  if (
    route.version !== 1 ||
    route.coordinateSystem !== "ipp-runtime-y-up" ||
    route.closed !== true ||
    route.rootEntityId !== "platformer-root" ||
    route.waypoints.length < 2
  )
    throw new Error("Platformer route metadata is invalid");
  const walkSpeed = route.modes.walk.speed;
  if (!Number.isFinite(walkSpeed) || walkSpeed <= 0)
    throw new Error("Platformer walk speed must be positive");
  const points = [...route.waypoints, route.waypoints[0]!];
  const times = [0];
  const distances: number[] = [];
  for (let index = 1; index < points.length; index++) {
    const before = points[index - 1]!.position;
    const after = points[index]!.position;
    const distance = Math.hypot(
      after[0] - before[0],
      after[1] - before[1],
      after[2] - before[2],
    );
    if (!Number.isFinite(distance) || distance <= 0)
      throw new Error("Platformer route segments must have positive length");
    distances.push(distance);
    times.push(times.at(-1)! + distance / walkSpeed);
  }
  const rotations: AnimationKeyframe[] = [];
  for (let index = 0; index < route.waypoints.length; index++) {
    const incoming =
      route.waypoints[
        (index + route.waypoints.length - 1) % route.waypoints.length
      ]!.headingRadians;
    const outgoing = route.waypoints[index]!.headingRadians;
    const beforeDistance = Math.min(
      TURN_DISTANCE,
      distances[(index + distances.length - 1) % distances.length]! * 0.25,
    );
    const afterDistance = Math.min(TURN_DISTANCE, distances[index]! * 0.25);
    if (index === 0) {
      rotations.push({ time: 0, value: rotationValue(outgoing) });
      rotations.push(
        {
          time: times.at(-1)! - beforeDistance / walkSpeed,
          value: rotationValue(incoming),
        },
        { time: times.at(-1)!, value: rotationValue(outgoing) },
      );
    } else {
      rotations.push(
        {
          time: times[index]! - beforeDistance / walkSpeed,
          value: rotationValue(incoming),
        },
        {
          time: times[index]! + afterDistance / walkSpeed,
          value: rotationValue(outgoing),
        },
      );
    }
  }
  rotations.sort((a, b) => a.time - b.time);
  const transform = client.components.Transform;
  if (!transform) throw new Error("Platformer requires Transform support");
  const numeric = (field: "x" | "y" | "z", axis: number): AnimationTrack => ({
    property: {
      component: transform.id,
      offsets: [transform.fields[field]!.offset],
    },
    keys: points.map((point, index) => ({
      time: times[index]!,
      value: { kind: "f32", value: point.position[axis]! },
    })),
  });
  return {
    duration: times.at(-1)!,
    tracks: [
      numeric("x", 0),
      numeric("y", 1),
      numeric("z", 2),
      {
        property: {
          component: transform.id,
          offsets: [
            transform.fields.qx!.offset,
            transform.fields.qy!.offset,
            transform.fields.qz!.offset,
            transform.fields.qw!.offset,
          ],
        },
        keys: rotations,
      },
    ],
  };
}

/** Host clocks own route travel, gait and local facing. */
export class PlatformerSession {
  private readonly routeRef = createRef<AnimationHandle>();
  private readonly gaitRef = createRef<AnimationHandle>();
  private readonly originRef = createRef<AnimationHandle>();
  private readonly facingRef = createRef<AnimationHandle>();
  private closed = false;
  private queue: Promise<void> = Promise.resolve();
  private modeRequest = 0;
  private modeAbort: AbortController | undefined;
  private readonly modeOperations = new Set<Promise<void>>();
  private routeController: bigint | undefined;
  private gaitController: bigint | undefined;
  private originController: bigint | undefined;
  private facingController: bigint | undefined;
  private readonly preparationControllers = new Set<bigint>();
  private state: PlatformerPlayback = {
    mode: "walk",
    direction: 1,
    playing: true,
  };

  private constructor(
    private readonly root: ReactWorldRoot,
    private readonly client: AnimationWorldClient,
    private readonly routeTarget: bigint,
    private readonly route: RouteManifest,
    private readonly routeAsset: AnimationClipSource,
    private readonly poseOrigins: PlatformerPoseOrigins,
    private readonly gaitDrivers: Record<
      PlatformerMode,
      AnimationDriverDescription[]
    >,
    private readonly changed: (state: PlatformerPlayback) => void,
  ) {}

  static async create(
    canvas: IppCanvasHandle,
    assets: GalleryAssets,
    output: OutputReference,
    signal: AbortSignal,
    changed: (state: PlatformerPlayback) => void,
  ) {
    const client = canvas.client as AnimationWorldClient;
    const [manifest, route, poseOrigins] = await Promise.all([
      assets.readJson<BlenderDiskManifest>(
        PLATFORMER_ASSETS + "manifest.json",
        signal,
      ),
      assets.readJson<RouteManifest>(PLATFORMER_ROUTE, signal),
      assets.readJson<PlatformerPoseOrigins>(
        PLATFORMER_ASSETS + "pose-origins.json",
        signal,
      ),
    ]);
    if (manifest.format !== 1)
      throw new Error("Unsupported platformer asset manifest");
    const inspection = await client.inspect();
    signal.throwIfAborted();
    const find = (symbolicId: string) => {
      const entity = inspection.entities.find(
        (value) => value.metadata.symbolicId === symbolicId,
      );
      if (!entity) throw new Error(`Saved entity ${symbolicId} is missing`);
      return entity.id;
    };
    const rootEntity = find("platformer-root");
    const rigEntity = find("platformer-rig");
    find("platformer-character");
    find("platformer-camera");
    find("platformer-camera-target");
    find("platformer-overhead-light");
    const exported = (mode: PlatformerMode) => {
      const name = route.modes[mode]?.clip;
      const matches = manifest.clips.filter(
        (entry) => entry.name === name && entry.target === "platformer-rig",
      );
      if (matches.length !== 1)
        throw new Error(
          `Expected one exported ${name} action on platformer-rig`,
        );
      return matches[0]!.clip;
    };
    const gaitClips = {
      walk: exported("walk"),
      run: exported("run"),
      crawl: exported("crawl"),
    };
    const drivers = (
      clip: ReturnType<typeof exported>,
    ): AnimationDriverDescription[] =>
      clip.properties.map((property, track) => ({
        source: clip.source,
        track,
        target: rigEntity,
        property,
        repeat: true,
      }));
    const routeAsset = routeClip(client, route);
    const session = new PlatformerSession(
      canvas.createRoot(),
      client,
      rootEntity,
      route,
      routeAsset,
      poseOrigins,
      {
        walk: drivers(gaitClips.walk),
        run: drivers(gaitClips.run),
        crawl: drivers(gaitClips.crawl),
      },
      changed,
    );
    try {
      await session.render();
      await session.prepareGaits(["walk"], signal);
      await session.originRef.current?.seek(POSE_ORIGIN_HOLD_TIME);
      const deadline = performance.now() + 60_000;
      for (;;) {
        signal.throwIfAborted();
        await canvas.flush();
        const state = await client.inspect();
        const failed = state.resources.find(
          (resource) => resource.status === "failed",
        );
        if (failed)
          throw new Error(
            `Platformer resource ${failed.source}: ${failed.error ?? "Loading failed"}`,
          );
        const routeSource =
          session.root.getAsset("platformer-route")?.current?.source;
        session.routeController = state.controllers?.find((controller) =>
          controller.description.drivers.some(
            (driver) =>
              driver.source === routeSource && driver.target === rootEntity,
          ),
        )?.id;
        session.gaitController = state.controllers?.find((controller) =>
          controller.description.drivers.some((driver) =>
            session.gaitDrivers.walk.some(
              (candidate) =>
                candidate.source === driver.source &&
                driver.target === rigEntity,
            ),
          ),
        )?.id;
        session.originController = state.controllers?.find((controller) =>
          controller.description.drivers.some(
            (driver) =>
              driver.source ===
              session.originDrivers(session.state.mode)[0]!.source,
          ),
        )?.id;
        session.facingController = state.controllers?.find((controller) =>
          controller.description.drivers.some(
            (driver) =>
              driver.target === rigEntity &&
              "component" in driver.property &&
              driver.property.component === client.components.Transform?.id &&
              driver.property.offsets !== undefined &&
              driver.property.offsets.includes(
                client.components.Transform.fields.qy!.offset,
              ),
          ),
        )?.id;
        if (
          session.routeController !== undefined &&
          session.gaitController !== undefined &&
          session.originController !== undefined &&
          session.facingController !== undefined &&
          state.resources.every((resource) => resource.status === "loaded")
        )
          break;
        if (performance.now() > deadline)
          throw new Error("Timed out loading platformer scene resources");
        await client.waitForFrame(state.tick);
      }
      // The first completed frame of the loaded scene, without readback.
      await canvas.frame({ afterOutputs: [output] });
      signal.throwIfAborted();
      await session.applyPlayback();
      await session.releasePreparations();
      changed({ ...session.state });
      return session;
    } catch (failure) {
      await session.close();
      throw failure;
    }
  }

  private render() {
    return this.root.render(
      <>
        <AnimationAsset id="platformer-route" clip={this.routeAsset} />
        {(["walk", "run", "crawl"] as const).map((mode) => (
          <AnimationAsset
            key={mode}
            id={"platformer-pose-origin-" + mode}
            clip={poseOriginClip(this.poseOrigins, mode)}
          />
        ))}
        <AnimationAsset
          id="platformer-facing-forward"
          clip={this.facingClip(0)}
        />
        <AnimationAsset
          id="platformer-facing-reverse"
          clip={this.facingClip(Math.PI)}
        />
        <Animation
          ref={this.routeRef}
          bindings={this.routeAsset.tracks.map((track, index) => ({
            source: assetRef("platformer-route"),
            track: index,
            target: this.routeTarget,
            property: track.property ?? { joints: track.joints! },
          }))}
          speed={this.routeSpeed()}
          looping
          autoPlay={false}
        />
        <Animation
          ref={this.originRef}
          source={assetRef("platformer-pose-origin-" + this.state.mode)}
          target={this.gaitDrivers.walk[0]!.target}
          speed={0}
          autoPlay={false}
          transition={{
            duration: TRANSITION_SECONDS,
            easing: "smoothstep",
            startTime: { policy: "seek", time: POSE_ORIGIN_HOLD_TIME },
          }}
        />
        <Animation
          ref={this.gaitRef}
          bindings={this.gaitDrivers[this.state.mode]}
          speed={1}
          looping
          autoPlay={false}
          transition={{
            duration: TRANSITION_SECONDS,
            easing: "smoothstep",
            startTime: { policy: "matchPhase" },
          }}
        />
        <Animation
          ref={this.facingRef}
          source={assetRef(
            this.state.direction === 1
              ? "platformer-facing-forward"
              : "platformer-facing-reverse",
          )}
          target={this.gaitDrivers.walk[0]!.target}
          speed={0}
          looping
          autoPlay={false}
          transition={{
            duration: TRANSITION_SECONDS,
            easing: "smoothstep",
            startTime: { policy: "seek", time: FACING_HOLD_TIME },
          }}
        />
      </>,
    );
  }

  private enqueue(operation: () => Promise<void>) {
    const next = this.queue.then(operation);
    this.queue = next.catch(() => {});
    return next;
  }

  private routeSpeed() {
    return (
      (this.route.modes[this.state.mode].speed / this.route.modes.walk.speed) *
      this.state.direction
    );
  }

  private async prepareGaits(
    modes: readonly PlatformerMode[],
    signal?: AbortSignal,
  ) {
    const controllers: bigint[] = [];
    try {
      for (const mode of modes) {
        signal?.throwIfAborted();
        const controller = await this.client.createAnimationController({
          drivers: [...this.originDrivers(mode), ...this.gaitDrivers[mode]],
          speed: 1,
          looping: true,
        });
        controllers.push(controller);
        this.preparationControllers.add(controller);
      }
      const sources = new Set(
        modes.flatMap((mode) =>
          [...this.originDrivers(mode), ...this.gaitDrivers[mode]].map(
            (driver) => String(driver.source),
          ),
        ),
      );
      const deadline = performance.now() + 60_000;
      for (;;) {
        signal?.throwIfAborted();
        const state = await this.client.inspect();
        const resources = state.resources.filter((resource) =>
          sources.has(resource.source),
        );
        const failed = resources.find(
          (resource) => resource.status === "failed",
        );
        if (failed)
          throw new Error(
            `Platformer resource ${failed.source}: ${failed.error ?? "Loading failed"}`,
          );
        if (
          resources.length === sources.size &&
          resources.every((resource) => resource.status === "loaded")
        )
          return controllers;
        if (performance.now() > deadline)
          throw new Error("Timed out preparing platformer gait resources");
        await this.client.waitForFrame(state.tick);
      }
    } catch (failure) {
      await this.releasePreparations(controllers);
      throw failure;
    }
  }

  private async releasePreparations(
    controllers = [...this.preparationControllers],
  ) {
    controllers.forEach((controller) =>
      this.preparationControllers.delete(controller),
    );
    await Promise.all(
      controllers.map((controller) =>
        this.client.deleteAnimationController(controller),
      ),
    );
  }

  /**
   * A controller adds its clip's change from time 0, so facing is a clip that
   * turns from the authored heading to `heading` and holds it; the facing
   * controller stays at the hold time and a transition blends the turn.
   */
  private facingClip(heading: number): AnimationClipSource {
    const property = this.client.components.Transform;
    if (!property) throw new Error("Platformer requires Transform support");
    return {
      duration: 2 * FACING_HOLD_TIME,
      tracks: [
        {
          property: {
            component: property.id,
            offsets: [
              property.fields.qx!.offset,
              property.fields.qy!.offset,
              property.fields.qz!.offset,
              property.fields.qw!.offset,
            ],
          },
          keys: [
            { time: 0, value: rotationValue(0) },
            { time: FACING_HOLD_TIME, value: rotationValue(heading) },
            { time: 2 * FACING_HOLD_TIME, value: rotationValue(heading) },
          ],
        },
      ],
    };
  }

  private originDrivers(mode: PlatformerMode): AnimationDriverDescription[] {
    const resource = this.root.getAsset(
      "platformer-pose-origin-" + mode,
    )?.current;
    if (!resource) throw new Error("Platformer pose origin is not prepared");
    return [
      {
        source: resource.source,
        track: 0,
        target: this.gaitDrivers.walk[0]!.target,
        property: {
          joints: this.poseOrigins.baseline.map((_, ordinal) => ordinal),
        },
      },
    ];
  }

  private async applyPlayback() {
    const controllers = [
      [this.originController, 0],
      [this.gaitController, 1],
      [this.routeController, this.routeSpeed()],
      [this.facingController, 0],
    ] as const;
    // Submit without acknowledgement gaps. The protocol orders these inputs;
    // separate controllers can still begin on adjacent Host ticks.
    for (const [controller, speed] of controllers) {
      if (controller === undefined) continue;
      this.client.playback(
        controller,
        this.state.playing
          ? { action: "playAtSpeed", speed }
          : { action: "pause" },
      );
    }
    await this.client.inspect();
  }

  setMode(mode: PlatformerMode) {
    if (this.closed) return Promise.resolve();
    const request = ++this.modeRequest;
    this.modeAbort?.abort();
    if (mode === this.state.mode) return Promise.resolve();
    const abort = new AbortController();
    this.modeAbort = abort;
    const operation = this.changeMode(mode, request, abort);
    this.modeOperations.add(operation);
    void operation.then(
      () => this.modeOperations.delete(operation),
      () => this.modeOperations.delete(operation),
    );
    return operation;
  }

  private async changeMode(
    mode: PlatformerMode,
    request: number,
    abort: AbortController,
  ) {
    let preparations: bigint[] = [];
    try {
      preparations = await this.prepareGaits([mode], abort.signal);
      abort.signal.throwIfAborted();
      await this.enqueue(async () => {
        if (this.closed || request !== this.modeRequest) return;
        // Freeze both fade clocks before React submits their separate retargets.
        // Destination sources are ready, so a cold load cannot strand playback.
        await Promise.all([
          this.originRef.current?.pause(),
          this.gaitRef.current?.pause(),
        ]);
        this.state = { ...this.state, mode };
        try {
          await this.render();
        } finally {
          await this.applyPlayback();
        }
        this.changed({ ...this.state });
      });
    } catch (failure) {
      if (!abort.signal.aborted) throw failure;
    } finally {
      await this.releasePreparations(preparations);
      if (this.modeAbort === abort) this.modeAbort = undefined;
    }
  }

  reverse() {
    return this.enqueue(async () => {
      if (this.closed) return;
      this.state = {
        ...this.state,
        direction: this.state.direction === 1 ? -1 : 1,
      };
      await this.render();
      await this.applyPlayback();
      this.changed({ ...this.state });
    });
  }

  playback(playing: boolean) {
    return this.enqueue(async () => {
      if (this.closed) return;
      this.state = { ...this.state, playing };
      await this.applyPlayback();
      this.changed({ ...this.state });
    });
  }

  reset() {
    return this.enqueue(async () => {
      if (this.closed) return;
      await Promise.all([
        this.routeRef.current?.seek(0),
        this.gaitController === undefined
          ? undefined
          : this.client.transitionAnimationController(this.gaitController, {
              description: {
                drivers: this.gaitDrivers[this.state.mode],
                speed: 1,
                looping: true,
              },
              duration: 0,
              startTime: { policy: "seek", time: 0 },
            }),
        this.originController === undefined
          ? undefined
          : this.client.transitionAnimationController(this.originController, {
              description: {
                drivers: this.originDrivers(this.state.mode),
                speed: 0,
                looping: false,
              },
              duration: 0,
              startTime: { policy: "seek", time: POSE_ORIGIN_HOLD_TIME },
            }),
      ]);
      this.state = { ...this.state, direction: 1 };
      await this.render();
      await this.applyPlayback();
      this.changed({ ...this.state });
    });
  }

  async close() {
    if (this.closed) return;
    this.closed = true;
    this.modeRequest += 1;
    this.modeAbort?.abort();
    await this.queue.catch(() => {});
    await Promise.allSettled(this.modeOperations);
    await this.releasePreparations();
    await this.root.render(null);
    await this.root.unmount();
  }
}
