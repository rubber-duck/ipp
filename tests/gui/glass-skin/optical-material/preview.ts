import { workerTransport } from "../../../../packages/ipp-client/src/worker.js";
import { openOpticalComponents } from "./component-session.js";
import { COMPONENT_EXTENT } from "./component-scene.js";
import { CanvasGuiInput } from "../../../../packages/ipp-react/src/gui/platform/input.js";
import { canvasViewport } from "../../../../packages/ipp-react/src/canvas/presentation.js";
import { canvasOutput } from "@ipp/client";
import { contextScene, type ContextOptions } from "./context-scene.js";

async function start() {
  const moduleUrl = "/target/browser-build/render/generated.js";
  const contract = await import(moduleUrl);
  const canvas = document.querySelector("canvas")!;
  const status = document.querySelector<HTMLOutputElement>("#status")!;
  const host = await contract.IppHostClient.connectTransport(
    workerTransport(
      "/target/gui-glass-skin/worker.js",
      "/target/browser-build/render/runtime.wasm",
      contract.MAX_MESSAGE_BYTES,
      { canvas: canvas.transferControlToOffscreen() },
    ),
  );
  const font = new Uint8Array(
    await (
      await fetch("/target/font-assets/shure-tech-mono.ippf")
    ).arrayBuffer(),
  );
  const session = await openOpticalComponents(
    host,
    font,
    contract,
    "glass-optical-preview",
    new URLSearchParams(location.search).get("skin") === "liquid"
      ? "liquid"
      : new URLSearchParams(location.search).get("skin") === "production",
  );
  const surface = await host.presentation.surface();
  const viewport = () => {
    const { width, height } = canvas.getBoundingClientRect();
    return canvasViewport(
      { width, height, devicePixelRatio: window.devicePixelRatio },
      surface,
    );
  };
  session.binding = await host.setRootOutput(
    session.binding.output,
    viewport(),
  );
  let presentation = await host.presentation.select(surface, session.binding);
  const input = new CanvasGuiInput(canvas, host.input, {
    onError: (error) => {
      status.textContent = String(error);
    },
  });
  await input.select(presentation);
  let resizing = Promise.resolve();
  const resize = () => {
    resizing = resizing
      .then(async () => {
        if (canvas.clientWidth <= 0 || canvas.clientHeight <= 0) return;
        const next = viewport();
        const current = presentation.binding.viewport;
        if (
          next.width === current.width &&
          next.height === current.height &&
          next.devicePixelRatio === current.devicePixelRatio
        )
          return;
        presentation = await host.presentation.resize(presentation, next);
        session.binding = presentation.binding;
        await input.select(presentation);
      })
      .catch((error) => {
        status.textContent = String(error);
        console.error(error);
      });
  };
  const observer = new ResizeObserver(resize);
  observer.observe(canvas);
  // A monitor/zoom change can change density without changing the CSS box.
  let density = matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
  const densityChanged = () => {
    density.removeEventListener("change", densityChanged);
    density = matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
    density.addEventListener("change", densityChanged);
    resize();
  };
  density.addEventListener("change", densityChanged);
  window.addEventListener("resize", resize);
  const view = document.querySelector<HTMLInputElement>("#view")!;
  const light = document.querySelector<HTMLInputElement>("#light")!;
  const shape = document.querySelector<HTMLSelectElement>("#shape")!;
  const animate = document.querySelector<HTMLInputElement>("#animate")!;
  animate.checked = !matchMedia("(prefers-reduced-motion: reduce)").matches;
  const transform = session.sceneClient.components.Transform!;
  let pending = Promise.resolve();
  const update = () => {
    const options: ContextOptions = {
      extent: COMPONENT_EXTENT,
      angle: (Number(view.value) * Math.PI) / 180,
      light: animate.checked ? -3 : Number(light.value),
      shape: shape.value as NonNullable<ContextOptions["shape"]>,
      ...(animate.checked
        ? {
            motion: {
              component: transform.id,
              offset: transform.fields.x!.offset,
            },
          }
        : {}),
    };
    pending = pending
      .then(() =>
        session.sceneRoot.render(
          contextScene(session.canvas.reference, options),
        ),
      )
      .catch((error) => {
        status.textContent = String(error);
      });
  };
  view.addEventListener("input", update);
  shape.addEventListener("change", update);
  animate.addEventListener("change", update);
  light.addEventListener("input", () => {
    animate.checked = false;
    update();
  });
  update();
  await pending;
  status.textContent =
    "Live components · hover, click, type and drag · move the view or light";
  // Inspection of the actual preview, including its worker-rendered pixels.
  window.glassPreview = {
    session,
    capture: async () => {
      await resizing;
      await pending;
      return host.presentation.capture(presentation, {
        afterOutputs: [
          session.binding.output,
          canvasOutput(session.canvas.reference),
        ],
      });
    },
    close: async () => {
      observer.disconnect();
      density.removeEventListener("change", densityChanged);
      window.removeEventListener("resize", resize);
      await resizing;
      await pending;
      await input.close();
      await host.close();
    },
  };
  window.glassPreviewReady = true;
}

declare global {
  interface Window {
    glassPreviewReady?: boolean;
    glassPreview: {
      session: Awaited<ReturnType<typeof openOpticalComponents>>;
      capture(): Promise<import("@ipp/client").PresentedCapture>;
      close(): Promise<void>;
    };
  }
}

start().catch((error) => {
  document.querySelector("#status")!.textContent = String(error);
  console.error(error);
});
