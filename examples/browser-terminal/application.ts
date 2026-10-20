import {
  canvasOutput,
  type GuiWorldClient,
  type HostClientBase,
} from "@ipp/client";
import { startBrowserTerminal } from "@ipp/terminal";
import { CanvasGuiInput } from "../../packages/ipp-react/src/gui/platform/input.js";
import {
  apply,
  component,
  alias,
  handle,
} from "../../packages/ipp-terminal/src/commands.js";

export async function mountTerminal(
  canvas: HTMLCanvasElement,
  baseUrl: string,
  onError: (error: Error) => void,
) {
  const url = (path: string) => new URL(path, baseUrl).href;
  const generatedModuleUrl = url("runtime/generated.js");
  const contract = (await import(generatedModuleUrl)) as {
    IppHostClient: {
      connectWorker(
        worker: string,
        wasm: string,
        options: object,
      ): Promise<HostClientBase<GuiWorldClient>>;
    };
  };
  const width = canvas.width,
    height = canvas.height;
  const host = await contract.IppHostClient.connectWorker(
    url("runtime/wasm-worker.js"),
    url("runtime/runtime.wasm"),
    {
      canvas: canvas.transferControlToOffscreen(),
      timeoutMs: 20000,
      logLevel: "warn",
    },
  );
  let input: CanvasGuiInput | undefined;
  let terminal: ReturnType<typeof startBrowserTerminal> | undefined;
  try {
    const created = await host.createWorld({
      temporary: true,
      selectedSystems: [
        "ipp.asset-dependencies",
        "ipp.lifecycle-publisher",
        "ipp.gui",
        "ipp.gui-layout",
        "ipp.canvas",
      ],
    });
    const client = await host.openWorld(created.reference);
    client.sendCommand({
      type: "CanvasStateUpdateCommand",
      extent: [width, height],
      unitsPerMetre: 1,
    });
    const result = await apply(client, [
      {
        kind: "create",
        alias: 0,
        metadata: { symbolicId: "terminal", classes: [] },
      },
      component(client, "GuiInputTarget", alias(0), {
        capture_tab: true,
        capture_escape: true,
      }),
      component(client, "GuiLayout", alias(0), { width, height, clip: true }),
      component(
        client,
        "GuiBehavior",
        alias(0),
        { semantic_label: "BashKit terminal" },
        true,
      ),
    ]);
    const target = result.aliases[0]!.id;
    const binding = await host.setRootOutput(canvasOutput(created.reference), {
      width,
      height,
      devicePixelRatio: 1,
    });
    const surface = await host.presentation.surface();
    const view = await host.presentation.select(surface, binding);
    input = new CanvasGuiInput(canvas, host.input, { onError });
    await input.select(view);
    terminal = startBrowserTerminal(
      host,
      url("terminal-worker.js"),
      generatedModuleUrl,
      {
        world: created.reference,
        target,
        fontSource: url("font.ippf"),
        fontMetricsUrl: url("terminal-font.json"),
        fontSize: 18,
        lineHeight: 24,
        wasm: url("ghostty-vt.wasm"),
        callbacksWasm: url("ghostty-callbacks.wasm"),
        bashkit: url("bashkit.wasm"),
        shell: {
          files: {
            "/home/pilot/readme.txt":
              "IPP browser terminal\nGhostty parses the terminal. IPP draws it.\nTry: cat readme.txt, ls, printf, vi notes.txt, or exit.\n",
          },
          cwd: "/home/pilot",
        },
      },
      onError,
    );
    await terminal.ready;
    await host.presentation.frame(view);
    return {
      host,
      client,
      target,
      terminal,
      view,
      async resize(width: number, height: number) {
        await apply(client, [
          component(
            client,
            "GuiLayout",
            handle(target),
            { width, height },
            true,
          ),
        ]);
      },
      async close() {
        const errors: unknown[] = [];
        for (const close of [
          () => input!.close(),
          () => terminal!.close(),
          () => host.close(),
        ]) {
          try {
            await close();
          } catch (error) {
            errors.push(error);
          }
        }
        if (errors.length)
          throw new AggregateError(
            errors,
            "Terminal application cleanup failed",
          );
      },
    };
  } catch (error) {
    await Promise.allSettled([input?.close(), terminal?.close()]);
    await host.close().catch(() => {});
    throw error;
  }
}
