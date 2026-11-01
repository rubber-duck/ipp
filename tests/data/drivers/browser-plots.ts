/** Worker/WebGL driver for the same transport-independent Plot scenarios. */
import type { Client, HostClientBase } from "@ipp/client";
import { createWorkerHost } from "../../../packages/ipp-client/src/worker.js";
import { createPlotCapture } from "../support/plot-capture.js";
import { exercisePlot2d } from "../scenarios/plots-2d.js";
import { presentationTesting } from "../../../packages/ipp-client/src/testing.js";
import { renderDiagnostics } from "../../../packages/ipp-client/src/diagnostics.js";
import { check } from "../../harness/page/checks.js";
import { exercisePlotFills } from "../scenarios/plot-fills.js";
import { exerciseDenseLines } from "../scenarios/dense-lines.js";
import { exercisePlot3d } from "../scenarios/plots-3d.js";
import { reactPlots } from "../scenarios/react-plots.js";
import { exercisePlotAxisSupport } from "../scenarios/plot-axis-support.js";
import { exercisePlotAxisMotion } from "../scenarios/plot-axis-motion.js";
import { exercisePlotAxisGating } from "../scenarios/plot-axis-gating.js";
import { exercisePlotGlass } from "../scenarios/plot-glass.js";
import { exercisePlotPercentage } from "../scenarios/plot-percentage.js";
import {
  exercisePlotViewPlacement,
  type PlotOffscreenCase,
} from "../scenarios/plot-view-placement.js";

function encodePixels(pixels: Uint8Array): string {
  const chunks: string[] = [];
  for (let start = 0; start < pixels.length; start += 0x8000)
    chunks.push(String.fromCharCode(...pixels.subarray(start, start + 0x8000)));
  return btoa(chunks.join(""));
}

export async function workerPlots(
  urls: {
    generated: string;
    workerScript: string;
    wasm: string;
    origin: string;
  },
  family:
    | "glass-2d"
    | "glass-3d"
    | "glass-lifecycle"
    | "2d"
    | "dense"
    | "fills"
    | "fills-recovery"
    | "3d"
    | "react"
    | "view"
    | "axis"
    | "axis-motion"
    | "axis-gating"
    | "percentage",
  offscreen?: PlotOffscreenCase,
  transformed = false,
) {
  const contract = await import(urls.generated);
  const canvas = document.createElement("canvas");
  canvas.width = 1400;
  canvas.height = 1000;
  document.body.replaceChildren(canvas);
  const owner = createWorkerHost(
    urls.workerScript,
    urls.wasm,
    contract.MAX_MESSAGE_BYTES,
    {
      canvas: canvas.transferControlToOffscreen(),
    },
  );
  let connection: HostClientBase<Client> | undefined;
  const record = async (label: string, value: unknown) => {
    await (
      globalThis as unknown as {
        recordPlot(label: string, value: unknown): Promise<void>;
      }
    ).recordPlot(
      label,
      JSON.parse(
        JSON.stringify(value, (_, v) =>
          typeof v === "bigint" ? { $bigint: v.toString() } : v,
        ),
      ),
    );
  };
  try {
    const host: HostClientBase<Client> =
      await contract.IppHostClient.connectTransport(owner.connect());
    connection = host;
    const font = new Uint8Array(
      await (
        await fetch(`${urls.origin}/target/font-assets/shure-tech-mono.ippf`)
      ).arrayBuffer(),
    );
    const surface = await host.presentation.surface();
    const saveFrame = async (
      label: string,
      frame: { width: number; height: number; pixels: Uint8Array },
    ) =>
      record("capture", {
        label,
        width: frame.width,
        height: frame.height,
        rgba: encodePixels(frame.pixels),
      });
    let capture = createPlotCapture(host, surface, saveFrame, record);
    const recover = async () => {
      await renderDiagnostics(host)!.statistics();
      const testing = presentationTesting(host);
      testing.loseContext();
      let lost = false;
      const deadline = performance.now() + 30000;
      while (performance.now() < deadline) {
        try {
          await host.presentation.surface();
        } catch {
          lost = true;
          break;
        }
        await new Promise((resolve) => setTimeout(resolve, 20));
      }
      check(lost, "Plot context loss was not observed");
      testing.restoreContext();
      while (performance.now() < deadline) {
        try {
          const restored = await host.presentation.surface();
          if (restored.context !== surface.context) {
            capture = createPlotCapture(host, restored, saveFrame, record);
            await record("plot-context-restored", {
              before: surface.context,
              after: restored.context,
            });
            return;
          }
        } catch {
          /* Restoration is asynchronous. */
        }
        await new Promise((resolve) => setTimeout(resolve, 20));
      }
      throw new Error("Plot context restoration timed out");
    };
    return family === "glass-2d" ||
      family === "glass-3d" ||
      family === "glass-lifecycle"
      ? await exercisePlotGlass(host, contract, font, capture, record, family)
      : family === "2d"
        ? await exercisePlot2d(host, contract, font, capture, record)
        : family === "fills" || family === "fills-recovery"
          ? await exercisePlotFills(
              host,
              contract,
              font,
              (label, binding) => capture(label, binding),
              record,
              family === "fills-recovery" ? recover : undefined,
            )
          : family === "dense"
            ? await exerciseDenseLines(host, contract, font, capture, record)
            : family === "3d"
              ? await exercisePlot3d(host, contract, font, capture)
              : family === "percentage"
                ? await exercisePlotPercentage(
                    host,
                    contract,
                    font,
                    capture,
                    record,
                  )
                : family === "axis-gating"
                  ? await exercisePlotAxisGating(
                      host,
                      contract,
                      font,
                      capture,
                      record,
                    )
                  : family === "axis-motion"
                    ? await exercisePlotAxisMotion(
                        host,
                        contract,
                        font,
                        capture,
                        record,
                      )
                    : family === "axis"
                      ? await exercisePlotAxisSupport(
                          host,
                          contract,
                          font,
                          capture,
                          record,
                          transformed,
                        )
                      : family === "view"
                        ? await exercisePlotViewPlacement(
                            host,
                            contract,
                            font,
                            capture,
                            record,
                            offscreen,
                          )
                        : await reactPlots(
                            host,
                            contract,
                            font,
                            capture,
                            record,
                          );
  } finally {
    try {
      await connection?.close();
    } finally {
      await owner.close();
    }
  }
}
