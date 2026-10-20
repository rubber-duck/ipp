import assert from "node:assert/strict";
import { join, resolve } from "node:path";
import type { BrowserEnvironmentContext } from "../../harness/browser.js";
import { invoke } from "../../harness/page-calls.js";
import { writeDataUrl } from "../../harness/evidence.js";
import type * as Fixture from "../pages/application.js";
import {
  cockpitFields,
  surfaceSample,
  cameraCoordinates,
} from "../cockpit-oracle.js";

export const applicationEnvironment = {
  workspace: resolve(process.cwd()),
  build: {
    name: "render" as const,
    generatedModule: resolve("target/browser-build/render/generated.js"),
    runtimeWasm: resolve("target/browser-build/render/runtime.wasm"),
    contractArtifact: resolve("target/browser-build/render/contract.bin"),
  },
  operationTimeoutMs: 30_000,
  closeTimeoutMs: 5_000,
  evidenceParent: resolve("target/integration-artifacts/demo-application"),
};
export async function openApplication(
  environment: BrowserEnvironmentContext,
  options: { entryPath?: string; helperPath?: string } = {},
) {
  const page = environment.page;
  const helper = `${environment.urls.origin}${options.helperPath ?? "/target/gallery-fixtures/application-browser-helper.js"}`;
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto(
    `${environment.urls.origin}${options.entryPath ?? "/examples/demos/index.html"}`,
  );
  await page.waitForFunction(
    () => !!window.ippDemoApplication && !!window.ippDemoCanvas,
  );
  const call = <T>(name: string, ...args: unknown[]) =>
    invoke<T>(page, helper, name, args);
  const settle = () => call("settle");
  await settle();
  const state = () => call<Awaited<ReturnType<typeof Fixture.state>>>("state");
  const waitForOption = async (name: string, value: unknown) => {
    const deadline = performance.now() + 30_000;
    for (;;) {
      const current = await state();
      if (current.options[name] === value && !current.preparing) return current;
      if (performance.now() >= deadline) {
        await environment.evidence.record(
          "application-option-timeout",
          current,
        );
        throw new Error(`Native GUI did not select ${name}=${String(value)}`);
      }
    }
  };
  const inspect = () =>
    call<Awaited<ReturnType<typeof Fixture.inspect>>>("inspect");
  const waitFor = async (
    predicate: (value: Awaited<ReturnType<typeof Fixture.inspect>>) => boolean,
  ) => {
    const deadline = performance.now() + 30_000;
    while (performance.now() < deadline) {
      const current = await inspect();
      if (predicate(current)) return current;
    }
    throw new Error("The active demo state did not settle");
  };
  const coordinates = async (id: string) => {
    const point = await call<Awaited<ReturnType<typeof Fixture.point>>>(
      "point",
      id,
    );
    const bounds = await page.locator("#ipp-demo-canvas").boundingBox();
    assert.ok(bounds);
    return {
      x: bounds.x + point.x * bounds.width,
      y: bounds.y + point.y * bounds.height,
    };
  };
  const press = async (id: string) => {
    const point = await coordinates(id);
    await page.mouse.click(point.x, point.y);
    await settle();
  };
  const cockpitCoordinates = async (
    index: number,
    id: string,
    fraction = 0.5,
    verticalFraction = 0.5,
  ) => {
    const snapshot =
      await call<Awaited<ReturnType<typeof Fixture.cockpitGeometry>>>(
        "cockpitGeometry",
      );
    const panel = snapshot.panels[index]!;
    const header = id.startsWith("cockpit-header-");
    const rect = cockpitFields(
      panel,
      header ? `cockpit-grip-${index}` : id,
      snapshot.componentIds.CanvasBounds!,
    );
    const extent = panel.canvas?.state.extent;
    assert.ok(extent, "The pane has evaluated Canvas dimensions");
    const targetHeight = header
      ? cockpitFields(panel, id, snapshot.componentIds.GuiLayout!).height
      : rect.height;
    const point = surfaceSample(
      snapshot.scene,
      snapshot.componentIds,
      index,
      (Number(rect.x) + Number(rect.width) * fraction) / extent[0],
      (Number(rect.y) + Number(targetHeight) * verticalFraction) / extent[1],
    ).position;
    const camera = cockpitFields(
      snapshot.scene,
      "cockpit-camera",
      snapshot.componentIds.Transform!,
    );
    const projection = cockpitFields(
      snapshot.scene,
      "cockpit-camera",
      snapshot.componentIds.Camera!,
    );
    const local = cameraCoordinates(point, camera);
    const halfHeight = Math.tan(Number(projection.fov_y) / 2) * -local[2]!;
    assert.ok(halfHeight > 0, `${id} is ahead of the Camera`);
    const bounds = await page.locator("#ipp-demo-canvas").boundingBox();
    assert.ok(bounds);
    const surface = snapshot.cameraSurface;
    const scaleX =
      (bounds.width * snapshot.viewport.devicePixelRatio) /
      snapshot.viewport.width;
    const scaleY =
      (bounds.height * snapshot.viewport.devicePixelRatio) /
      snapshot.viewport.height;
    return {
      x:
        bounds.x +
        scaleX *
          (surface.x +
            surface.width *
              (0.5 +
                local[0]! /
                  ((2 * halfHeight * surface.width) / surface.height))),
      y:
        bounds.y +
        scaleY *
          (surface.y + surface.height * (0.5 - local[1]! / (2 * halfHeight))),
    };
  };
  const capture = async (label: string) => {
    const frame = await call<Awaited<ReturnType<typeof Fixture.capture>>>(
      "capture",
      label,
    );
    await writeDataUrl(
      join(environment.evidence.directory, `${label}.png`),
      frame.dataUrl,
    );
    return frame;
  };
  const action = (name: string, value?: unknown) => call("action", name, value);
  return {
    page,
    errors,
    call,
    settle,
    state,
    waitForOption,
    inspect,
    waitFor,
    coordinates,
    cockpitCoordinates,
    press,
    capture,
    action,
    navigate: (scene: string) => action("scene", scene),
    difference: (before: string, after: string) =>
      call<ReturnType<typeof Fixture.difference>>("difference", before, after),
    drag: async (
      from: readonly [number, number],
      to: readonly [number, number],
    ) => {
      await page.mouse.move(...from);
      await page.mouse.down();
      await page.mouse.move(...to, { steps: 4 });
      await page.mouse.up();
      await settle();
    },
  };
}
