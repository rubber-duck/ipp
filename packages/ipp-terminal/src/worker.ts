import {
  PortTransport,
  type HostClientBase,
  type GuiWorldClient,
} from "@ipp/client";
import { TerminalSession } from "./session.js";
import type { TerminalConfiguration, TerminalContract } from "./types.js";

export interface TerminalWorkerStart {
  type: "start";
  port: MessagePort;
  connection: bigint;
  generatedModuleUrl: string;
  configuration: TerminalConfiguration;
}

/** Only launch/close handshakes cross this port; terminal traffic uses IPP. */
export function installTerminalWorker(
  scope: Pick<Window, "addEventListener" | "postMessage">,
): void {
  let host: HostClientBase<GuiWorldClient> | undefined;
  let session: TerminalSession | undefined;
  let started = false;
  let startup: Promise<void> = Promise.resolve();
  const report = (error: unknown) =>
    scope.postMessage({
      type: "error",
      error: error instanceof Error ? error.message : String(error),
    });
  scope.addEventListener(
    "message",
    (event: MessageEvent<TerminalWorkerStart | { type: "close" }>) => {
      if (event.data.type === "close") {
        void startup
          .catch(() => {})
          .then(async () => {
            try {
              await session?.close();
              await host?.close();
              scope.postMessage({ type: "closed" });
            } catch (error) {
              report(error);
            }
          });
        return;
      }
      if (event.data.type !== "start" || started) {
        report(new Error("Invalid terminal Worker launch"));
        return;
      }
      started = true;
      const message = event.data;
      startup = (async () => {
        const contract = (await import(
          message.generatedModuleUrl
        )) as TerminalContract & {
          IppHostClient: {
            connectTransport(
              transport: PortTransport,
              options: { timeoutMs: number },
            ): Promise<HostClientBase<GuiWorldClient>>;
          };
        };
        host = await contract.IppHostClient.connectTransport(
          new PortTransport(message.port, message.connection),
          { timeoutMs: 20000 },
        );
        const client = await host.openWorld(message.configuration.world);
        session = await TerminalSession.start({
          ...message.configuration,
          client,
          contract,
          onError: report,
        });
        scope.postMessage({ type: "ready" });
      })();
      void startup.catch(report);
    },
  );
}
