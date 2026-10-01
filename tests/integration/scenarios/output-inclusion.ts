import { renderDiagnostics } from "../../../packages/ipp-client/src/diagnostics.js";
import {
  canvasOutput,
  sameOutputReference,
} from "../../../packages/ipp-client/src/references.js";
import type {
  Command,
  GuiWorldClient,
  HostClientBase,
  OutputReference,
  PresentedCapture,
  PresentedFrame,
} from "@ipp/client";
import { guiAction } from "../gui-actions.js";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../camera-fixtures.js";
export {
  presentationTransport,
  workerTransport,
} from "../presentation-transport.js";
import { controlTarget } from "./gui-lifecycle.js";
import {
  ATTACHMENTS,
  LIFECYCLE,
  SURFACE,
  CANVAS,
  GUI,
  selectSystems,
} from "../system-selections.js";
export { nativePresentationTransport } from "../../../packages/ipp-client/src/native-presentation.js";

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

function witness(frame: PresentedFrame, output: OutputReference) {
  const source = frame.sources.find(
    (source) => source.output.world.id === output.world.id,
  );
  check(
    source &&
      sameOutputReference(source.output, output) &&
      source.minimumTick > 0n &&
      source.tick >= source.minimumTick &&
      source.publication.host === frame.publication.host &&
      source.publication.revision > 0n,
    "Completion lost its exact post-admission output witness",
  );
  return source;
}

function pixel(image: PresentedCapture, expected: number[]) {
  const actual = [...new Uint8Array(image.pixels, (32 * 96 + 48) * 4, 4)];
  check(
    actual.every((channel, index) => Math.abs(channel - expected[index]!) <= 2),
    `Center pixel ${actual} differs from ${expected}`,
  );
}

async function unavailable(pending: Promise<unknown>) {
  const failure = await pending.then(
    () => null,
    (error: unknown) => error,
  );
  check(
    failure instanceof Error &&
      "reason" in failure &&
      failure.reason === "unavailable",
    `Expected exact output Unavailable, got ${String(failure)}`,
  );
}

export async function composedOutputInclusion(
  host: HostClientBase<GuiWorldClient>,
  evidence: {
    record(value: object): Promise<void>;
    capture(name: string, image: PresentedCapture): Promise<void>;
  },
) {
  // Both Worlds are canvases 96 units wide at 96 units per Surface metre.
  const canvas = { extent: [96, 64], unitsPerMetre: 96 } as const;
  const childCreation = {
    selectedSystems: selectSystems(GUI, LIFECYCLE),
    symbolicId: "included-child",
    canvas,
  };
  const childWorld = await host.createWorld(childCreation);
  const parentWorld = await host.createWorld({
    selectedSystems: selectSystems(ATTACHMENTS, CANVAS, SURFACE, LIFECYCLE),
    symbolicId: "included-parent",
    canvas,
  });
  const child = await host.openWorld(childWorld.reference);
  const parent = await host.openWorld(parentWorld.reference);
  let childOpen = true;
  let childReference = childWorld.reference;
  const shape = { kind: "alias", alias: 2 } as const;
  const button = { kind: "alias", alias: 3 } as const;
  try {
    const childOutcome = successfulBatch(
      await child.batch([
        createEntity(2, "child-fill"),
        insertComponent(child, "CanvasBox", shape, { width: 96, height: 64 }),
        insertComponent(child, "CanvasStyle", shape, {
          red: 1,
          green: 0,
          blue: 0,
        }),
        createEntity(3, "callback-only-button"),
        insertComponent(child, "GuiButton", button),
      ]),
    );
    const childOutput = canvasOutput(childWorld.reference);
    const attachment = parent.components.WorldAttachment!;
    const parentOutcome = successfulBatch(
      await parent.batch([
        createEntity(2, "child-surface"),
        insertComponent(parent, "Surface", shape, {
          width: 1,
          height: 64 / 96,
        }),
        {
          kind: "insertComponent",
          entity: shape,
          component: attachment.id,
          fields: [
            {
              offset: attachment.fields.mode!.offset,
              value: { kind: "u32", value: 1 },
            },
            {
              offset: attachment.fields.child!.offset,
              value: { kind: "world", value: childWorld.reference },
            },
          ],
        },
      ]),
    );
    const parentOutput = canvasOutput(parentWorld.reference);
    const view = await host.presentation.select(
      await host.presentation.surface(),
      await host.setRootOutput(parentOutput, {
        width: 96,
        height: 64,
        devicePixelRatio: 1,
      }),
    );
    const afterOutputs = [parentOutput, childOutput, childOutput];
    const first = await host.presentation.capture(view, { afterOutputs });
    check(
      first.sources.length === 2,
      "Duplicate requested output retained duplicate witness",
    );
    witness(first, childOutput);
    witness(first, parentOutput);
    pixel(first, [255, 0, 0, 255]);
    await evidence.capture("direct-red", first);

    const idle = await host.presentation.frame(view, {
      afterOutputs,
      afterSequence: first.sequence,
    });
    check(
      witness(idle, childOutput).minimumTick >
        witness(first, childOutput).minimumTick,
      "Idle request did not capture a new admission cut",
    );
    const buttonTarget = await controlTarget(
      child,
      aliasId(childOutcome, 3),
      child.components.GuiButton!.id,
    );
    check(buttonTarget, "Callback-only button unavailable");
    const action = await guiAction(child, buttonTarget, { kind: "press" });
    check(action.ok, "Callback-only action failed");
    const callback = await host.presentation.frame(view, { afterOutputs });
    check(
      witness(callback, childOutput).tick >= action.tick,
      "Output predates acknowledged callback",
    );
    await evidence.record({
      stage: "idle-and-callback",
      first: { ...first, pixels: undefined },
      idle,
      action,
      callback,
    });

    const color = (red: number, blue: number): Command =>
      insertComponent(
        child,
        "CanvasStyle",
        { kind: "handle", id: aliasId(childOutcome, 2) },
        { red, green: 0, blue },
      );
    successfulBatch(await child.batch([color(0, 1)]));
    const included = await host.presentation.frame(view, { afterOutputs });
    witness(included, childOutput);
    const blue = await host.presentation.capture(view, { afterOutputs });
    pixel(blue, [0, 0, 255, 255]);
    await evidence.capture("released-blue", blue);
    await evidence.record({ stage: "child-edit", included });

    const anchor = { kind: "handle", id: aliasId(parentOutcome, 2) } as const;
    successfulBatch(
      await parent.batch([
        insertComponent(parent, "SurfaceCache", anchor, {
          direct_distance: 0,
          texels_per_metre: 96,
          max_refresh_hz: 1,
        }),
      ]),
    );
    const cached = await host.presentation.capture(view, { afterOutputs });
    pixel(cached, [0, 0, 255, 255]);
    const diagnostics = renderDiagnostics(host);
    check(diagnostics, "Real render diagnostics absent");
    const before = await diagnostics.statistics();
    const reused = await host.presentation.frame(view, { afterOutputs });
    witness(reused, childOutput);
    const after = await diagnostics.statistics();
    check(
      before.surfaces &&
        after.surfaces &&
        before.surfaces.surfaceCacheEntries === 1 &&
        before.surfaces.totalSurfaceCacheRepaints > 0 &&
        after.surfaces.totalSurfaceCacheReuses >
          before.surfaces.totalSurfaceCacheReuses &&
        after.surfaces.totalSurfaceCacheRepaints ===
          before.surfaces.totalSurfaceCacheRepaints,
      "Unchanged output barrier did not reuse a populated image without repaint",
    );
    successfulBatch(await child.batch([color(1, 0)]));
    const refreshed = await host.presentation.capture(view, { afterOutputs });
    pixel(refreshed, [255, 0, 0, 255]);
    await evidence.capture("cached-red", refreshed);
    await evidence.record({
      stage: "cache",
      before,
      reused,
      after,
      refreshed: { ...refreshed, pixels: undefined },
    });

    // A canvas lives as long as its World: a recreated World is a new
    // output, and the destroyed World's output is no longer available.
    await child.close();
    childOpen = false;
    await host.destroyWorld(childWorld.reference);
    childReference = (await host.createWorld(childCreation)).reference;
    const replacement = canvasOutput(childReference);
    check(
      !sameOutputReference(replacement, childOutput),
      "A recreated canvas World reused its output lifetime",
    );
    await unavailable(
      host.presentation.frame(view, { afterOutputs: [childOutput] }),
    );
    await evidence.record({
      stage: "exact-replacement",
      childOutput,
      replacement,
    });
    await host.presentation.clear(view);
    return {
      idle: true,
      callbackOnly: true,
      cache: true,
      replacement: true,
    };
  } finally {
    await Promise.all([...(childOpen ? [child.close()] : []), parent.close()]);
    await host.destroyWorld(parentWorld.reference);
    await host.destroyWorld(childReference);
  }
}
