import type { IppCanvasHandle } from "@ipp/react/web";
import { createRef } from "react";
import {
  Animation,
  AnimationAsset,
  Entity,
  LookAt,
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
} from "@ipp/client";
import type { BlenderDiskManifest } from "../../../../integrations/blender/client/disk-import.js";
import { PLATFORMER_ASSETS, PLATFORMER_ROUTE } from "./scene-file.js";

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
const ORB_DURATION = 12;
const TURN_DISTANCE = 0.24;

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

/** Host clocks own route travel, gait, local facing and the ambient orb. */
export class PlatformerSession {
  private readonly routeRef = createRef<AnimationHandle>();
  private readonly gaitRef = createRef<AnimationHandle>();
  private readonly facingRef = createRef<AnimationHandle>();
  private readonly orbRef = createRef<AnimationHandle>();
  private closed = false;
  private queue: Promise<void> = Promise.resolve();
  private modeRequest = 0;
  private modeAbort: AbortController | undefined;
  private readonly modeOperations = new Set<Promise<void>>();
  private routeController: bigint | undefined;
  private gaitController: bigint | undefined;
  private facingController: bigint | undefined;
  private orbController: bigint | undefined;
  private readonly preparationControllers = new Set<bigint>();
  private state: PlatformerPlayback = {
    mode: "walk",
    direction: 1,
    playing: true,
  };

  private constructor(
    private readonly root: ReactWorldRoot,
    private readonly client: AnimationWorldClient,
    private readonly cameraTarget: bigint,
    private readonly routeTarget: bigint,
    private readonly orbTarget: bigint,
    private readonly route: RouteManifest,
    private readonly routeAsset: AnimationClipSource,
    private readonly gaitDrivers: Record<
      PlatformerMode,
      AnimationDriverDescription[]
    >,
    private readonly orbAsset: AnimationClipSource,
    private readonly changed: (state: PlatformerPlayback) => void,
  ) {}

  static async create(
    canvas: IppCanvasHandle,
    signal: AbortSignal,
    changed: (state: PlatformerPlayback) => void,
  ) {
    const client = canvas.client as AnimationWorldClient;
    const [manifestResponse, routeResponse] = await Promise.all([
      fetch(PLATFORMER_ASSETS + "manifest.json", { signal }),
      fetch(PLATFORMER_ROUTE, { signal }),
    ]);
    if (!manifestResponse.ok)
      throw new Error(`Platformer manifest: HTTP ${manifestResponse.status}`);
    if (!routeResponse.ok)
      throw new Error(`Platformer route: HTTP ${routeResponse.status}`);
    const manifest: BlenderDiskManifest = await manifestResponse.json();
    const route: RouteManifest = await routeResponse.json();
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
    const orbEntity = find("platformer-orb");
    find("platformer-character");
    find("platformer-camera");
    const cameraTarget = find("platformer-camera-target");
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
    const orbMaterial = client.components.CustomMaterial;
    if (!orbMaterial)
      throw new Error("Platformer requires custom material support");
    const orbAsset: AnimationClipSource = {
      duration: ORB_DURATION,
      tracks: [
        {
          property: { component: orbMaterial.id, name: "time" },
          keys: [0, ORB_DURATION].map((time) => ({
            time,
            value: {
              kind: "dynamic",
              value: { kind: "f32", value: time },
            },
          })),
        },
      ],
    };
    const routeAsset = routeClip(client, route);
    const session = new PlatformerSession(
      canvas.createRoot(),
      client,
      cameraTarget,
      rootEntity,
      orbEntity,
      route,
      routeAsset,
      {
        walk: drivers(gaitClips.walk),
        run: drivers(gaitClips.run),
        crawl: drivers(gaitClips.crawl),
      },
      orbAsset,
      changed,
    );
    try {
      await session.prepareGaits(["walk"], signal);
      await session.render();
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
        const orbSource = session.root.getAsset("platformer-orb-time")?.current
          ?.source;
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
        session.orbController = state.controllers?.find((controller) =>
          controller.description.drivers.some(
            (driver) =>
              driver.source === orbSource && driver.target === orbEntity,
          ),
        )?.id;
        if (
          session.routeController !== undefined &&
          session.gaitController !== undefined &&
          session.facingController !== undefined &&
          session.orbController !== undefined &&
          state.resources.every((resource) => resource.status === "loaded")
        )
          break;
        if (performance.now() > deadline)
          throw new Error("Timed out loading platformer scene resources");
        await client.waitForFrame(state.tick);
      }
      await canvas.capture();
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
        <AnimationAsset
          id="platformer-facing-forward"
          clip={this.facingClip(0)}
        />
        <AnimationAsset
          id="platformer-facing-reverse"
          clip={this.facingClip(Math.PI)}
        />
        <AnimationAsset id="platformer-orb-time" clip={this.orbAsset} />
        <Entity bindTo="platformer-camera">
          <LookAt target={this.cameraTarget} />
        </Entity>
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
          speed={1}
          looping
          autoPlay={false}
          transition={{
            duration: TRANSITION_SECONDS,
            easing: "smoothstep",
            startTime: { policy: "preserve" },
          }}
        />
        <Animation
          ref={this.orbRef}
          bindings={this.orbAsset.tracks.map((track, index) => ({
            source: assetRef("platformer-orb-time"),
            track: index,
            target: this.orbTarget,
            property: track.property ?? { joints: track.joints! },
          }))}
          looping
          autoPlay={false}
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
          drivers: this.gaitDrivers[mode],
          speed: 1,
          looping: true,
        });
        controllers.push(controller);
        this.preparationControllers.add(controller);
      }
      const sources = new Set(
        modes.flatMap((mode) =>
          this.gaitDrivers[mode].map((driver) => String(driver.source)),
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

  private facingClip(heading: number): AnimationClipSource {
    const property = this.client.components.Transform;
    if (!property) throw new Error("Platformer requires Transform support");
    return {
      duration: 1,
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
          keys: [{ time: 0, value: rotationValue(heading) }],
        },
      ],
    };
  }

  private async applyPlayback() {
    if (!this.state.playing) {
      await Promise.all([
        this.routeRef.current?.pause(),
        this.gaitRef.current?.pause(),
        this.facingRef.current?.pause(),
        this.orbRef.current?.pause(),
      ]);
      return;
    }
    await this.routeRef.current?.playAtSpeed(this.routeSpeed());
    await this.gaitRef.current?.playAtSpeed(1);
    await this.facingRef.current?.playAtSpeed(1);
    await this.orbRef.current?.play();
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
        this.state = { ...this.state, mode };
        await this.render();
        await this.applyPlayback();
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
        this.gaitRef.current?.seek(0),
        this.facingRef.current?.seek(0),
        this.orbRef.current?.seek(0),
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
    await this.root.unmount();
  }
}
