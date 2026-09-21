import { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { browserRuntime } from "@ipp/client";
import { IppCanvas, type IppCanvasHandle } from "@ipp/react/web";
import {
  BlenderAdapter,
  type AppliedRevision,
  type BlenderClient,
  type BlenderContract,
} from "../../integrations/blender/client/adapter.js";

export interface BlenderViewerHandle {
  canvas: IppCanvasHandle;
  adapter: BlenderAdapter;
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

  async function ready(canvas: IppCanvasHandle) {
    const request = ++startup.current;
    live.current?.adapter.close();
    const endpoint = connection.get("endpoint");
    const token = connection.get("token");
    if (!endpoint || !token) {
      setStatus("Open this viewer from Blender's IPP panel");
      return;
    }
    const contract: BlenderContract = await import(runtime.generatedModuleUrl);
    if (startup.current !== request) return;
    const adapter = new BlenderAdapter(
      canvas.client as BlenderClient,
      contract,
      new URL(endpoint),
      token,
      (latest) => {
        if (live.current?.adapter !== adapter) return;
        live.current.latest = latest;
        delete live.current.error;
        setStatus(`Connected · revision ${latest.revision}`);
        setDiagnostics(latest.diagnostics);
      },
      (error) => {
        if (live.current?.adapter !== adapter) return;
        live.current.error = error.message;
        setStatus(error.message);
      },
    );
    live.current = { canvas, adapter };
    window.ippBlender = live.current;
    setStatus("Connecting to Blender…");
    adapter.connect({
      streamBatchSize: Number(connection.get("stream") ?? 0),
      refresh: connection.get("refresh") === "1",
    });
  }

  function reconnect() {
    startup.current++;
    live.current?.adapter.close();
    live.current = undefined;
    delete window.ippBlender;
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
        width={960}
        height={640}
        canvasProps={{ "aria-label": "Blender scene" }}
        onReady={(canvas) => {
          void ready(canvas).catch((error: unknown) =>
            setStatus(String(error)),
          );
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
