/** Browser adapter: surface, physical input, asset fetch and connection lifetime. */
import { useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  browserRuntime,
  canvasOutput,
  type ViewQueryTarget,
  type PickingWorldClient,
} from "@ipp/client";
import { IppCanvas, type IppCanvasHandle } from "@ipp/react/web";
import { createGuiUnhandledInputGate } from "@ipp/react/gui";
import { DemoApplication, DEMO_SYSTEMS } from "./application.js";
import { browserGalleryAssets } from "./browser-assets.js";
import { GALLERY_RUNTIME } from "./shared/runtime.js";
import { installCameraControls } from "./shared/camera-controls.js";
import {
  PLATFORMER_ASSETS,
  PLATFORMER_SOURCE,
} from "./scenes/platformer/scene-file.js";

const contract = await import(`${GALLERY_RUNTIME}generated.js`);
declare global {
  interface Window {
    ippDemoApplication?: DemoApplication;
    ippDemoCanvas?: IppCanvasHandle | undefined;
    ippDemoScene?: import("./shared/scene.js").GallerySceneMount | undefined;
  }
}

function BrowserDemos() {
  const assets = useMemo(
    () => browserGalleryAssets(new URL(window.location.href)),
    [],
  );
  const gate = useMemo(createGuiUnhandledInputGate, []);
  const applications = useMemo(
    () => new WeakMap<IppCanvasHandle, DemoApplication>(),
    [],
  );
  const scenePointers = useMemo(() => new Set<number>(), []);
  const [canvas, setCanvas] = useState<IppCanvasHandle>();
  const [application, setApplication] = useState<DemoApplication>();
  const [renderScale, setRenderScale] = useState(1);
  const [, changed] = useState(0);
  const [failure, setFailure] = useState<string>();
  useEffect(() => {
    if (!canvas) return;
    const lifetime = new AbortController();
    const app = new DemoApplication(
      {
        canvas,
        assets,
        contract,
        signal: lifetime.signal,
        onError: (error) => setFailure(error.message),
        renderScaleChanged: setRenderScale,
      },
      { scene: window.location.hash.slice(1) || "shapes" },
    );
    applications.set(canvas, app);
    setApplication(app);
    window.ippDemoApplication = app;
    const unsubscribe = app.subscribe(() => {
      window.ippDemoCanvas = app.sceneCanvas;
      window.ippDemoScene = app.scene;
      changed((value) => value + 1);
    });
    void app.ready.catch((error) => setFailure(String(error)));
    return () => {
      unsubscribe();
      lifetime.abort();
      delete window.ippDemoApplication;
      delete window.ippDemoCanvas;
      delete window.ippDemoScene;
      void app.dispose().catch(console.error);
    };
  }, [canvas, assets, applications]);
  useEffect(
    () => () => {
      const element =
        document.querySelector<HTMLCanvasElement>("#ipp-demo-canvas");
      for (const pointer of scenePointers)
        if (element?.hasPointerCapture(pointer))
          element.releasePointerCapture(pointer);
      scenePointers.clear();
    },
    [application?.sceneId, scenePointers],
  );
  useEffect(() => {
    if (
      !canvas ||
      !application ||
      ["charts", "fixed"].includes(application.navigationMode)
    )
      return;
    const element =
      document.querySelector<HTMLCanvasElement>("#ipp-demo-canvas");
    if (!element) return;
    const controls = installCameraControls(element, {
      client: canvas.client as PickingWorldClient,
      picking: application.pickingEnabled,
      pan: false,
      binding: () => canvas.view?.binding,
      cameraView: (binding) =>
        ({
          kind: "camera",
          root: { kind: "bound", binding },
          path: application.cameraPath,
        }) as ViewQueryTarget,
      navigate: (motion, binding) =>
        application.navigateCamera(motion, binding),
      flush: () => canvas.flush(),
      picked: (result, signal) => application.picked(result, signal),
      pending: () => {},
      error: (error) => setFailure(String(error)),
      admitPointer: (pointer, button, signal) =>
        gate.pointerDown(
          pointer,
          button === 0 ? "primary" : button === 1 ? "auxiliary" : "secondary",
          signal,
        ),
      admitScroll: (signal) => gate.scroll(signal),
    });
    return () => controls.dispose();
  }, [canvas, application, application?.sceneId, gate]);
  return (
    <IppCanvas
      id="stage"
      className="stage"
      canvasProps={{
        id: "ipp-demo-canvas",
        style: { width: "100%", height: "100%" },
      }}
      runtime={{
        ...browserRuntime(new URL(GALLERY_RUNTIME, window.location.href)),
        resourceUrls: [
          { prefix: PLATFORMER_SOURCE, baseUrl: assets.url(PLATFORMER_ASSETS) },
          {
            prefix: "https://cockpit.ipp.invalid/",
            baseUrl: assets.url("/target/demos-cockpit-assets/"),
          },
        ],
      }}
      world={{
        create: {
          selectedSystems: DEMO_SYSTEMS,
          canvas: { extent: [960, 640], unitsPerMetre: 1 },
        },
      }}
      output={
        canvas?.client.worldReference
          ? canvasOutput(canvas.client.worldReference)
          : null
      }
      renderScale={renderScale}
      guiInput={{
        unhandledInputGate: gate,
        wheelStep: 40,
        awaitPointerDown: true,
        onInputOutcome: (input, outcome) => {
          const pending = application?.handlePhysicalInput(input, outcome);
          const element =
            document.querySelector<HTMLCanvasElement>("#ipp-demo-canvas");
          if (
            input.kind === "pointerDown" &&
            application?.capturingSceneInput
          ) {
            element?.setPointerCapture(Number(input.pointer));
            scenePointers.add(Number(input.pointer));
          }
          if (
            (input.kind === "pointerUp" || input.kind === "pointerCancel") &&
            element?.hasPointerCapture(Number(input.pointer))
          ) {
            element.releasePointerCapture(Number(input.pointer));
            scenePointers.delete(Number(input.pointer));
          }
          return pending?.catch((error) => setFailure(String(error)));
        },
      }}
      onReady={setCanvas}
      beforeClose={(handle) => applications.get(handle)?.dispose()}
      onViewChange={(view) => {
        if (view)
          void application?.resize(view.binding.viewport).catch(console.error);
      }}
      onError={(error) => setFailure(error.message)}
      aria-label={failure ?? "IPP interactive demos"}
    />
  );
}
const mount = document.getElementById("app");
if (!mount) throw new Error("Missing application surface");
const root = createRoot(mount);
root.render(<BrowserDemos />);
window.addEventListener("pagehide", () => root.unmount(), { once: true });
