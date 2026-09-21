import assert from "node:assert/strict";
import test from "node:test";
import type { AnimationControllerSnapshot, Inspection } from "@ipp/client";
import type { Page } from "playwright";
import { runBrowserEnvironment } from "../browser/environment.js";
import {
  entity,
  galleryEnvironment,
  openGallery,
  position,
} from "./gallery-driver.js";
import { requireVisible } from "./image-assertions.js";

interface PlatformerMetadata {
  route: {
    firstLegDuration: number;
    ramp: { time: number; position: number[] };
    upper: { time: number; position: number[] };
  };
  clips: Record<"walk" | "run" | "crawl", { duration: number }>;
}

interface PlatformerSceneMetadata {
  scene: {
    entities: Array<{
      id: string;
      mesh?: { source: string };
      texture?: { source: string };
    }>;
  };
}

interface PlatformerCatalogEntry {
  bytes: number;
  contentType: string;
}

async function platformerSceneResource(
  page: Page,
  entityId: string,
  kind: "mesh" | "texture",
) {
  const base = "/target/gallery-platformer-assets/";
  const { source, catalog } = await page.evaluate(
    async ({ base, entityId, kind }) => {
      const [sceneResponse, catalogResponse] = await Promise.all([
        fetch(`${base}blender-scene.json`),
        fetch(`${base}catalog.json`),
      ]);
      if (!sceneResponse.ok)
        throw new Error(`blender-scene.json: HTTP ${sceneResponse.status}`);
      if (!catalogResponse.ok)
        throw new Error(`catalog.json: HTTP ${catalogResponse.status}`);
      const metadata = (await sceneResponse.json()) as PlatformerSceneMetadata;
      const entity = metadata.scene.entities.find(
        (candidate) => candidate.id === entityId,
      );
      return {
        source: entity?.[kind]?.source,
        catalog: (await catalogResponse.json()) as Record<
          string,
          PlatformerCatalogEntry
        >,
      };
    },
    { base, entityId, kind },
  );
  assert.ok(source, `Missing ${kind} source for ${entityId}`);
  assert.ok(
    source.startsWith("/assets/"),
    `${entityId} ${kind} must use the gallery asset namespace`,
  );
  const resource = source.slice("/assets/".length);
  const entry = catalog[resource];
  assert.ok(entry, `${entityId} ${kind} is missing from the asset catalog`);
  assert.ok(entry.bytes > 0, `${entityId} ${kind} catalog entry is empty`);
  return resource;
}

const IDS = [
  "platformer-root",
  "platformer-rig",
  "platformer-character",
  "platformer-camera",
  "platformer-camera-target",
  "platformer-overhead-light",
  "platformer-orb",
] as const;

const CLIP_NAMES = {
  walk: "dd030573edc3333396a7846cbc01a7dd19c38ad64e1e34ab8ca573d0c0fe2239",
  run: "ae3f72e4775534450e22029267ff0391c5ec1b93e75a811d7461ecfa55cda0c0",
  crawl: "5764911b0b9f202230861f9f6cec2c3186e76cbe70741e56fded177d4a9891c7",
} as const;
const WALK_DURATION = 16 / 15;

function controllerFor(state: Inspection, target: bigint) {
  const matches = (state.controllers ?? []).filter((controller) =>
    controller.description.drivers.some((driver) => driver.target === target),
  );
  assert.equal(matches.length, 1, `Expected one controller for ${target}`);
  return matches[0]!;
}

function gaitControllersFor(state: Inspection, target: bigint) {
  return (state.controllers ?? []).filter((controller) =>
    controller.description.drivers.some(
      (driver) =>
        driver.target === target &&
        driver.source.startsWith("https://platformer.ipp.invalid/"),
    ),
  );
}

function gaitControllerFor(state: Inspection, target: bigint) {
  const matches = gaitControllersFor(state, target);
  assert.equal(matches.length, 1, `Expected one gait controller for ${target}`);
  return matches[0]!;
}

function facingControllerFor(state: Inspection, target: bigint) {
  const matches = (state.controllers ?? []).filter((controller) =>
    controller.description.drivers.some(
      (driver) =>
        driver.target === target &&
        !driver.source.startsWith("https://platformer.ipp.invalid/"),
    ),
  );
  assert.equal(
    matches.length,
    1,
    `Expected one facing controller for ${target}`,
  );
  return matches[0]!;
}

function parentOf(state: Inspection, id: string) {
  return entity(state, id).base.find(
    (component) => "parent" in component.fields,
  )?.fields.parent;
}

function distance(a: number[], b: number[]) {
  return Math.hypot(...a.map((value, index) => value - b[index]!));
}

function forwardLoopElapsed(current: number, start: number, duration: number) {
  return (((current - start) % duration) + duration) % duration;
}

async function continueCanceledRoute(route: { continue(): Promise<void> }) {
  try {
    await route.continue();
  } catch (failure) {
    if (
      failure instanceof Error &&
      failure.message.includes("Route is already handled")
    )
      return;
    throw failure;
  }
}

function basePosition(state: Inspection, id: string) {
  const fields = entity(state, id).base.find(
    (component) => "qx" in component.fields,
  )!.fields;
  return [Number(fields.x), Number(fields.y), Number(fields.z)];
}

function effectiveRotation(
  state: Inspection,
  id: string,
): [number, number, number, number] {
  const fields = entity(state, id).effective.find(
    (component) => "qx" in component.fields,
  )!.fields;
  return [
    Number(fields.qx),
    Number(fields.qy),
    Number(fields.qz),
    Number(fields.qw),
  ];
}

test("platformer keeps moving while a new gait loads and reuses cached clips", {
  timeout: 120_000,
}, async (context) => {
  await runBrowserEnvironment(
    "platformer delayed gait",
    galleryEnvironment,
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario);
      const clipNames = {
        walk: "dd030573edc3333396a7846cbc01a7dd19c38ad64e1e34ab8ca573d0c0fe2239",
        run: "ae3f72e4775534450e22029267ff0391c5ec1b93e75a811d7461ecfa55cda0c0",
        crawl:
          "5764911b0b9f202230861f9f6cec2c3186e76cbe70741e56fded177d4a9891c7",
      } as const;
      const requests: Record<keyof typeof clipNames, number> = {
        walk: 0,
        run: 0,
        crawl: 0,
      };
      let releaseRun!: () => void;
      let reportRunRequest!: () => void;
      const runRelease = new Promise<void>((resolve) => {
        releaseRun = resolve;
      });
      const runRequested = new Promise<void>((resolve) => {
        reportRunRequest = resolve;
      });
      for (const [mode, name] of Object.entries(clipNames) as Array<
        [keyof typeof clipNames, string]
      >) {
        await g.page.route(
          `**/target/gallery-platformer-assets/${name}`,
          async (route) => {
            requests[mode] += 1;
            const response = await route.fetch();
            const body = await response.body();
            if (mode === "run" && requests.run === 1) {
              reportRunRequest();
              await runRelease;
            }
            await route.fulfill({
              response,
              body,
              headers: { ...response.headers(), "cache-control": "no-store" },
            });
          },
        );
      }

      try {
        await g.navigate("platformer");
        await g.page.waitForSelector('#platformer-status[data-state="ready"]');
        const initial = await g.waitFor((state) => {
          const rig = state.entities.find(
            (value) => value.metadata.symbolicId === "platformer-rig",
          )?.id;
          const gaits = rig === undefined ? [] : gaitControllersFor(state, rig);
          return gaits.length === 1 && gaits[0]!.state === "playing";
        });
        const root = entity(initial, "platformer-root").id;
        const rig = entity(initial, "platformer-rig").id;
        const orb = entity(initial, "platformer-orb").id;
        const beforeGait = gaitControllerFor(initial, rig);

        await g.page.locator("#platformer-mode-run").click();
        await runRequested;
        const loading = await g.inspect();
        const outgoing = gaitControllersFor(loading, rig).find((controller) =>
          controller.description.drivers[0]!.source.endsWith(clipNames.walk),
        );
        const incoming = gaitControllersFor(loading, rig).find((controller) =>
          controller.description.drivers[0]!.source.endsWith(clipNames.run),
        );
        assert.ok(outgoing, "The outgoing Walk controller remains present");
        assert.ok(incoming, "The pending Run controller retains asset demand");
        assert.equal(
          outgoing.description.drivers[0]!.source,
          beforeGait.description.drivers[0]!.source,
          "The outgoing Walk clip remains active while Run loads",
        );
        assert.equal(incoming.state, "stopped");
        await g.waitFor((state) => {
          const walk = gaitControllersFor(state, rig).find((controller) =>
            controller.description.drivers[0]!.source.endsWith(clipNames.walk),
          );
          return (
            distance(
              position(state, "platformer-root"),
              position(initial, "platformer-root"),
            ) > 0.08 &&
            walk !== undefined &&
            forwardLoopElapsed(walk.time, beforeGait.time, WALK_DURATION) >
              0.08 &&
            walk.state === "playing"
          );
        });

        const isolated = await g.page.evaluate(
          ({ route, facing, orb }) => {
            const client = window.ippWorldCanvas!
              .client as import("@ipp/client").AnimationWorldClient;
            for (const id of [route, facing, orb])
              client.playback(BigInt(id), { action: "pause" });
            return client.inspect();
          },
          {
            route: String(controllerFor(loading, root).id),
            facing: String(facingControllerFor(loading, rig).id),
            orb: String(controllerFor(loading, orb).id),
          },
        );
        assert.equal(controllerFor(isolated, root).state, "paused");
        assert.equal(facingControllerFor(isolated, rig).state, "paused");
        assert.equal(controllerFor(isolated, orb).state, "paused");
        const isolatedWalk = gaitControllersFor(isolated, rig).find(
          (controller) =>
            controller.description.drivers[0]!.source.endsWith(clipNames.walk),
        );
        assert.equal(isolatedWalk?.state, "playing");
        await g.capturePending("platformer-delayed-walk-a");
        const moving = await g.waitFor((state) => {
          const gait = gaitControllersFor(state, rig).find((controller) =>
            controller.description.drivers[0]!.source.endsWith(clipNames.walk),
          );
          return (
            gait !== undefined &&
            forwardLoopElapsed(gait.time, isolatedWalk!.time, WALK_DURATION) >
              0.12 &&
            gait.state === "playing" &&
            controllerFor(state, root).state === "paused" &&
            facingControllerFor(state, rig).state === "paused" &&
            controllerFor(state, orb).state === "paused"
          );
        });
        assert.equal(
          distance(
            position(moving, "platformer-root"),
            position(isolated, "platformer-root"),
          ),
          0,
          "Route motion is excluded from the gait-only frame comparison",
        );
        await g.capturePending("platformer-delayed-walk-b");
        assert.ok(
          (
            await g.difference(
              "platformer-delayed-walk-a",
              "platformer-delayed-walk-b",
            )
          ).changedPixels > 50,
          "Completed frames show articulated Walk motion while every non-gait controller is paused",
        );

        await g.page.locator("#platformer-pause").click();
        await g.waitFor((state) => {
          const walk = gaitControllersFor(state, rig).find((controller) =>
            controller.description.drivers[0]!.source.endsWith(clipNames.walk),
          );
          return (
            walk?.state === "paused" &&
            controllerFor(state, root).state === "paused" &&
            facingControllerFor(state, rig).state === "paused" &&
            controllerFor(state, orb).state === "paused"
          );
        });
        assert.equal(
          await g.page.locator("#platformer-pause").textContent(),
          "Resume",
          "Pause remains responsive while the incoming clip is pending",
        );
        await g.page.locator("#platformer-pause").click();

        releaseRun();
        const running = await g.waitFor((state) => {
          const gaits = gaitControllersFor(state, rig);
          return (
            gaits.length === 1 &&
            gaits[0]!.description.drivers[0]!.source.endsWith(clipNames.run)
          );
        });
        assert.ok(gaitControllerFor(running, rig).time > 0);
        await g.page.locator("#platformer-mode-walk").click();
        await g.waitFor((state) => {
          const gaits = gaitControllersFor(state, rig);
          return (
            gaits.length === 1 &&
            gaits[0]!.description.drivers[0]!.source.endsWith(clipNames.walk)
          );
        });
        assert.deepEqual(requests, { walk: 1, run: 1, crawl: 0 });
        assert.equal(
          await g.page.locator("#platformer-status").getAttribute("data-state"),
          "ready",
        );
        assert.deepEqual(g.errors, []);
      } finally {
        releaseRun();
        for (const name of Object.values(clipNames))
          await g.page.unroute(`**/target/gallery-platformer-assets/${name}`);
      }
    },
  );
});

test("a newer gait request cancels a pending clip and commits only the latest mode", {
  timeout: 60_000,
}, async (context) => {
  await runBrowserEnvironment(
    "platformer superseded gait",
    galleryEnvironment,
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario);
      let releaseRun!: () => void;
      let reportRun!: () => void;
      const held = new Promise<void>((resolve) => {
        releaseRun = resolve;
      });
      const requested = new Promise<void>((resolve) => {
        reportRun = resolve;
      });
      const pattern = `**/target/gallery-platformer-assets/${CLIP_NAMES.run}`;
      await g.page.route(pattern, async (route) => {
        reportRun();
        await held;
        await continueCanceledRoute(route);
      });
      try {
        await g.navigate("platformer");
        await g.page.waitForSelector('#platformer-status[data-state="ready"]');
        const initial = await g.inspect();
        const rig = entity(initial, "platformer-rig").id;
        await g.page.locator("#platformer-mode-run").click();
        await requested;
        await g.page.locator("#platformer-mode-crawl").click();
        const crawled = await g.waitFor(
          (state) =>
            state.controllers?.length === 4 &&
            gaitControllerFor(
              state,
              rig,
            ).description.drivers[0]!.source.endsWith(CLIP_NAMES.crawl),
        );
        assert.equal(gaitControllerFor(crawled, rig).state, "playing");
        await g.page.waitForFunction(
          () =>
            document.querySelector<HTMLInputElement>("#platformer-mode-crawl")
              ?.checked === true,
        );
        assert.equal(
          await g.page.locator("#platformer-mode-crawl").isChecked(),
          true,
        );
        assert.equal(
          await g.page.locator("#platformer-mode-run").isChecked(),
          false,
        );
      } finally {
        releaseRun();
        await g.page.unroute(pattern);
      }
    },
  );
});

test("navigation closes platformer promptly while a gait request is pending", {
  timeout: 60_000,
}, async (context) => {
  await runBrowserEnvironment(
    "platformer pending gait navigation",
    galleryEnvironment,
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario);
      let releaseRun!: () => void;
      let reportRun!: () => void;
      const held = new Promise<void>((resolve) => {
        releaseRun = resolve;
      });
      const requested = new Promise<void>((resolve) => {
        reportRun = resolve;
      });
      const pattern = `**/target/gallery-platformer-assets/${CLIP_NAMES.run}`;
      await g.page.route(pattern, async (route) => {
        reportRun();
        await held;
        await continueCanceledRoute(route);
      });
      try {
        await g.navigate("platformer");
        await g.page.waitForSelector('#platformer-status[data-state="ready"]');
        await g.page.locator("#platformer-mode-run").click();
        await requested;
        const departing = await g.page.evaluateHandle(
          () => window.ippWorldCanvas!,
        );
        await g.navigate("shapes");
        await departing.evaluate((canvas) => canvas.closed);
        await departing.dispose();
        assert.equal(await g.page.locator("#platformer-status").count(), 0);
        assert.deepEqual(g.errors, []);
      } finally {
        releaseRun();
        await g.page.unroute(pattern);
      }
    },
  );
});

test("platformer changes gait without resetting, reverses its loop and keeps the camera, light and orb attached", {
  timeout: 120_000,
}, async (context) => {
  await runBrowserEnvironment(
    "platformer locomotion",
    galleryEnvironment,
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario);
      const consoleErrors: string[] = [];
      g.page.on("console", (message) => {
        if (message.type() === "error") consoleErrors.push(message.text());
      });
      await g.navigate("platformer");
      await g.page.waitForSelector('#platformer-status[data-state="ready"]');
      await g.page.evaluate(() => {
        (
          window as typeof window & { platformerInitialClient?: object }
        ).platformerInitialClient = window.ippWorldCanvas!.client;
      });
      const metadata = (await g.page.evaluate(async () => {
        const base = "/target/gallery-platformer-assets/";
        const [manifest, route] = await Promise.all(
          ["manifest.json", "route.json"].map(async (name) => {
            const response = await fetch(base + name);
            if (!response.ok)
              throw new Error(`${name}: HTTP ${response.status}`);
            return response.json();
          }),
        );
        const first = route.waypoints[0].position as number[];
        const second = route.waypoints[1].position as number[];
        const firstLegDuration =
          Math.hypot(...first.map((value, axis) => second[axis]! - value)) /
          route.modes.walk.speed;
        const segmentDuration = (index: number) => {
          const start = route.waypoints[index].position as number[];
          const end = route.waypoints[index + 1].position as number[];
          return (
            Math.hypot(...start.map((value, axis) => end[axis]! - value)) /
            route.modes.walk.speed
          );
        };
        const segmentStart = (index: number) =>
          Array.from({ length: index }, (_, segment) =>
            segmentDuration(segment),
          ).reduce((sum, duration) => sum + duration, 0);
        const midpoint = (index: number) => {
          const start = route.waypoints[index].position as number[];
          const end = route.waypoints[index + 1].position as number[];
          return {
            time: segmentStart(index) + segmentDuration(index) / 2,
            position: start.map((value, axis) => (value + end[axis]!) / 2),
          };
        };
        const clips = Object.fromEntries(
          Object.entries(route.modes).map(([mode, settings]) => {
            const match = manifest.clips.find(
              (clip: { name: string; target: string }) =>
                clip.name === (settings as { clip: string }).clip &&
                clip.target === "platformer-rig",
            );
            if (!match) throw new Error(`Missing ${mode} clip`);
            return [mode, { duration: match.clip.duration }];
          }),
        );
        return {
          route: {
            firstLegDuration,
            ramp: midpoint(2),
            upper: midpoint(4),
          },
          clips,
        };
      })) as PlatformerMetadata;
      const initial = await g.waitFor(
        (state) =>
          IDS.every((id) =>
            state.entities.some((value) => value.metadata.symbolicId === id),
          ) &&
          state.resources.every((resource) => resource.status === "loaded") &&
          entity(state, "platformer-camera").effective.some(
            (component) => "target" in component.fields,
          ),
      );
      const root = entity(initial, "platformer-root").id;
      const rig = entity(initial, "platformer-rig").id;
      const cameraTarget = entity(initial, "platformer-camera-target").id;
      const orb = entity(initial, "platformer-orb").id;
      const rootController = controllerFor(initial, root);
      const gaitController = gaitControllerFor(initial, rig);
      facingControllerFor(initial, rig);
      const orbController = controllerFor(initial, orb);
      assert.equal(initial.controllers?.length, 4);
      assert.ok(
        gaitController.description.drivers.every(
          (driver) =>
            driver.source.startsWith("https://platformer.ipp.invalid/") &&
            driver.repeat,
        ),
        "The gait uses the imported immutable Blender clip",
      );
      for (const id of [
        "platformer-camera",
        "platformer-camera-target",
        "platformer-overhead-light",
        "platformer-orb",
      ])
        assert.equal(
          parentOf(initial, id),
          root,
          `${id} follows the route root`,
        );
      assert.ok(
        distance(basePosition(initial, "platformer-camera"), [14, 6, 0]) <
          0.001,
        "The camera retains its authored side-follow offset",
      );
      const lookAt = entity(initial, "platformer-camera").effective.find(
        (component) => "target" in component.fields,
      );
      assert.ok(lookAt);
      assert.equal(lookAt.fields.target, cameraTarget);
      assert.equal(lookAt.fields.enabled, true);
      const light = entity(initial, "platformer-overhead-light").effective.find(
        (component) => "intensity" in component.fields,
      );
      assert.equal(
        light?.fields.kind,
        1,
        "The attached overhead light is a point light",
      );
      assert.ok(
        entity(initial, "platformer-orb").effective.some(
          (component) => component.properties && "time" in component.properties,
        ),
        "The orb exposes its shader time property",
      );

      const start = position(initial, "platformer-root");
      await g.waitFor(
        (state) => distance(position(state, "platformer-root"), start) > 0.25,
      );
      await g.page.locator("#platformer-pause").click();
      const paused = await g.waitFor((state) =>
        state.controllers!.every((controller) => controller.state === "paused"),
      );
      const pausedPosition = position(paused, "platformer-root");
      assert.ok(distance(pausedPosition, start) > 0.25);
      const phase = 0.37;
      const seek = async (
        controller: AnimationControllerSnapshot,
        time: number,
      ) => {
        await g.page.evaluate(
          ({ id, time }) => {
            const client = window.ippWorldCanvas!
              .client as import("@ipp/client").AnimationWorldClient;
            client.playback(BigInt(id), { action: "seek", time });
          },
          { id: String(controller.id), time },
        );
        return g.waitFor((state) => {
          const current = state.controllers!.find(
            (candidate) => candidate.id === controller.id,
          );
          return (
            current?.state === "paused" && Math.abs(current.time - time) < 0.001
          );
        });
      };
      await seek(gaitController, phase * metadata.clips.walk.duration);
      const walkFrame = await g.capture("platformer-walk-phase");
      requireVisible(walkFrame.summary, "platformer track and character");
      assert.ok(walkFrame.summary.coverage > 0.04);
      const sources = new Set<string>();
      for (const mode of ["walk", "run", "crawl"] as const) {
        await g.page.locator(`#platformer-mode-${mode}`).click();
        const changed = await g.waitFor((state) => {
          const gaits = gaitControllersFor(state, rig);
          if (gaits.length !== 1) return false;
          const controller = gaits[0]!;
          return (
            controller.id === gaitController.id &&
            controller.state === "paused" &&
            !sources.has(controller.description.drivers[0]!.source)
          );
        });
        await g.page.waitForFunction(
          (mode) =>
            document.querySelector<HTMLInputElement>(`#platformer-mode-${mode}`)
              ?.checked === true,
          mode,
        );
        const controller = gaitControllerFor(changed, rig);
        sources.add(controller.description.drivers[0]!.source);
        assert.ok(
          Math.abs(controller.time / metadata.clips[mode].duration - phase) <
            0.015,
          `${mode} retained normalized gait phase`,
        );
        if (mode !== "walk") {
          assert.equal(controller.transition?.easing, "smoothstep");
          assert.ok((controller.transition?.duration ?? 0) > 0);
        }
        assert.ok(
          distance(position(changed, "platformer-root"), pausedPosition) <
            0.001,
        );
      }
      assert.equal(sources.size, 3);

      // Finish Crawl's crossfade while the route and orb remain paused, then
      // seek the same phase so changed pixels isolate the imported gait pose.
      await g.page.evaluate(
        ({ id }) => {
          const client = window.ippWorldCanvas!
            .client as import("@ipp/client").AnimationWorldClient;
          client.playback(BigInt(id), { action: "play" });
        },
        { id: String(gaitController.id) },
      );
      const crawlPlaying = await g.waitFor((state) => {
        const gaits = gaitControllersFor(state, rig);
        if (gaits.length !== 1) return false;
        const controller = gaits[0]!;
        return (
          controller.state === "playing" && controller.transition === undefined
        );
      });
      await g.page.evaluate(
        ({ id }) => {
          const client = window.ippWorldCanvas!
            .client as import("@ipp/client").AnimationWorldClient;
          client.playback(BigInt(id), { action: "pause" });
        },
        { id: String(gaitControllerFor(crawlPlaying, rig).id) },
      );
      await seek(gaitController, phase * metadata.clips.crawl.duration);
      await g.capture("platformer-crawl-phase");
      assert.ok(
        (await g.difference("platformer-walk-phase", "platformer-crawl-phase"))
          .changedPixels > 50,
        "Imported Walk and Crawl clips produce different visible character poses",
      );

      const routeTime = metadata.route.firstLegDuration * 0.72;
      const previousTime = metadata.route.firstLegDuration * 0.52;
      const previous = await seek(rootController, previousTime);
      const previousPosition = position(previous, "platformer-root");
      const routePaused = await seek(rootController, routeTime);
      const routePosition = position(routePaused, "platformer-root");
      const forwardFacing = effectiveRotation(routePaused, "platformer-rig");
      assert.ok(distance(routePosition, previousPosition) > 0.2);
      await g.page.locator("#platformer-reverse").click();
      await g.page.locator("#platformer-pause").click();
      const turning = await g.waitFor((state) => {
        const facing = facingControllerFor(state, rig);
        return (
          facing.state === "playing" &&
          facing.transition?.easing === "smoothstep" &&
          (facing.transition?.duration ?? 0) > 0 &&
          distance(effectiveRotation(state, "platformer-rig"), forwardFacing) >
            0.02
        );
      });
      assert.ok(
        (gaitControllerFor(turning, rig).description.speed ?? 0) > 0,
        "Reverse keeps the selected gait playing forward",
      );
      assert.ok(
        distance(effectiveRotation(turning, "platformer-rig"), forwardFacing) >
          0.02,
        "Reverse begins a visible local-space turnaround",
      );
      await g.capture("platformer-smooth-turn");
      const reversed = await g.waitFor((state) => {
        const route = controllerFor(state, root);
        return (
          route.state === "playing" &&
          (route.description.speed ?? 0) < 0 &&
          (gaitControllerFor(state, rig).description.speed ?? 0) > 0 &&
          route.time < routeTime - 0.08 &&
          route.time > previousTime
        );
      });
      assert.ok(
        distance(position(reversed, "platformer-root"), previousPosition) <
          distance(routePosition, previousPosition),
        "Reverse moves toward the previous point without wrapping the loop",
      );
      assert.equal(
        await g.page.locator("#platformer-direction").textContent(),
        "reverse",
      );
      const turnedAround = await g.waitFor(
        (state) => facingControllerFor(state, rig).transition === undefined,
      );
      const [turnedQx, turnedQy, turnedQz, turnedQw] = effectiveRotation(
        turnedAround,
        "platformer-rig",
      );
      assert.ok(
        Math.abs(Math.abs(turnedQy) - 1) < 0.01 &&
          Math.abs(turnedQx) < 0.01 &&
          Math.abs(turnedQz) < 0.01 &&
          Math.abs(turnedQw) < 0.01,
        "Reverse completes the authored 180-degree local Y rotation",
      );
      await g.page.locator("#platformer-pause").click();
      await g.waitFor((state) =>
        state.controllers!.every((controller) => controller.state === "paused"),
      );
      await g.page.locator("#platformer-reset").click();
      const resetting = await g.waitFor((state) => {
        const facing = facingControllerFor(state, rig);
        return (
          controllerFor(state, root).time < 0.001 &&
          facing.transition?.easing === "smoothstep"
        );
      });
      await g.page.waitForFunction(
        () =>
          document.querySelector("#platformer-direction")?.textContent ===
          "forward",
      );
      assert.equal(
        await g.page.locator("#platformer-direction").textContent(),
        "forward",
      );
      assert.ok(
        distance(
          effectiveRotation(resetting, "platformer-rig"),
          forwardFacing,
        ) > 0.02,
        "Reset starts from the reversed facing pose",
      );
      await g.page.locator("#platformer-pause").click();
      const resetForward = await g.waitFor((state) => {
        const facing = facingControllerFor(state, rig);
        return facing.state === "playing" && facing.transition === undefined;
      });
      assert.ok(
        distance(
          effectiveRotation(resetForward, "platformer-rig"),
          forwardFacing,
        ) < 0.01,
        "Reset completes the smooth return to forward facing",
      );
      await g.page.locator("#platformer-pause").click();
      await g.waitFor((state) =>
        state.controllers!.every((controller) => controller.state === "paused"),
      );

      for (const [label, sample] of Object.entries({
        ramp: metadata.route.ramp,
        upper: metadata.route.upper,
      })) {
        const sampled = await seek(rootController, sample.time);
        assert.ok(
          distance(position(sampled, "platformer-root"), sample.position) <
            0.01,
          `${label} route sample follows the authored raised track`,
        );
        assert.ok(
          sampled.controllers!.every(
            (controller) => controller.state === "paused",
          ),
        );
        const frame = await g.capture(`platformer-${label}-track`);
        requireVisible(frame.summary, `${label} track and character`);
        assert.ok(frame.summary.coverage > 0.04);
        assert.ok(frame.frame.drawCalls > 0 && frame.frame.triangles > 0);
      }

      await seek(orbController, 0);
      await g.capture("platformer-orb-dim");
      await seek(orbController, 0.375);
      const bright = await g.capture("platformer-orb-bright");
      assert.ok(bright.frame.drawCalls > 0 && bright.frame.triangles > 0);
      assert.ok(
        (await g.difference("platformer-orb-dim", "platformer-orb-bright"))
          .changedPixels > 20,
        "The custom shader twinkle changes rendered orb pixels",
      );

      await g.navigate("shapes");
      const left = await g.inspect();
      assert.equal(
        await g.page.evaluate(
          () =>
            window.ippWorldCanvas!.client ===
            (window as typeof window & { platformerInitialClient?: object })
              .platformerInitialClient,
        ),
        false,
        "Leaving the disk World replaces its client session",
      );
      assert.ok(
        IDS.every(
          (id) =>
            !left.entities.some((value) => value.metadata.symbolicId === id),
        ),
      );
      await g.navigate("platformer");
      await g.page.waitForSelector('#platformer-status[data-state="ready"]');
      const returned = await g.inspect();
      assert.equal(returned.controllers?.length, 4);
      assert.equal(
        await g.page.evaluate(
          () =>
            window.ippWorldCanvas!.client ===
            (window as typeof window & { platformerInitialClient?: object })
              .platformerInitialClient,
        ),
        false,
        "Reentry uses a fresh client even when World-local IDs are reused",
      );
      controllerFor(returned, entity(returned, "platformer-root").id);
      gaitControllerFor(returned, entity(returned, "platformer-rig").id);
      facingControllerFor(returned, entity(returned, "platformer-rig").id);
      controllerFor(returned, entity(returned, "platformer-orb").id);
      assert.deepEqual(consoleErrors, []);
      assert.deepEqual(g.errors, []);
    },
  );
});

test("platformer loading covers the saved World and pending resources", {
  timeout: 60_000,
}, async (context) => {
  await runBrowserEnvironment(
    "platformer pending resource",
    galleryEnvironment,
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario);
      const resource = await platformerSceneResource(
        g.page,
        "platformer-track-east-approach",
        "texture",
      );
      let releaseWorld!: () => void;
      let releaseResource!: () => void;
      let reportWorld!: () => void;
      let reportResource!: () => void;
      const worldGate = new Promise<void>((resolve) => {
        releaseWorld = resolve;
      });
      const resourceGate = new Promise<void>((resolve) => {
        releaseResource = resolve;
      });
      const worldRequested = new Promise<void>((resolve) => {
        reportWorld = resolve;
      });
      const resourceRequested = new Promise<void>((resolve) => {
        reportResource = resolve;
      });
      await g.page.route(
        "**/target/gallery-platformer-assets/platformer.ipp",
        async (route) => {
          reportWorld();
          await worldGate;
          await route.continue();
        },
      );
      await g.page.route(
        `**/target/gallery-platformer-assets/${resource}`,
        async (route) => {
          reportResource();
          await resourceGate;
          await route.continue();
        },
      );
      try {
        await g.selectScene("platformer");
        await worldRequested;
        assert.equal(
          await g.page.locator("#status").getAttribute("data-state"),
          "switching",
        );
        assert.equal(
          await g.page.locator("#platformer-loading").isVisible(),
          true,
        );
        releaseWorld();
        await resourceRequested;
        await g.page.waitForFunction(() => window.ippWorldCanvas !== undefined);
        const pending = await g.waitFor((state) =>
          state.resources.some(
            (entry) =>
              entry.source.endsWith(resource) && entry.status !== "loaded",
          ),
        );
        assert.equal(
          await g.page.locator("#platformer-loading").isVisible(),
          true,
        );
        assert.equal(
          await g.page.locator("#platformer-mode-run").isDisabled(),
          true,
        );
        assert.ok(pending.entities.length > 0);
        assert.equal(
          await g.page.evaluate(() => {
            const canvas = document.querySelector("#ipp-world-canvas")!;
            const rect = canvas.getBoundingClientRect();
            return Boolean(
              document
                .elementFromPoint(
                  rect.x + rect.width / 2,
                  rect.y + rect.height / 2,
                )
                ?.closest("#platformer-loading"),
            );
          }),
          true,
          "The loading screen covers the canvas until the scene is complete",
        );
        const ids = pending.entities.map((entry) => entry.id);
        releaseResource();
        const loaded = await g.waitFor((state) =>
          state.resources.every((entry) => entry.status === "loaded"),
        );
        await g.page.waitForSelector('#status[data-state="ready"]');
        assert.equal(await g.page.locator("#platformer-loading").count(), 0);
        assert.deepEqual(
          loaded.entities.slice(0, ids.length).map((entry) => entry.id),
          ids,
          "resource arrival does not replace the deserialized World",
        );
        await g.capture("platformer-resource-arrived");
      } finally {
        releaseWorld();
        releaseResource();
        await g.page.unroute(
          "**/target/gallery-platformer-assets/platformer.ipp",
        );
        await g.page.unroute(`**/target/gallery-platformer-assets/${resource}`);
      }
    },
  );
});

test("platformer resource failure reports an error and navigation recovers", {
  timeout: 60_000,
}, async (context) => {
  await runBrowserEnvironment(
    "platformer loading failure",
    galleryEnvironment,
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario);
      const resource = await platformerSceneResource(
        g.page,
        "platformer-track-east-approach",
        "texture",
      );
      const pattern = `**/target/gallery-platformer-assets/${resource}`;
      await g.page.route(pattern, (route) =>
        route.fulfill({ status: 404, body: "Missing platform texture" }),
      );
      try {
        await g.page.goto(
          `${g.origin}/examples/world-gallery/index.html#platformer`,
        );
        await g.page.reload();
        await g.page.waitForSelector('#status[data-state="error"]');
        const overlay = g.page.locator("#platformer-loading");
        assert.equal(await overlay.isVisible(), true);
        assert.equal(await overlay.getAttribute("role"), "alert");
        assert.match(
          (await overlay.textContent()) ?? "",
          /Unable to load scene/,
        );
        await g.navigate("shapes");
        assert.equal(await overlay.count(), 0);
      } finally {
        await g.page.unroute(pattern);
      }
      await g.navigate("platformer");
      await g.page.waitForSelector('#platformer-status[data-state="ready"]');
      await g.capture("platformer-recovered-scene");
      assert.deepEqual(g.errors, []);
    },
  );
});

test("leaving platformer with a pending resource cancels startup", {
  timeout: 60_000,
}, async (context) => {
  await runBrowserEnvironment(
    "platformer loading cancellation",
    galleryEnvironment,
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario);
      const resource = await platformerSceneResource(
        g.page,
        "platformer-track-east-approach",
        "texture",
      );
      const pattern = `**/target/gallery-platformer-assets/${resource}`;
      let release!: () => void;
      let reportRequest!: () => void;
      const held = new Promise<void>((resolve) => {
        release = resolve;
      });
      const requested = new Promise<void>((resolve) => {
        reportRequest = resolve;
      });
      await g.page.route(pattern, async () => {
        reportRequest();
        await held;
      });
      try {
        await g.selectScene("platformer");
        await requested;
        await g.page.waitForFunction(() => window.ippWorldCanvas !== undefined);
        const departing = await g.page.evaluateHandle(
          () => window.ippWorldCanvas!,
        );
        await g.navigate("shapes");
        await departing.evaluate((canvas) => canvas.closed);
        await departing.dispose();
        assert.equal(await g.page.locator("#platformer-loading").count(), 0);
      } finally {
        release();
        await g.page.unroute(pattern);
      }
      await g.navigate("platformer");
      await g.page.waitForSelector('#platformer-status[data-state="ready"]');
      assert.deepEqual(g.errors, []);
    },
  );
});
