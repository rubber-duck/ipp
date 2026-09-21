import { clientAssetSource } from "@ipp/client";
import { createRef } from "react";
import {
  Animation,
  AnimationAsset,
  Entity,
  assetRef,
  type AnimationHandle,
  type ReactWorldRoot,
} from "@ipp/react";
import type {
  AnimationWorldClient,
  Command,
  FieldValue,
  AnimationPlaybackControl,
  AnimationControllerSnapshot,
} from "@ipp/client";
import type { IppCanvasHandle } from "@ipp/react/web";
import { ANIMATED_IDS, SCENE_OBJECTS, type ObjectId } from "./model.js";
import {
  beamPickingSource,
  SKELETON_ASSET,
  SKIN_ASSET,
  uploadRigAssets,
  animationClips,
} from "./animation-assets.js";

export interface DemoPlayer extends AnimationControllerSnapshot {
  name: string;
  symbol: ObjectId;
  speed: number;
  looping: boolean;
}

/** Runtime controllers own clocks; React owns target objects and visible properties. */
export class AnimationSession {
  private closed = false;
  private root: ReactWorldRoot;
  private readonly refs = ANIMATED_IDS.map(() => createRef<AnimationHandle>());
  private readonly settings = ANIMATED_IDS.map(() => ({
    speed: 1,
    looping: true,
  }));
  private constructor(
    private readonly client: AnimationWorldClient,
    readonly players: readonly bigint[],
    private readonly targets: readonly bigint[],
    root: ReactWorldRoot,
  ) {
    this.root = root;
  }

  static async create(
    canvas: IppCanvasHandle,
    moduleUrl: string,
    signal: AbortSignal,
  ) {
    const client = canvas.client as AnimationWorldClient;
    await uploadRigAssets(client, moduleUrl);
    signal.throwIfAborted();
    const deadline = performance.now() + 10_000;
    let targets: bigint[];
    for (;;) {
      await canvas.flush();
      signal.throwIfAborted();
      const inspection = await client.inspect();
      const entities = ANIMATED_IDS.map((id) =>
        inspection.entities.find((entity) => entity.metadata.symbolicId === id),
      );
      if (entities.every((entity) => entity !== undefined)) {
        targets = entities.map((entity) => entity!.id);
        break;
      }
      if (performance.now() > deadline)
        throw new Error("Animated objects did not mount");
      await client.waitForFrame(inspection.tick);
    }
    signal.throwIfAborted();
    const operations: Command[] = [];
    const insert = (
      entity: { kind: "alias"; alias: number } | { kind: "handle"; id: bigint },
      name: string,
      fields: Record<string, FieldValue>,
    ) => {
      const component = client.components[name]!;
      operations.push({
        kind: "insertComponent",
        entity,
        component: component.id,
        fields: Object.entries(fields).map(([field, value]) => ({
          offset: component.fields[field]!.offset,
          value,
        })),
      });
    };
    const beam = targets[3]!;
    insert({ kind: "handle", id: beam }, "Skeleton", {
      source: {
        kind: "string",
        value: clientAssetSource(client.session, 3, SKELETON_ASSET).source,
      },
    });
    insert({ kind: "handle", id: beam }, "Skin", {
      skeleton: { kind: "entity", value: { kind: "handle", id: beam } },
      source: {
        kind: "string",
        value: clientAssetSource(client.session, 5, SKIN_ASSET).source,
      },
    });
    const outcome = await client.batch(operations);
    if (!outcome.ok)
      throw new Error(`Animation setup: ${outcome.error.reason}`);
    const controllers: bigint[] = [];
    const session = new AnimationSession(
      client,
      controllers,
      targets,
      canvas.createRoot(),
    );
    try {
      await session.render();
      for (;;) {
        signal.throwIfAborted();
        const inspection = await client.inspect();
        const sources = new Set([
          clientAssetSource(client.session, 3, SKELETON_ASSET).source,
          clientAssetSource(client.session, 5, SKIN_ASSET).source,
          beamPickingSource(client.session),
        ]);
        const resources = inspection.resources.filter((resource) =>
          sources.has(resource.source),
        );
        const failed = resources.find(
          (resource) => resource.status === "failed",
        );
        if (failed) throw new Error(failed.error ?? "Animation asset failed");
        const readyControllers = targets.map((target) =>
          inspection.controllers?.find((controller) =>
            controller.description.drivers.some(
              (driver) => driver.target === target,
            ),
          ),
        );
        if (
          readyControllers.every(Boolean) &&
          resources.length === sources.size &&
          resources.every((resource) => resource.status === "loaded")
        ) {
          controllers.push(
            ...readyControllers.map((controller) => controller!.id),
          );
          break;
        }
        if (performance.now() > deadline)
          throw new Error("Animation assets did not load");
        await client.waitForFrame(inspection.tick);
      }
      signal.throwIfAborted();
      await session.control("all", { action: "play" });
      return session;
    } catch (failure) {
      await session.close();
      throw failure;
    }
  }

  private render() {
    const clips = animationClips(this.client);
    return this.root.render(
      <>
        {ANIMATED_IDS.map((symbol, index) => (
          <Entity key={symbol} bindTo={symbol}>
            <Animation
              source={assetRef(symbol)}
              ref={this.refs[index]!}
              {...this.settings[index]!}
              bindings={clips[index]!.tracks.map((_, track) => ({
                track,
                additive: symbol !== "lighting-skinning",
              }))}
            />
            <AnimationAsset id={symbol} clip={clips[index]!} />
          </Entity>
        ))}
      </>,
    );
  }

  async control(selection: string, control: AnimationPlaybackControl) {
    if (this.closed) return;
    for (const id of this.selected(selection)) {
      const ref = this.refs[this.players.indexOf(id)]!.current;
      if (!ref) continue;
      if (control.action === "seek") await ref.seek(control.time);
      else if (control.action === "playAtSpeed")
        await ref.playAtSpeed(control.speed);
      else await ref[control.action]();
      if (control.action === "stop") await ref.seek(0);
    }
  }

  async hold(symbol: ObjectId) {
    const [entity] = this.selected(symbol);
    if (this.closed || entity === undefined) return () => {};
    const inspection = await this.client.inspect();
    const running =
      inspection.controllers?.find((controller) => controller.id === entity)
        ?.state === "playing";
    if (this.closed) return () => {};
    if (running) await this.control(entity.toString(), { action: "pause" });
    let released = false;
    return () => {
      if (released) return;
      released = true;
      if (!this.closed && running)
        void this.control(entity.toString(), { action: "play" }).catch(
          console.error,
        );
    };
  }

  async configure(
    selection: string,
    field: "speed" | "looping",
    value: number | boolean,
  ) {
    if (this.closed) return;
    for (const id of this.selected(selection)) {
      const setting = this.settings[this.players.indexOf(id)]!;
      if (field === "speed") setting.speed = Number(value);
      else setting.looping = Boolean(value);
    }
    await this.render();
  }

  async observe(): Promise<DemoPlayer[]> {
    const inspection = await this.client.inspect();
    return this.players.map((entity, index) => {
      const snapshot = inspection.controllers?.find(
        (controller) => controller.id === entity,
      );
      if (!snapshot) throw new Error("Animation controller was removed");
      const symbol = ANIMATED_IDS[index]!;
      return {
        ...snapshot,
        symbol,
        name: SCENE_OBJECTS.find((object) => object.id === symbol)!.name,
        speed: snapshot.description.speed ?? 1,
        looping: snapshot.description.looping ?? false,
      };
    });
  }

  async close() {
    if (this.closed) return;
    this.closed = true;
    await this.root.unmount();
  }

  private selected(selection: string) {
    return this.players.filter(
      (entity, index) =>
        selection === "all" ||
        selection === entity.toString() ||
        selection === ANIMATED_IDS[index] ||
        selection === this.targets[index]?.toString(),
    );
  }
}
