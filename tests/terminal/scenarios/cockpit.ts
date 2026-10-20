import assert from "node:assert/strict";
import type { Page } from "playwright";
import type {
  Client,
  RowsTable,
  WorldReference,
  HostClientBase,
  GuiWorldClient,
} from "@ipp/client";
interface CockpitWindow {
  terminalObserver: Client;
  ippDemoCanvas: {
    host: HostClientBase<GuiWorldClient>;
    capture(options: {
      afterOutputs: { kind: "canvas"; world: WorldReference }[];
    }): Promise<{ pixels: ArrayBuffer }>;
  };
  ippDemoApplication: { sceneClient: Client; inspect(): Promise<unknown> };
}
interface CockpitDriver {
  page: Page;
  navigate(scene: string): Promise<unknown>;
  action(name: string, value?: unknown): Promise<unknown>;
  call<T>(name: string, ...args: unknown[]): Promise<T>;
  cockpitCoordinates(
    panel: number,
    id: string,
  ): Promise<{ x: number; y: number }>;
  settle(): Promise<unknown>;
  capture(label: string): Promise<{ summary: { foregroundPixels: number } }>;
  difference(before: string, after: string): Promise<{ changedPixels: number }>;
  state(): Promise<{ worlds: readonly unknown[] }>;
}

export async function cockpitTerminalScenario(
  app: CockpitDriver,
  expectedGlyphs: number[],
  skin: "glass" | "cybertron" = "glass",
) {
  await app.navigate("cockpit");
  await app.action("skin", skin);
  await app.action("carousel", 2);
  const idle = await app.call<{
    panels: {
      world: WorldReference;
      entities: {
        metadata: { classes: string[] };
        components: { fields: Record<string, unknown> }[];
      }[];
    }[];
  }>("cockpitGeometry");
  assert.ok(
    !idle.panels[2]!.entities.some((entity) =>
      entity.metadata.classes.includes("terminal-content"),
    ),
  );
  const world = idle.panels[2]!.world;
  await app.page.evaluate(async (world) => {
    const state = window as unknown as CockpitWindow;
    state.terminalObserver = await state.ippDemoCanvas.host.openWorld(world);
  }, world);
  try {
    const press = async (id: string) => {
      const point = await app.cockpitCoordinates(2, id);
      await app.page.mouse.click(point.x, point.y);
      await app.settle();
    };
    // Completed controller evaluation establishes stable projected coordinates.
    await waitFor(
      async () =>
        app.page.evaluate(async () => {
          const scene = await (
            window as unknown as CockpitWindow
          ).ippDemoApplication.sceneClient!.inspectPage({
            collection: "controllers",
          });
          return scene.controllers?.every(
            (controller) => !controller.transition,
          );
        }),
      "carousel transition",
    );
    await press("cockpit-terminal-start");
    await waitFor(
      () =>
        app.page.evaluate(async () => {
          const state = await (
            window as unknown as CockpitWindow
          ).terminalObserver.inspectPage({
            collection: "entities",
            limit: 256,
          });
          return state.entities.some((entity) =>
            entity.components.some(
              (component) =>
                component.fields.text ===
                "Click to type · Ctrl+Shift+Esc releases focus",
            ),
          );
        }),
      "terminal startup",
    );
    await press("cockpit-terminal");
    await app.page.waitForFunction(
      () => document.activeElement?.tagName === "TEXTAREA",
    );
    const redPixels = () =>
      app.page.evaluate(async (world) => {
        const windowState = window as unknown as CockpitWindow;
        const frame = await windowState.ippDemoCanvas.capture({
          afterOutputs: [{ kind: "canvas", world }],
        });
        const pixels = new Uint8Array(frame.pixels);
        let red = 0;
        for (let i = 0; i < pixels.length; i += 4)
          if (
            pixels[i]! > 160 &&
            pixels[i]! > pixels[i + 1]! + 80 &&
            pixels[i]! > pixels[i + 2]! + 60
          )
            red++;
        return red;
      }, world);
    const beforeRed = await redPixels();
    await app.capture("terminal-before");
    await app.page.keyboard.type("printf ");
    // Several colored rows keep the pixel assertion meaningful at the panel's
    // projected text size, without pushing output below the navigation strip.
    await app.page.keyboard.insertText(
      "'\\033[31mCOCKPIT_READY\\nCOCKPIT_READY\\nCOCKPIT_READY\\033[0m\\n'",
    );
    await app.page.keyboard.press("Enter");
    await waitFor(
      () =>
        app.page.evaluate(async (expectedGlyphs) => {
          const client = (window as unknown as CockpitWindow).terminalObserver;
          const state = await client.inspectPage({
            collection: "entities",
            limit: 256,
          });
          return state.entities.some((entity) =>
            entity.components.some(
              (component) =>
                component.component === client.components.CanvasGlyphRun!.id &&
                JSON.stringify(
                  [...(component.fields.glyphs as RowsTable).rows.values()].map(
                    (row) => Number(row.glyph_id),
                  ),
                ) === JSON.stringify(expectedGlyphs),
            ),
          );
        }, expectedGlyphs),
      "terminal output",
    );
    // Curved Surfaces may retain their last texture until the configured refresh
    // interval. Observe completed frames until the actual terminal pixels arrive.
    let afterRed = await redPixels();
    const deadline = performance.now() + 15000;
    while (afterRed <= beforeRed + 20 && performance.now() < deadline)
      afterRed = await redPixels();
    const frame = await app.capture("cockpit-terminal");
    assert.ok(
      afterRed > beforeRed + 20,
      `Terminal red glyphs: ${beforeRed} -> ${afterRed}`,
    );
    assert.ok(
      (await app.difference("terminal-before", "cockpit-terminal"))
        .changedPixels > 50,
    );
    assert.ok(frame.summary.foregroundPixels > 1000);
    await app.page.keyboard.press("Control+Shift+Escape");
  } finally {
    await app.page.evaluate(() =>
      (window as unknown as CockpitWindow).terminalObserver.close(),
    );
  }
  await app.navigate("shapes");
  assert.equal((await app.state()).worlds.length, 2);
}

async function waitFor(
  read: () => Promise<boolean | undefined>,
  label: string,
) {
  const deadline = performance.now() + 90000;
  while (performance.now() < deadline) {
    if (await read()) return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  assert.fail(`Timed out waiting for ${label}`);
}
