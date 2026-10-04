import assert from "node:assert/strict";
import { writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import type { Inspection, EntitySnapshot } from "@ipp/client";
import type { Page } from "playwright";
import type { BrowserEnvironmentContext } from "../browser/environment.js";
import { bigintJson, invoke, writeDataUrl } from "./evidence.js";
import {
  startGalleryServer,
  type GalleryServerOptions,
} from "./gallery-server.js";
import type {
  ViewerBrowserCapture,
  locateGalleryObject,
} from "./viewer-browser-helper.js";
import type { ImageDifference } from "./image-assertions.js";

export const galleryBuild = (name: "render" | "headless") => {
  const directory = resolve("target/browser-build", name);
  return {
    name,
    generatedModule: join(directory, "generated.js"),
    runtimeWasm: join(directory, "runtime.wasm"),
    contractArtifact: join(directory, "contract.bin"),
  };
};
export const galleryEnvironment = {
  workspace: resolve(process.cwd()),
  build: galleryBuild("render"),
  operationTimeoutMs: 15_000,
  closeTimeoutMs: 5_000,
  evidenceParent: resolve("target/integration-artifacts/gallery-combined"),
};

/**
 * Share of the gallery's canvas frame, per axis, that scenarios render into.
 *
 * Every rasterized and captured pixel costs CPU on a software renderer, so
 * scenarios draw a canvas of half the width and half the height. A smaller
 * viewport cannot do this: below 961 CSS pixels the gallery switches to its
 * phone layout, where the controls become a modal sheet. Scenarios therefore
 * keep the desktop layout and confine its canvas frame to this share of the
 * showcase. A scenario whose canvas size is part of what it proves, such as
 * the responsive layout scenario, opens the gallery with `canvasShare: 1`.
 */
export const GALLERY_CANVAS_SHARE = 0.5;

/**
 * Confine the gallery's canvas frame to `share` of its showcase on every
 * document the page loads. The frame keeps its place in the desktop layout,
 * and the canvas still fills the frame and follows its CSS size and density.
 */
export async function confineGalleryCanvas(page: Page, share: number) {
  assert.ok(share > 0 && share <= 1, `canvas share ${share} is not in (0, 1]`);
  if (share === 1) return;
  await page.addInitScript((share) => {
    const style = document.createElement("style");
    style.textContent = `.viewer-shell .canvas-frame { width: ${share * 100}%; height: ${share * 100}%; }`;
    const install = () => document.head.append(style);
    if (document.head) install();
    else document.addEventListener("DOMContentLoaded", install, { once: true });
  }, share);
}

export async function openGallery(
  scenario: BrowserEnvironmentContext,
  options: GalleryServerOptions & {
    readonly initialPage?: "gui";
    /** Share of the canvas frame per axis; `GALLERY_CANVAS_SHARE` by default. */
    readonly canvasShare?: number;
    /** Alternate maintained application/observer fixtures for diagnostic products. */
    readonly entryPath?: string;
    readonly helperPath?: string;
  } = {},
) {
  const { page } = scenario;
  const {
    initialPage,
    canvasShare = GALLERY_CANVAS_SHARE,
    entryPath = "/examples/world-gallery/index.html",
    helperPath = "/target/gallery-fixtures/viewer-browser-helper.js",
    ...server
  } = options;
  await confineGalleryCanvas(page, canvasShare);
  const origin = await startGalleryServer(process.cwd(), scenario, server);
  const helper = `${origin}${helperPath}`;
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  // Image measurements are evidence: their values show the margin of each
  // pixel threshold at the canvas size the scenario rendered.
  const measured = /^(compare|countViewerColors$|captureColorRegion$)/;
  const call = async <T>(name: string, ...args: unknown[]) => {
    const result = await invoke<T>(page, helper, name, args);
    if (measured.test(name))
      await scenario.evidence.record("image-measurement", {
        name,
        args,
        result,
      });
    return result;
  };
  const inspect = () => call<Inspection>("settleGalleryInput");
  // Wait for pending picks to answer. Callers then pass another ingress barrier
  // (`inspect` includes one) so input and React commits the selection change
  // caused are admitted.
  const settleSelection = async () => {
    await call("awaitGalleryIngress");
    await page.waitForFunction(
      () =>
        !document.querySelector("#selection-status") ||
        document.querySelector<HTMLElement>("#selection-status")!.dataset
          .pending === "0",
    );
  };
  const settle = async () => {
    await call("waitForGallerySceneReady");
    await settleSelection();
    return inspect();
  };
  const selectScene = async (
    name: "shapes" | "lighting" | "particles" | "gui" | "platformer",
  ) => {
    await page.locator("#scene-picker-trigger").click();
    await page.locator(`#world-${name}`).click();
  };
  const navigate = async (
    name: "shapes" | "lighting" | "particles" | "gui" | "platformer",
  ) => {
    await selectScene(name);
    const state = await page.waitForFunction((name) => {
      if (
        document.querySelector<HTMLElement>(".viewer-shell")?.dataset.page !==
        name
      )
        return false;
      const status = document.querySelector<HTMLOutputElement>("#status");
      return status?.dataset.state === "ready" ||
        status?.dataset.state === "error"
        ? {
            state: status.dataset.state,
            message: status.textContent?.trim(),
          }
        : false;
    }, name);
    const result = (await state.jsonValue()) as {
      state: "ready" | "error";
      message?: string;
    };
    if (result.state === "error")
      throw new Error(result.message || `${name} failed to start`);
  };
  // Async observations need awaited polling; Playwright treats a Promise predicate as truthy.
  const waitFor = async (predicate: (inspection: Inspection) => boolean) => {
    const deadline = performance.now() + galleryEnvironment.operationTimeoutMs;
    for (;;) {
      const inspection = await inspect();
      if (predicate(inspection)) return inspection;
      if (performance.now() > deadline) {
        await scenario.evidence.record("gallery-state-timeout", {
          input: await page.evaluate(() => ({
            target:
              document.querySelector<HTMLSelectElement>("#animation-target")
                ?.value,
            seek: document.querySelector<HTMLInputElement>("#animation-seek")
              ?.value,
            status: document.querySelector("#status")?.textContent,
          })),
          controllers: inspection.controllers,
          tick: inspection.tick,
        });
        throw new Error("Gallery state did not settle");
      }
    }
  };
  const seek = async (time: number, target = "all") => {
    await page.locator("#animation-target").selectOption(target);
    const slider = page.locator("#animation-seek");
    const pausedAt = (time: number) => (inspection: Inspection) => {
      const players =
        inspection.controllers?.filter(
          (player) =>
            target === "all" ||
            player.description.drivers.some(
              (driver) =>
                inspection.entities.find(
                  (entity) => entity.id === driver.target,
                )?.metadata.symbolicId === target,
            ),
        ) ?? [];
      return (
        players.length > 0 &&
        players.every(
          (player) =>
            player.state === "paused" && Math.abs(player.time - time) < 0.001,
        )
      );
    };
    // Read and change the live input in one browser turn: playback can update it
    // between Playwright calls. The native setter preserves React's change detection.
    const dispatch = (requested: number) =>
      slider.evaluate((element, requested) => {
        const input = element as HTMLInputElement;
        const next =
          input.valueAsNumber === requested
            ? requested +
              (requested >= Number(input.max) ? -1 : 1) * Number(input.step)
            : requested;
        const setter = Object.getOwnPropertyDescriptor(
          HTMLInputElement.prototype,
          "value",
        )!.set!;
        setter.call(input, String(next));
        const sampled = input.valueAsNumber;
        input.dispatchEvent(new Event("input", { bubbles: true }));
        return sampled;
      }, requested);
    const first = await dispatch(time);
    await waitFor(pausedAt(first));
    if (first !== time) {
      await page.waitForFunction(
        (expected) =>
          Math.abs(
            Number(
              document.querySelector<HTMLInputElement>("#animation-seek")
                ?.value,
            ) - expected,
          ) < 0.001,
        first,
      );
      assert.equal(await dispatch(time), time);
    }
    return waitFor(pausedAt(time));
  };
  const capture = async (label: string, waitForResources = true) => {
    // The capture's own inspection, after flushing React, is the read barrier
    // and the inspected state; settling needs only the ingress barriers.
    await settleSelection();
    await call("awaitGalleryIngress");
    const frame = await call<ViewerBrowserCapture>(
      "captureViewer",
      label,
      waitForResources,
    );
    // Pixels and the complete World inspection are artifacts; the bounded
    // event log keeps only the frame and image summary so later records fit.
    const { dataUrl, inspection: _inspection, ...observation } = frame;
    const { dataUrl: _dataUrl, ...metadata } = frame;
    await Promise.all([
      writeDataUrl(join(scenario.evidence.directory, `${label}.png`), dataUrl),
      writeFile(
        join(scenario.evidence.directory, `${label}.json`),
        `${JSON.stringify(metadata, bigintJson, 2)}\n`,
      ),
    ]);
    await scenario.evidence.record(label, {
      ...observation,
      artifacts: [`${label}.png`, `${label}.json`],
    });
    assert.deepEqual(errors, []);
    return frame;
  };
  const locate = async (id: string) => {
    await page.locator("#ipp-world-canvas").scrollIntoViewIfNeeded();
    return call<Awaited<ReturnType<typeof locateGalleryObject>>>(
      "locateGalleryObject",
      id,
    );
  };
  const click = async (id: string) => {
    const point = await locate(id);
    await page.mouse.click(point.clientX, point.clientY);
    await settle();
    await page.waitForFunction(
      (id) =>
        document.querySelector<HTMLElement>("#selection-status")?.dataset
          .selected === id,
      id,
    );
    return inspect();
  };
  const drag = async (
    from: readonly [number, number],
    to: readonly [number, number],
    button: "left" | "middle" = "left",
  ) => {
    await page.mouse.move(...from);
    await page.mouse.down({ button });
    await page.mouse.move(...to, { steps: 6 });
    await page.mouse.up({ button });
  };
  const held = async () => {
    const deadline = performance.now() + galleryEnvironment.operationTimeoutMs;
    while (!(await call<boolean>("queryReplyHeld"))) {
      await inspect();
      if (performance.now() > deadline) {
        await scenario.evidence.record("held-reply-timeout", {
          status: await page.locator("#status").textContent(),
          outcome: await call("heldReplyOutcome"),
        });
        throw new Error("The held reply did not arrive");
      }
    }
  };
  await page.goto(
    `${origin}${entryPath}${initialPage ? `#${initialPage}` : ""}`,
  );
  await call("waitForViewer");
  return {
    page,
    origin,
    helper,
    call,
    inspect,
    settle,
    selectScene,
    navigate,
    seek,
    waitFor,
    capture,
    capturePending: (label: string) => capture(label, false),
    locate,
    click,
    drag,
    held,
    errors,
    difference: (a: string, b: string) =>
      call<ImageDifference>("compareViewerCaptures", a, b),
  };
}

export function entity(inspection: Inspection, id: string): EntitySnapshot {
  const entity = inspection.entities.find(
    (entity) => entity.metadata.symbolicId === id,
  );
  assert.ok(entity, `Missing ${id}`);
  return entity;
}
export function transform(inspection: Inspection, id = "gallery-camera") {
  return entity(inspection, id).components.find(
    (entry) => "qx" in entry.fields,
  )!.fields;
}
export function position(inspection: Inspection, id: string) {
  const t = transform(inspection, id);
  return [Number(t.x), Number(t.y), Number(t.z)];
}
export function selected(inspection: Inspection) {
  return inspection.entities
    .filter((entity) =>
      entity.components.some(
        (entry) =>
          entry.fields.is_rendered === true && "outline" in entry.fields,
      ),
    )
    .map((entity) => entity.metadata.symbolicId);
}
