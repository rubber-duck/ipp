import type { MessageTransport, TransportEvents } from "@ipp/client";
import { workerTransport } from "../../../../packages/ipp-client/src/worker.js";
import { nativePresentationTransport } from "../../../../packages/ipp-client/src/native-presentation.js";
import {
  AGENT_FORM_SIZE,
  agentActions,
  type AgentActionsEnvironment,
} from "../scenarios/agent-actions.js";

/** Native WebSocket and worker/WASM share all semantic operations/assertions. */
export async function runAgentActions(
  urls: { generated: string; wasm: string; origin: string },
  endpoint: { url: string; presentationUrl: string } | undefined,
  environment: Omit<AgentActionsEnvironment, "loseConnectionOnNextSend">,
) {
  const contract = await import(urls.generated);
  let original: MessageTransport;
  if (endpoint) {
    original = nativePresentationTransport(
      endpoint.url,
      endpoint.presentationUrl,
    );
  } else {
    const canvas = document.createElement("canvas");
    Object.assign(canvas, AGENT_FORM_SIZE);
    document.body.append(canvas);
    original = workerTransport(
      `${urls.origin}/target/gui-agents/worker.js`,
      urls.wasm,
      contract.MAX_MESSAGE_BYTES,
      { canvas: canvas.transferControlToOffscreen() },
    );
  }
  let cut = false;
  let dropped = false;
  let events: TransportEvents;
  const transport: MessageTransport = {
    start(listener) {
      events = listener;
      original.start({
        ...listener,
        message: (bytes) => {
          if (!dropped) listener.message(bytes);
        },
      });
    },
    send(bytes) {
      const result = original.send(bytes);
      if (cut) {
        cut = false;
        dropped = true;
        // Bytes have crossed the real transport boundary; lose its reply before
        // telling the client it closed. Application may already have happened.
        queueMicrotask(() => {
          events.closed();
          void original.close();
        });
      }
      return result;
    },
    close: () => original.close(),
  };
  const host = await contract.IppHostClient.connectTransport(transport);
  try {
    const font = await (
      await fetch(`${urls.origin}/target/font-assets/shure-tech-mono.ippf`)
    ).arrayBuffer();
    return await agentActions(host, contract, font, {
      ...environment,
      loseConnectionOnNextSend: () => {
        cut = true;
      },
    });
  } finally {
    await host.close();
  }
}
