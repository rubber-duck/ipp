import assert from "node:assert/strict";
import { writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import type { Inspection, EntitySnapshot } from "@ipp/client";
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

export const galleryBuild = (name: "render-expanded" | "headless") => {
  const directory = resolve("target/browser-build", name);
  return {
    name,
    generatedModule: join(directory, "generated.js"),
    runtimeWasm: join(directory, "runtime.wasm"),
    exportWasm: join(directory, "export.wasm"),
    contractArtifact: join(directory, "contract.bin"),
  };
};
export const galleryEnvironment = {
  workspace: resolve(process.cwd()),
  build: galleryBuild("render-expanded"),
  mismatchBuild: galleryBuild("headless"),
  operationTimeoutMs: 15_000,
  closeTimeoutMs: 5_000,
  evidenceParent: resolve("target/integration-artifacts/gallery-combined"),
};

export async function openGallery(
  scenario: BrowserEnvironmentContext,
  options: GalleryServerOptions & {
    readonly initialPage?: "gui";
  } = {},
) {
  const { page } = scenario;
  const { initialPage, ...server } = options;
  const origin = await startGalleryServer(process.cwd(), scenario, server);
  const helper = `${origin}/target/gallery-fixtures/viewer-browser-helper.js`;
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const call = <T>(name: string, ...args: unknown[]) =>
    invoke<T>(page, helper, name, args);
  const inspect = () => call<Inspection>("settleGalleryInput");
  const settle = async () => {
    await inspect();
    await page.waitForFunction(
      () =>
        !document.querySelector("#selection-status") ||
        document.querySelector<HTMLElement>("#selection-status")!.dataset
          .pending === "0",
    );
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
    await settle();
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
      if (performance.now() > deadline)
        throw new Error("The held reply did not arrive");
    }
  };
  await page.goto(
    `${origin}/examples/world-gallery/index.html${initialPage ? `#${initialPage}` : ""}`,
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
  return entity(inspection, id).effective.find((entry) => "qx" in entry.fields)!
    .fields;
}
export function position(inspection: Inspection, id: string) {
  const t = transform(inspection, id);
  return [Number(t.x), Number(t.y), Number(t.z)];
}
export function selected(inspection: Inspection) {
  return inspection.entities
    .filter((entity) =>
      entity.effective.some(
        (entry) =>
          entry.fields.is_rendered === true && "outline" in entry.fields,
      ),
    )
    .map((entity) => entity.metadata.symbolicId);
}
