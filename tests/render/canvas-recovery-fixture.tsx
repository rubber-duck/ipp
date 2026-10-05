/** Ordinary rejected authoring through real React DOM, worker transport and Canvas output. */
import { createRef } from "react";
import { flushSync } from "react-dom";
import { createRoot as createDomRoot } from "react-dom/client";
import { canvasOutput, type Client, type GuiWorldClient } from "@ipp/client";
import {
  CanvasWorld,
  Entity,
  FlatSurface,
  createRoot,
  type CanvasWorldHandle,
} from "@ipp/react";
import {
  Box,
  Button,
  Layout,
  Style,
  type GuiControlHandle,
} from "@ipp/react/gui";
import { IppCanvas, World, type IppCanvasHandle } from "@ipp/react/web";
import {
  GUI,
  SURFACE,
  ATTACHMENTS,
  LIFECYCLE,
  selectSystems,
} from "../integration/system-selections.js";
import {
  waitUntil,
  type CanvasRuntimeInput,
} from "./canvas-fixture-helpers.js";

let retainedEvidence: {
  images: Record<string, string>;
  observations: Record<string, unknown>;
} = { images: {}, observations: {} };

export function recoveryEvidence() {
  return retainedEvidence;
}

export async function declarationRecovery(
  configuration: CanvasRuntimeInput,
  kind: "World" | "CanvasWorld",
  explicit: boolean,
) {
  retainedEvidence = { images: {}, observations: {} };
  const container = document.createElement("div");
  document.body.replaceChildren(container);
  const dom = createDomRoot(container);
  const errors: string[] = [];
  const originalError = console.error;
  console.error = (...args: unknown[]) => {
    for (const value of args)
      if (value instanceof Error) errors.push(value.message);
    originalError(...args);
  };
  const selectedSystems = selectSystems(GUI, LIFECYCLE, SURFACE, ATTACHMENTS);
  const button = createRef<GuiControlHandle>();
  const healthyButton = createRef<GuiControlHandle>();
  const childErrors: string[] = [];
  let healthyCallbacks = 0;
  let healthyWorld: CanvasWorldHandle | undefined;
  let handle: IppCanvasHandle | undefined;
  let child: CanvasWorldHandle | undefined;
  let width = 40;
  let revision = 0;
  const callbacks: number[] = [];
  let client: Client | undefined;
  let peer: Client | undefined;
  let independent: ReturnType<typeof createRoot> | undefined;
  let independentWorld: NonNullable<Client["worldReference"]> | undefined;
  let batches = 0;
  const observe = (error: Error) => {
    errors.push(error.message);
  };
  const content = () => {
    const callbackRevision = revision;
    return (
      <>
        <Entity id="recovery-box">
          <Box width={width} height={24} />
          <Style red={1} green={0} blue={0} />
        </Entity>
        <Entity id="healthy-anchor">
          <FlatSurface width={16} height={16} />
          <Style x={76} y={44} />
        </Entity>
        <CanvasWorld
          create={{ selectedSystems }}
          extent={[16, 16]}
          presentation={{ anchor: "healthy-anchor" }}
          onReady={(ready) => {
            healthyWorld = ready;
          }}
          onError={(error) => {
            childErrors.push(error.message);
          }}
        >
          <Entity id="healthy-button">
            <Layout width={16} height={16} align_x={-1} align_y={-1} />
            <Button
              ref={healthyButton}
              onPress={() => {
                healthyCallbacks++;
              }}
            />
          </Entity>
        </CanvasWorld>
        <Entity id="recovery-button">
          <Layout width={16} height={16} align_x={-1} align_y={-1} />
          <Style y={32} />
          <Button
            ref={button}
            onPress={() => {
              callbacks.push(callbackRevision);
            }}
          />
        </Entity>
      </>
    );
  };
  const render = () =>
    flushSync(() =>
      dom.render(
        <IppCanvas
          runtime={configuration}
          world={{
            create: {
              selectedSystems,
              canvas: { extent: [96, 64], unitsPerMetre: 1 },
            },
          }}
          width={96}
          height={64}
          onReady={(ready) => {
            handle = ready;
          }}
          {...(explicit ? { onError: observe } : {})}
          {...(kind === "World"
            ? {
                output: handle
                  ? canvasOutput(handle.client.worldReference!)
                  : null,
              }
            : {})}
        >
          {handle &&
            (kind === "World" ? (
              <World>{content()}</World>
            ) : (
              <CanvasWorld
                create={{ selectedSystems }}
                extent={[96, 64]}
                presentation={{ root: true }}
                onReady={(ready) => {
                  child = ready;
                }}
              >
                {content()}
              </CanvasWorld>
            ))}
        </IppCanvas>,
      ),
    );
  function check(value: unknown, message: string): asserts value {
    if (!value) throw new Error(message);
  }
  const box = async (target: Client) => {
    const entity = (await target.inspect()).entities.find(
      (entry) => entry.metadata.symbolicId === "recovery-box",
    );
    check(entity, "Recovery box disappeared");
    const component = entity.components.find(
      (entry) => entry.component === target.components.CanvasBox!.id,
    );
    check(component, "Recovery box component disappeared");
    return { id: entity.id, width: component.fields.width };
  };
  const capture = async (label: string) => {
    await waitUntil(
      () => handle!.view !== null,
      "recovery Canvas presentation",
    );
    const frame = await handle!.host.presentation.capture(handle!.view!, {
      afterOutputs: [canvasOutput(client!.worldReference!)],
    });
    const canvas = document.createElement("canvas");
    canvas.width = frame.view.binding.viewport.width;
    canvas.height = frame.view.binding.viewport.height;
    canvas
      .getContext("2d")!
      .putImageData(
        new ImageData(
          new Uint8ClampedArray(frame.pixels),
          canvas.width,
          canvas.height,
        ),
        0,
        0,
      );
    retainedEvidence.images[label] = canvas.toDataURL("image/png");
    retainedEvidence.observations[label] = {
      sequence: frame.sequence,
      sources: frame.sources,
      box: await box(client!),
    };
    return frame;
  };
  const redPixels = (pixels: ArrayBuffer) => {
    const bytes = new Uint8Array(pixels);
    let count = 0;
    for (let index = 0; index < bytes.length; index += 4)
      if (
        bytes[index]! > 200 &&
        bytes[index + 1]! < 60 &&
        bytes[index + 2]! < 60
      )
        count++;
    return count;
  };
  try {
    render();
    await waitUntil(() => !!handle, "Canvas session ready");
    render();
    await waitUntil(
      () =>
        !!button.current &&
        !!healthyButton.current &&
        !!healthyWorld &&
        (kind === "World" || !!child),
      "initial recovery declarations",
    );
    await handle!.flush();
    client =
      kind === "World"
        ? handle!.client
        : await handle!.host.openWorld(child!.world);
    const authored =
      kind === "World"
        ? client
        : [...handle!.host.sessions.values()].find(
            (session) =>
              session.worldReference?.id === child!.world.id &&
              session !== client,
          )!;
    const batch = authored.batch.bind(authored);
    authored.batch = async (commands) => {
      batches++;
      return batch(commands);
    };
    const initial = await box(client);
    const initialPixels = redPixels((await capture("initial")).pixels);
    check(
      initialPixels > 500,
      `Initial acknowledged box did not render: ${initialPixels} red pixels`,
    );
    await button.current!.action({ kind: "press" });
    await waitUntil(() => callbacks.length === 1, "initial GUI callback");
    independentWorld = (
      await handle!.host.createWorld({
        selectedSystems,
        canvas: { extent: [96, 64], unitsPerMetre: 1 },
      })
    ).reference;
    peer = await handle!.host.openWorld(independentWorld);
    independent = createRoot(peer);
    await independent.render(
      <Entity id="recovery-box">
        <Box width={12} height={12} />
      </Entity>,
    );
    const priorButton = button.current!;
    const priorHealthyWorld = healthyWorld!.world;
    width = -1; // CanvasBox explicitly rejects negative dimensions with InvalidValue.
    revision = 1;
    render();
    await waitUntil(
      () => errors.some((error) => error.includes("InvalidValue")),
      "ordinary authoring rejection observation",
    );
    await handle!.flush().catch(() => {});
    const afterFailure = await box(client);
    const failedBatches = batches;
    await handle!.flush().catch(() => {});
    await handle!.flush().catch(() => {});
    check(
      batches === failedBatches,
      "Invalid declarations retried without a correction",
    );
    check(
      afterFailure.id === initial.id && afterFailure.width === 40,
      "Rejected value replaced last acknowledged box",
    );
    let effectObserved = false;
    const effects = await (client as GuiWorldClient).subscribeGuiEffects(
      (event) => {
        if (
          event.target.entity === priorButton.target.entity &&
          event.effect.kind === "pressed"
        )
          effectObserved = true;
      },
    );
    try {
      const outcome = await priorButton.action({ kind: "press" });
      check(
        outcome.ok,
        "Known-live button failed during declaration rejection",
      );
      await waitUntil(
        () => effectObserved,
        "GUI effect during authoring rejection",
      );
      check(
        callbacks.length === 1,
        "Rejected declaration republished GUI callbacks",
      );
    } finally {
      await effects.unsubscribe();
    }
    check(
      childErrors.length === 0,
      "Parent declaration rejection became a child boundary failure",
    );
    await healthyButton.current!.action({ kind: "press" });
    await waitUntil(
      () => healthyCallbacks === 1,
      "healthy attached child callback during parent rejection",
    );
    const failurePixels = redPixels((await capture("failure")).pixels);
    check(
      failurePixels === initialPixels,
      "Rejected box edit changed completed pixels",
    );
    await independent.render(
      <Entity id="recovery-box">
        <Box width={20} height={12} />
      </Entity>,
    );
    check(
      (await box(peer)).width === 20,
      "Independent World stopped after ordinary rejection",
    );
    width = 60;
    revision = 2;
    render();
    await waitUntil(
      () => button.current !== null && batches > failedBatches,
      "corrected authoring submission",
    );
    await handle!.flush();
    const corrected = await box(client);
    check(corrected.width === 60, "Corrected declaration did not acknowledge");
    await button.current!.action({ kind: "press" });
    await waitUntil(
      () => callbacks.length === 2,
      "GUI callback after correction",
    );
    check(
      callbacks.join(",") === "0,2",
      "Callbacks did not resume with corrected declarations",
    );
    check(
      childErrors.length === 0,
      "Correction reported an attached child failure",
    );
    check(
      healthyWorld!.world.id === priorHealthyWorld.id,
      "Recovery replaced the healthy child World",
    );
    await healthyButton.current!.action({ kind: "press" });
    await waitUntil(
      () => healthyCallbacks === 2,
      "healthy attached child callback after correction",
    );
    const correctedPixels = redPixels((await capture("corrected")).pixels);
    check(
      correctedPixels > failurePixels + 300,
      "Corrected box did not change completed output",
    );
    check(
      handle!.client.closure === undefined,
      "Recovery closed the Canvas session",
    );
    return {
      kind,
      explicit,
      errors,
      childErrors,
      healthyCallbacks,
      initial,
      afterFailure,
      corrected,
      callbacks,
      batchesAfterFailure: failedBatches,
      batchesAfterCorrection: batches,
      initialPixels,
      failurePixels,
      correctedPixels,
      independentWidth: (await box(peer)).width,
    };
  } finally {
    await independent?.unmount();
    await peer?.close();
    if (independentWorld && handle)
      await handle.host.destroyWorld(independentWorld);
    if (client && client !== handle?.client) await client.close();
    dom.unmount();
    if (handle) await handle.closed;
    console.error = originalError;
  }
}
