import { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { browserRuntime, type OutputReference } from "@ipp/client";
import { IppCanvas, type IppCanvasHandle } from "@ipp/react/web";
import {
  BLENDER_SYSTEMS,
  BlenderAdapter,
  type AppliedRevision,
  type BlenderClient,
  type BlenderContract,
} from "../../integrations/blender/client/adapter.js";
import { createViewingCamera, requireBlenderClient } from "./camera.js";

export interface BlenderViewerHandle {
  canvas: IppCanvasHandle;
  client: BlenderClient;
  adapter: BlenderAdapter;
  output: OutputReference | null;
  presentation: Promise<void>;
  latest?: AppliedRevision;
  error?: string;
}

declare global {
  interface Window {
    ippBlender?: BlenderViewerHandle;
  }
}

const runtime = browserRuntime(new URL("./runtime/", import.meta.url));
const connection = new URLSearchParams(location.hash.slice(1));

function App() {
  const [generation, setGeneration] = useState(0);
  const [status, setStatus] = useState("Waiting for Blender");
  const [output, setOutput] = useState<OutputReference | null>(null);
  const [diagnostics, setDiagnostics] = useState<
    AppliedRevision["diagnostics"]
  >([]);
  const live = useRef<BlenderViewerHandle | undefined>(undefined);
  const startup = useRef(0);

  useEffect(
    () => () => {
      startup.current++;
      live.current?.adapter.close();
      delete window.ippBlender;
    },
    [],
  );

  async function ready(canvas: IppCanvasHandle, request: number) {
    live.current?.adapter.close();
    const endpoint = connection.get("endpoint");
    const token = connection.get("token");
    if (!endpoint || !token) {
      setStatus("Open this viewer from Blender's IPP panel");
      return;
    }
    const contract: BlenderContract = await import(runtime.generatedModuleUrl);
    if (startup.current !== request) return;
    const client = canvas.client;
    requireBlenderClient(client);
    const world = client.worldReference;
    if (!world) throw new Error("Blender viewer requires an explicit World");
    const fallback = await createViewingCamera(client);
    if (startup.current !== request) return;
    const select = async (camera: bigint, latest?: AppliedRevision) => {
      const selected = await canvas.host.bindOutput(world, camera, "camera");
      if (
        startup.current !== request ||
        live.current !== current ||
        (latest && current.latest !== latest)
      )
        return;
      current.output = selected;
      setOutput(selected);
    };
    const adapter = new BlenderAdapter(
      client,
      contract,
      new URL(endpoint),
      token,
      (latest) => {
        if (live.current?.adapter !== adapter) return;
        live.current.latest = latest;
        delete live.current.error;
        setStatus(`Connected · revision ${latest.revision}`);
        setDiagnostics(latest.diagnostics);
        current.presentation = select(
          latest.selectedCamera ?? fallback,
          latest,
        );
        void current.presentation.catch((error: unknown) => {
          if (live.current !== current || current.latest !== latest) return;
          current.error = String(error);
          setStatus(String(error));
        });
      },
      (error) => {
        if (live.current?.adapter !== adapter) return;
        live.current.error = error.message;
        setStatus(error.message);
      },
    );
    const current: BlenderViewerHandle = {
      canvas,
      client,
      adapter,
      output: null,
      presentation: Promise.resolve(),
    };
    live.current = current;
    window.ippBlender = current;
    current.presentation = select(fallback);
    await current.presentation;
    if (startup.current !== request) return;
    setStatus("Connecting to Blender…");
    adapter.connect({
      streamBatchSize: Number(connection.get("stream") ?? 0),
      refresh: connection.get("refresh") === "1",
    });
  }

  async function reconnect() {
    const request = ++startup.current;
    try {
      await live.current?.adapter.dispose();
    } catch (error) {
      if (startup.current === request) setStatus(String(error));
      return;
    }
    if (startup.current !== request) return;
    live.current = undefined;
    delete window.ippBlender;
    setOutput(null);
    setStatus("Connecting to Blender…");
    setGeneration((value) => value + 1);
  }

  function playback(action: "play" | "pause" | "stop") {
    const current = live.current;
    if (!current?.latest) return;
    void current.adapter.playback(action).catch((error: unknown) => {
      if (live.current === current) setStatus(String(error));
    });
  }

  return (
    <main>
      <header>
        <div>
          <h1>Blender → IPP</h1>
          <p role="status">{status}</p>
        </div>
        <nav aria-label="Playback">
          <button onClick={() => playback("play")}>Play</button>
          <button onClick={() => playback("pause")}>Pause</button>
          <button onClick={() => playback("stop")}>Stop</button>
          <button onClick={reconnect}>Reconnect</button>
        </nav>
      </header>
      <IppCanvas
        key={generation}
        runtime={runtime}
        world={{
          create: {
            symbolicId: "blender-viewer",
            selectedSystems: BLENDER_SYSTEMS,
          },
        }}
        output={output}
        width={960}
        height={640}
        canvasProps={{ "aria-label": "Blender scene" }}
        onReady={(canvas) => {
          const request = ++startup.current;
          void ready(canvas, request).catch((error: unknown) => {
            if (startup.current === request) setStatus(String(error));
          });
        }}
        onError={(error) => setStatus(error.message)}
      />
      {diagnostics.length > 0 && (
        <aside aria-label="Export diagnostics">
          <h2>Export notes</h2>
          <ul>
            {diagnostics.map((item, index) => (
              <li key={`${item.code}-${index}`}>
                {item.entity ? `${item.entity}: ` : ""}
                {item.message}
              </li>
            ))}
          </ul>
        </aside>
      )}
    </main>
  );
}

const element = document.getElementById("app");
if (!element) throw new Error("Viewer root is missing");
createRoot(element).render(<App />);
