import { build } from "esbuild";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { cp, mkdir, writeFile } from "node:fs/promises";
import { resolve, join } from "node:path";
import { pathToFileURL } from "node:url";
import { setTimeout } from "node:timers/promises";
import { canvasOutput, registerNativeHtml } from "@ipp/client";
import { NativeWindowChannel } from "../../../tools/shared-host/window.js";
import { NativeHtmlSources } from "../../../tools/shared-host/html.js";
import { encodePng } from "../../../tools/shared-host/png.js";
import {
  createEntity,
  insertComponent,
  componentFields,
  aliasId,
  successfulBatch,
} from "../../fixtures/commands.js";
import {
  GUI,
  LIFECYCLE,
  selectSystems,
} from "../../fixtures/system-selections.js";

export async function withNativeCef(
  evidence: string,
  exercise: (driver: any) => Promise<void>,
) {
  const pages = join(evidence, "pages");
  await mkdir(pages, { recursive: true });
  await cp(resolve("tests/rendering/fixtures/native-cef"), pages, {
    recursive: true,
  });
  await build({
    entryPoints: [
      resolve("examples/demos/scenes/cockpit/browser-pages/page.js"),
    ],
    outfile: join(pages, "page.js"),
    bundle: true,
    format: "iife",
    platform: "browser",
  });
  const product = resolve("target/gles-host-instrumentation");
  const process = spawn(
    join(product, "gles_host"),
    [
      "--egl-dir",
      globalThis.process.env.IPP_EGL_LIBRARY_DIR!,
      "--cef",
      resolve("target/cef-engine"),
      pages,
    ],
    { stdio: ["ignore", "pipe", "pipe"] },
  );
  const exited = once(process, "exit");
  let log = "";
  process.stdout.on("data", (bytes) => {
    log += bytes;
  });
  process.stderr.on("data", (bytes) => {
    log += bytes;
  });
  let host: any,
    client: any,
    channel: NativeWindowChannel | undefined,
    adapter: NativeHtmlSources | undefined,
    source: any,
    physical: any,
    subscription: any,
    watch: any;
  const failures: string[] = [];
  const navigations: string[] = [];
  try {
    const deadline = Date.now() + 20_000;
    while (!log.includes('"ready"')) {
      if (process.exitCode !== null || Date.now() > deadline)
        throw new Error(log);
      await setTimeout(20);
    }
    const ready = JSON.parse(
      log.split("\n").find((line) => line.includes('"ready"'))!,
    );
    const generated = await import(
      pathToFileURL(join(product, "generated.js")).href
    );
    host = await generated.IppHostClient.connectWebSocket(ready.url);
    channel = await NativeWindowChannel.open(ready.presentation);
    adapter = new NativeHtmlSources(host, channel);
    const world = await host.createWorld({
      selectedSystems: selectSystems(GUI, LIFECYCLE),
      canvas: { extent: [192, 128], unitsPerMetre: 1 },
    });
    client = await host.openWorld(world.reference);
    const register = (page: string) =>
      registerNativeHtml(
        host,
        page,
        (error) => failures.push(error.message),
        (url) => navigations.push(url),
      );
    source = await register("page.html");
    const outcome = successfulBatch(
      await client.batch([
        createEntity(1, "background"),
        insertComponent(
          client,
          "CanvasBox",
          { kind: "alias", alias: 1 },
          { width: 192, height: 128 },
        ),
        insertComponent(
          client,
          "CanvasStyle",
          { kind: "alias", alias: 1 },
          { red: 0, green: 0, blue: 1 },
        ),
        createEntity(2, "html"),
        insertComponent(
          client,
          "GuiInputTarget",
          { kind: "alias", alias: 2 },
          { capture_tab: true, capture_escape: true },
        ),
        insertComponent(
          client,
          "GuiLayout",
          { kind: "alias", alias: 2 },
          { width: 192, height: 128 },
        ),
        insertComponent(
          client,
          "CanvasHtml",
          { kind: "alias", alias: 2 },
          { source: source.source, width: 192, height: 128 },
        ),
      ]),
    );
    const entity = aliasId(outcome, 2);
    const binding = await host.setRootOutput(canvasOutput(world.reference), {
      width: 192,
      height: 128,
      devicePixelRatio: 1,
    });
    let view = await host.presentation.select(
      await host.presentation.surface(),
      binding,
    );
    const fields = async (values: Record<string, string | number>) =>
      successfulBatch(
        await client.batch(
          componentFields(client, "CanvasHtml", values).map((field) => ({
            kind: "setField",
            entity: { kind: "handle", id: entity },
            component: client.components.CanvasHtml.id,
            field,
          })),
        ),
      );
    watch = await client.watchLifecycle(
      [
        {
          target: {
            kind: "component",
            entity,
            component: client.components.GuiInputTarget.id,
          },
          kinds: 8 | 16,
        },
      ],
      () => {},
    );
    const target = {
      world: world.reference,
      entity,
      component: client.components.GuiInputTarget.id,
      incarnation: watch.baselines[0].lifetime.incarnation,
    };
    physical = await host.input.open(view);
    // Source input adapter is the same sink used by BrowserPage after the GUI
    // target's exclusive effect subscription. The cockpit scenario covers routing.
    let input = source.bindInput(target);
    subscription = await client.subscribeGuiEffects(
      (event: any) => {
        if (event.effect.kind === "input") input.input(event.effect.input);
      },
      { inputTarget: target },
    );
    await exercise({
      async allocations() {
        const { hostProfiling } = await import(
          pathToFileURL(join(product, "profiling.js")).href
        );
        const profiler = hostProfiling(host);
        // Producer loading and initial GPU preparation finish before the steady window.
        await setTimeout(500);
        const id = await profiler.start({ counters: true, gpu: "off" });
        try {
          await setTimeout(600);
          const capture = await profiler.stop();
          await writeFile(
            join(evidence, "steady-frame-allocations.json"),
            JSON.stringify(capture),
          );
          return capture;
        } finally {
          await profiler.release(id);
        }
      },
      async capture() {
        const frame = await host.presentation.capture(view);
        return {
          width: 192,
          height: 128,
          pixels: new Uint8Array(frame.pixels),
        };
      },
      async save(label: string, frame: any) {
        await writeFile(join(evidence, `${label}.png`), encodePng(frame));
      },
      async navigations() {
        return navigations;
      },
      async errors() {
        return failures;
      },
      async click(x: number, y: number) {
        for (const kind of ["pointerDown", "pointerUp"] as const)
          await physical.send({
            kind,
            button: "primary",
            pointer: 1n,
            point: [x / 192, y / 128],
          });
        await setTimeout(50);
      },
      async text(text: string) {
        if (!physical.nativeText?.clientInput)
          throw new Error("CEF GUI target did not acquire native text focus");
        await physical.send({
          kind: "clientInput",
          fence: physical.nativeText.fence,
          input: { kind: "text", text },
        });
        await setTimeout(50);
      },
      async replace(page: string) {
        const previous = source;
        source = await register(page);
        await fields({ source: source.source });
        previous.release();
        input = source.bindInput(target);
      },
      async resize(width: number, height: number) {
        await fields({ width, height });
        successfulBatch(
          await client.batch(
            componentFields(client, "GuiLayout", { width, height }).map(
              (field) => ({
                kind: "setField",
                entity: { kind: "handle", id: entity },
                component: client.components.GuiLayout.id,
                field,
              }),
            ),
          ),
        );
      },
      async recover() {
        // These testing controls are already available on the same exclusive
        // diagnostics socket; production clients cannot advance Host clocks.
        channel!.testContextLoss();
        const old = view.surface.context,
          end = Date.now() + 10_000;
        for (;;) {
          const surface = await host.presentation
            .surface()
            .catch(() => undefined);
          if (surface && surface.context !== old) {
            await physical.close();
            view = await host.presentation.select(surface, binding);
            physical = await host.input.open(view);
            break;
          }
          if (Date.now() > end)
            throw new Error("Context restoration timed out");
          await setTimeout(20);
        }
      },
      async reconnect() {
        adapter!.close();
        channel!.close();
        channel = await NativeWindowChannel.open(ready.presentation);
        adapter = new NativeHtmlSources(host, channel);
        source = await register("page.html");
        await fields({ source: source.source });
        input = source.bindInput(target);
      },
      async key(key: string) {
        await physical.send({
          kind: "clientInput",
          fence: physical.nativeText.fence,
          input: {
            kind: "key",
            key,
            code: key,
            modifiers: 0,
            pressed: true,
            repeat: false,
          },
        });
      },
      async withdraw() {
        source.release();
        await setTimeout(50);
      },
    });
    input.release();
    await subscription.unsubscribe();
    subscription = undefined;
    await physical.close();
    physical = undefined;
    await watch.remove();
    watch = undefined;
    await client.close();
    client = undefined;
    await host.destroyWorld(world.reference);
  } finally {
    source?.release();
    adapter?.close();
    await subscription?.unsubscribe().catch(() => {});
    await physical?.close().catch(() => {});
    await watch?.remove().catch(() => {});
    channel?.testShutdown();
    channel?.close();
    await client?.close().catch(() => {});
    await host?.close().catch(() => {});
    await Promise.race([exited, setTimeout(5000, undefined, { ref: false })]);
    if (process.exitCode === null && process.signalCode === null) {
      process.kill("SIGKILL");
      failures.push("CEF Host did not shut down cleanly");
    }
    await writeFile(join(evidence, "host.log"), log);
    if (process.exitCode !== 0)
      throw new Error(
        `CEF Host did not shut down cleanly: ${process.exitCode}/${process.signalCode}`,
      );
  }
}
