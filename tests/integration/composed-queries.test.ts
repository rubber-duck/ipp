import assert from "node:assert/strict";
import test from "node:test";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import type {
  HostClientBase,
  PickingWorldClient,
  RootBinding,
  SystemQuery,
} from "@ipp/client";
import { webSocketTransport } from "../../packages/ipp-client/src/transport.js";
import { installCameraControls } from "../../examples/world-gallery/shared/camera-controls.js";
import { runNativeEnvironment } from "./environment.js";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
} from "./camera-fixtures.js";
import { CAMERA, selectSystems } from "./system-selections.js";

class PointerSurface extends EventTarget {
  private captured = new Set<number>();

  getBoundingClientRect() {
    return { left: 0, top: 0, width: 400, height: 200 };
  }

  setPointerCapture(pointer: number) {
    this.captured.add(pointer);
  }

  hasPointerCapture(pointer: number) {
    return this.captured.has(pointer);
  }

  releasePointerCapture(pointer: number) {
    this.captured.delete(pointer);
  }

  pointer(type: string, x: number, button = 0, buttons = 1) {
    this.dispatchEvent(
      Object.assign(new Event(type, { cancelable: true }), {
        pointerId: 1,
        isPrimary: true,
        clientX: x,
        clientY: 100,
        button,
        buttons,
      }),
    );
  }
}

function near(actual: number, expected: number) {
  assert.ok(Math.abs(actual - expected) < 1e-5, `${actual} != ${expected}`);
}

async function controllerBindingRevalidation(
  surface: PointerSurface,
  client: PickingWorldClient,
  binding: () => RootBinding,
  rebind: () => Promise<void>,
) {
  const records: {
    clicks: number;
    moves: number;
    started: number;
    stopped: number;
    finished: number;
  }[] = [];
  const failures: unknown[] = [];
  const hold = () => ({
    entered: Promise.withResolvers<void>(),
    release: Promise.withResolvers<void>(),
  });
  let queryHold:
    | (ReturnType<typeof hold> & { type: SystemQuery["type"] })
    | undefined;
  let pickedHold: ReturnType<typeof hold> | undefined;
  let pending = 0;
  let idle = Promise.withResolvers<void>();
  idle.resolve();
  const controlled = new Proxy(client, {
    get(target, key) {
      if (key === "query")
        return async (query: SystemQuery) => {
          const result = await target.query(query);
          if (queryHold?.type === query.type) {
            const held = queryHold;
            queryHold = undefined;
            held.entered.resolve();
            await held.release.promise;
          }
          return result;
        };
      const value: unknown = Reflect.get(target, key, target);
      return typeof value === "function" ? value.bind(target) : value;
    },
  });
  const dispose = installCameraControls(
    surface as unknown as HTMLCanvasElement,
    {
      client: controlled,
      binding,
      flush: async () => {
        await client.inspectPage({ collection: "summary" });
      },
      picked: async (result) => {
        assert.ok(result.ok && result.hit);
        const record = {
          clicks: 0,
          moves: 0,
          started: 0,
          stopped: 0,
          finished: 0,
        };
        records.push(record);
        if (pickedHold) {
          const held = pickedHold;
          pickedHold = undefined;
          held.entered.resolve();
          await held.release.promise;
        }
        return {
          click() {
            record.clicks++;
          },
          move() {
            record.moves++;
          },
          dragging(active) {
            if (active) record.started++;
            else record.stopped++;
          },
          finish() {
            record.finished++;
          },
        };
      },
      pending(delta) {
        if (pending === 0 && delta > 0) idle = Promise.withResolvers<void>();
        pending += delta;
        assert.ok(pending >= 0);
        if (pending === 0) idle.resolve();
      },
      error(error) {
        failures.push(error);
      },
    },
  );
  async function press() {
    surface.pointer("pointerdown", 200);
    await idle.promise;
    assert.ok(surface.hasPointerCapture(1));
    return records.at(-1)!;
  }
  async function freshClick() {
    const count = records.length;
    const fresh = await press();
    assert.equal(records.length, count + 1);
    surface.pointer("pointerup", 200, 0, 0);
    assert.deepEqual(fresh, {
      clicks: 1,
      moves: 0,
      started: 0,
      stopped: 0,
      finished: 1,
    });
    assert.equal(surface.hasPointerCapture(1), false);
    assert.equal(pending, 0);
  }
  try {
    const projected = await press();
    const projection = { ...hold(), type: "CameraProjectQuery" as const };
    queryHold = projection;
    surface.pointer("pointermove", 250);
    await projection.entered.promise;
    await rebind();
    projection.release.resolve();
    await idle.promise;
    assert.deepEqual(projected, {
      clicks: 0,
      moves: 0,
      started: 1,
      stopped: 1,
      finished: 1,
    });
    assert.equal(surface.hasPointerCapture(1), false);
    await freshClick();

    const picking = { ...hold(), type: "GeometryPickQuery" as const };
    queryHold = picking;
    const beforePick = records.length;
    surface.pointer("pointerdown", 200);
    await picking.entered.promise;
    await rebind();
    picking.release.resolve();
    await idle.promise;
    assert.equal(records.length, beforePick);
    assert.equal(surface.hasPointerCapture(1), false);
    await freshClick();

    const completion = hold();
    pickedHold = completion;
    surface.pointer("pointerdown", 200);
    await completion.entered.promise;
    const completing = records.at(-1)!;
    await rebind();
    completion.release.resolve();
    await idle.promise;
    assert.deepEqual(completing, {
      clicks: 0,
      moves: 0,
      started: 0,
      stopped: 0,
      finished: 1,
    });
    assert.equal(surface.hasPointerCapture(1), false);
    await freshClick();

    const released = await press();
    await rebind();
    surface.pointer("pointerup", 200, 0, 0);
    assert.deepEqual(released, {
      clicks: 0,
      moves: 0,
      started: 0,
      stopped: 0,
      finished: 1,
    });
    assert.equal(surface.hasPointerCapture(1), false);
    await freshClick();
    assert.deepEqual(failures, []);
    return records;
  } finally {
    dispose();
  }
}

test("generated client and gallery controller keep current-bound gestures usable across autonomous Host frames", {
  timeout: 60_000,
}, async (context) => {
  const contract = await import(
    pathToFileURL(resolve("target/composed-queries/generated.js")).href
  );
  await runNativeEnvironment(
    "composed-camera-current-source",
    {
      executable: resolve("target/integration-artifacts/native/ipp-server"),
      schemaArtifact: resolve(
        "target/integration-artifacts/native/contract.bin",
      ),
      workingDirectory: process.cwd(),
    },
    context.signal,
    async (environment) => {
      const host: HostClientBase<PickingWorldClient> = await environment.track(
        contract.IppHostClient.connectTransport(
          webSocketTransport(environment.url),
        ),
      );
      const world = await host.createWorld({
        selectedSystems: selectSystems(CAMERA),
      });
      const client = await host.openWorld(world.reference);
      const commands = [
        createEntity(1, "camera"),
        insertComponent(
          client,
          "Camera",
          { kind: "alias", alias: 1 },
          { projection: 1, ortho_height: 4 },
        ),
        insertComponent(
          client,
          "Transform",
          { kind: "alias", alias: 1 },
          { z: 5 },
        ),
        createEntity(2, "draggable"),
        insertComponent(
          client,
          "PickingGeometry",
          { kind: "alias", alias: 2 },
          {
            geometry: contract.encodeBoundingShape({
              type: "box",
              min: [-1, -1, -0.2],
              max: [1, 1, 0.2],
            }),
          },
        ),
      ];
      const created = successfulBatch(await client.batch(commands));
      const output = await host.bindOutput(
        world.reference,
        aliasId(created, 1),
        "camera",
      );
      let binding = await host.setRootOutput(output, {
        width: 400,
        height: 200,
        devicePixelRatio: 1,
      });
      const view = () => ({ kind: "bound" as const, binding });
      const picked = await client.query({
        type: "GeometryPickQuery",
        view: view(),
        x: 0.5,
        y: 0.5,
        includeViewPlane: true,
      });
      assert.ok(picked.ok && picked.hit?.viewPlane);
      assert.equal(picked.hit.entity, aliasId(created, 2));
      assert.deepEqual(picked.hit.world, world.reference);
      const plane = picked.hit.viewPlane;
      let lastTick = picked.tick;
      const observed = [];
      for (let sample = 1; sample <= 5; sample++) {
        const barrier = await client.inspectPage({ collection: "summary" });
        assert.ok(barrier.tick > lastTick);
        const projected = await client.query({
          type: "CameraProjectQuery",
          view: view(),
          x: 0.5 + sample * 0.025,
          y: 0.5,
          plane,
        });
        assert.ok(projected.ok && projected.position);
        assert.ok(projected.tick > barrier.tick);
        near(projected.position[0], sample * 0.2);
        assert.notDeepEqual(
          projected.view.publication,
          picked.view.publication,
        );
        observed.push({
          tick: projected.tick,
          source: projected.view.publication,
          position: projected.position,
        });
        lastTick = projected.tick;
      }
      const obsolete = await client.query({
        type: "CameraProjectQuery",
        view: { ...view(), publication: picked.view.publication },
        x: 0.5,
        y: 0.5,
        plane,
      });
      assert.equal(obsolete.ok, false);

      const surface = new PointerSurface();
      const previousWindow = Object.getOwnPropertyDescriptor(
        globalThis,
        "window",
      );
      const previousFrame = Object.getOwnPropertyDescriptor(
        globalThis,
        "requestAnimationFrame",
      );
      const previousCancel = Object.getOwnPropertyDescriptor(
        globalThis,
        "cancelAnimationFrame",
      );
      Object.defineProperty(globalThis, "window", {
        configurable: true,
        value: new EventTarget(),
      });
      let frameId = 0;
      const frames = new Map<number, ReturnType<typeof setTimeout>>();
      Object.defineProperty(globalThis, "requestAnimationFrame", {
        configurable: true,
        value: (callback: FrameRequestCallback) => {
          const id = ++frameId;
          frames.set(
            id,
            setTimeout(() => {
              frames.delete(id);
              callback(performance.now());
            }, 0),
          );
          return id;
        },
      });
      Object.defineProperty(globalThis, "cancelAnimationFrame", {
        configurable: true,
        value: (id: number) => {
          clearTimeout(frames.get(id));
          frames.delete(id);
        },
      });
      const pickReady = Promise.withResolvers<void>();
      const moved = Promise.withResolvers<[number, number, number]>();
      const panReady = Promise.withResolvers<void>();
      let waitingForPan = false;
      let pending = 0;
      const failures: unknown[] = [];
      let bindingCancellations;
      const dispose = installCameraControls(
        surface as unknown as HTMLCanvasElement,
        {
          client,
          binding: () => binding,
          flush: async () => {
            await client.inspectPage({ collection: "summary" });
          },
          picked: async (result) => {
            assert.ok(result.ok && result.hit);
            assert.deepEqual(result.hit.world, world.reference);
            pickReady.resolve();
            return {
              click() {},
              dragging() {},
              move(delta) {
                moved.resolve(delta);
              },
            };
          },
          pending(delta) {
            pending += delta;
            if (waitingForPan && pending === 0) panReady.resolve();
          },
          error(error) {
            failures.push(error);
            moved.reject(error);
            panReady.reject(error);
          },
        },
      );
      try {
        surface.pointer("pointerdown", 200);
        await pickReady.promise;
        const before = await client.inspectPage({ collection: "summary" });
        await client.inspectPage({ collection: "summary" });
        surface.pointer("pointermove", 250);
        const delta = await moved.promise;
        near(delta[0], 1);
        near(delta[1], 0);
        surface.pointer("pointerup", 250, 0, 0);
        const after = await client.inspectPage({ collection: "summary" });
        assert.ok(after.tick > before.tick);
        surface.pointer("pointerdown", 200, 1, 4);
        waitingForPan = true;
        surface.pointer("pointermove", 220, 1, 4);
        surface.pointer("pointerup", 220, 1, 0);
        await panReady.promise;
        const projected = await client.query({
          type: "CameraProjectQuery",
          view: view(),
          x: 0.5,
          y: 0.5,
          plane,
        });
        assert.ok(projected.ok && projected.position);
        near(projected.position[0], -0.4);
        assert.deepEqual(failures, []);
        assert.equal(pending, 0);
        dispose();
        bindingCancellations = await controllerBindingRevalidation(
          surface,
          client,
          () => binding,
          async () => {
            const previous = binding;
            binding = await host.setRootOutput(output, binding.viewport);
            assert.notDeepEqual(binding.generation, previous.generation);
          },
        );
      } finally {
        dispose();
        for (const timeout of frames.values()) clearTimeout(timeout);
        for (const [key, previous] of [
          ["window", previousWindow],
          ["requestAnimationFrame", previousFrame],
          ["cancelAnimationFrame", previousCancel],
        ] as const) {
          if (previous) Object.defineProperty(globalThis, key, previous);
          else Reflect.deleteProperty(globalThis, key);
        }
      }

      for (let sample = 0; sample < 3; sample++) {
        await client.inspectPage({ collection: "summary" });
        await client.navigateCamera({
          binding,
          motion: { kind: "pan", x: 0.05, y: 0 },
        });
      }
      const final = await client.query({
        type: "CameraProjectQuery",
        view: view(),
        x: 0.5,
        y: 0.5,
        plane,
      });
      assert.ok(final.ok && final.position);
      near(final.position[0], -1.6);
      await assert.rejects(
        client.navigateCamera({
          binding,
          publication: picked.view.publication,
          motion: { kind: "pan", x: 1, y: 0 },
        }),
      );
      const oldBinding: RootBinding = binding;
      binding = await host.setRootOutput(output, binding.viewport);
      assert.notDeepEqual(binding.generation, oldBinding.generation);
      const stale = await client.query({
        type: "CameraProjectQuery",
        view: { kind: "bound", binding: oldBinding },
        x: 0.5,
        y: 0.5,
        plane,
      });
      assert.equal(stale.ok, false);
      await assert.rejects(
        client.navigateCamera({
          binding: oldBinding,
          motion: { kind: "pan", x: 1, y: 0 },
        }),
      );
      await environment.evidence.writeJson("current-source-gestures.json", {
        observed,
        final,
        obsolete,
        stale,
        bindingCancellations,
      });
      await host.destroyWorld(world.reference);
    },
  );
});
