/** Native presentation diagnostics for the shared gallery workload. */
import { renderDiagnostics } from "@ipp/client/diagnostics";
import { nativePresentationTransport } from "@ipp/client/testing";
import type { GallerySceneDefinition } from "../../../examples/demos/shared/scene.js";
import { hostProfiling } from "../../../packages/ipp-client/src/profiling.js";
import { defineClient } from "../../../tools/shared-host/client.js";
import {
  type GalleryBenchmarkConfig,
  measureGallery,
  registry,
} from "../scenarios/gallery.js";

interface RunnerConfig extends GalleryBenchmarkConfig {
  readonly url: string;
  readonly presentationUrl?: string;
}

export default defineClient({
  async open(context, args) {
    const config: RunnerConfig = JSON.parse(args[0]!);
    const module = (await context.load(registry))!.module;
    const definitions =
      module.GALLERY_SCENE_DEFINITIONS as readonly GallerySceneDefinition[];
    const Host = context.contract
      .IppHostClient as typeof import("@ipp/host-contract").IppHostClient;
    const diagnosticsHost = config.presentationUrl
      ? await Host.connectTransport(
          nativePresentationTransport(config.url, config.presentationUrl),
        )
      : await Host.connectWebSocket(config.url);
    return { config, definitions, diagnosticsHost };
  },
  async capture({ config, definitions, diagnosticsHost }, context) {
    const diagnostics = renderDiagnostics(diagnosticsHost);
    const profiler = config.instrumented
      ? hostProfiling(diagnosticsHost)
      : undefined;
    return {
      report: await measureGallery(
        config,
        definitions,
        context,
        diagnostics,
        profiler,
      ),
    };
  },
  async close({ diagnosticsHost }) {
    await diagnosticsHost.close();
  },
});
