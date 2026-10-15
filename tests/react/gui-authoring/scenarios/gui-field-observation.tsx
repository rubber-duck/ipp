import {
  canvasOutput,
  type Client,
  type HostClientBase,
  type RootBinding,
  type PresentationView,
} from "@ipp/client";
import {
  Entity,
  Scalar,
  createRoot,
  useField,
  type ReactFieldState,
  type ReactWorldClient,
} from "@ipp/react";
import { Box, Style } from "@ipp/react/gui";
import type { GuiContract } from "../pages/gui-authoring.js";
import { check } from "../../../harness/page/checks.js";

export interface FieldPresentation {
  <T>(
    binding: RootBinding,
    section: (view: PresentationView) => Promise<T>,
  ): Promise<T>;
}

/** Real field reports and pixels with delayed admission and retryable release. */
export async function guiFieldObservation(
  host: HostClientBase<Client>,
  _contract: GuiContract,
  _font: Uint8Array<ArrayBuffer>,
  present?: FieldPresentation,
) {
  const extent = [120, 80] as const;
  const world = (
    await host.createWorld({
      temporary: true,
      selectedSystems: [
        "ipp.animation",
        "ipp.constraints",
        "ipp.asset-dependencies",
        "ipp.canvas",
        "ipp.lifecycle-publisher",
      ],
      canvas: { extent, unitsPerMetre: 1 },
    })
  ).reference;
  const client = await host.openWorld(world);
  const watchLifecycle = client.watchLifecycle.bind(client);
  let releaseAdmission!: () => void;
  const admission = new Promise<void>((resolve) => {
    releaseAdmission = resolve;
  });
  let watchCount = 0;
  let removes = 0;
  client.watchLifecycle = async (...args) => {
    const watch = await watchLifecycle(...args);
    watchCount++;
    void watch.closed.then(() => {
      watchCount--;
    });
    await admission;
    return {
      ...watch,
      async remove() {
        if (++removes === 1)
          throw new Error(
            "Injected first release failure before native removal",
          );
        return watch.remove();
      },
    };
  };
  const errors: Error[] = [];
  const root = createRoot(client as unknown as ReactWorldClient, {
    onError: (error) => {
      errors.push(error);
    },
  });
  let field: ReactFieldState | undefined;
  let absent: ReactFieldState | undefined;
  let binding: RootBinding | undefined;
  const images: {
    label: string;
    width: number;
    height: number;
    pixels: number[];
    sequence: bigint;
  }[] = [];
  const report: string[] = [];
  function Marker() {
    field = useField("field-source", "Scalar", "value");
    absent = useField("field-missing", "Scalar", "value");
    const red =
      field.status === "ready" && typeof field.value === "number"
        ? field.value / 20
        : 0;
    return (
      <Entity id="field-mark">
        <Style red={red} green={0} blue={0} />
        <Box width={80} height={40} />
      </Entity>
    );
  }
  const wait = async (ready: () => boolean, label: string) => {
    const deadline = performance.now() + 15_000;
    while (!ready()) {
      check(
        performance.now() < deadline,
        `Field observation timeout: ${label}`,
      );
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
    await root.flush();
  };
  try {
    await root.render(
      <>
        <Entity id="field-source">
          <Scalar value={10} />
        </Entity>
        <Marker />
      </>,
    );
    await wait(
      () => field?.status === "ready" && absent?.status === "absent",
      "native baseline/absent target",
    );
    binding = await host.setRootOutput(canvasOutput(world), {
      width: extent[0],
      height: extent[1],
      devicePixelRatio: 1,
    });
    const section = async (view: PresentationView) => {
      let sequence: bigint | undefined;
      const capture = async (label: string) => {
        const frame = await host.presentation.capture(view, {
          afterOutputs: [canvasOutput(world)],
          ...(sequence === undefined ? {} : { afterSequence: sequence }),
        });
        sequence = frame.sequence;
        const pixels = new Uint8Array(frame.pixels);
        images.push({
          label,
          width: extent[0],
          height: extent[1],
          pixels: Array.from(pixels),
          sequence,
        });
        return pixels[(20 * extent[0] + 30) * 4]!;
      };
      const initial = await capture("field-observation-baseline");
      check(
        (
          await client.batch([
            {
              kind: "setField",
              entity: { kind: "symbol", symbol: "field-source" },
              component: client.components.Scalar!.id,
              field: {
                offset: client.components.Scalar!.fields.value!.offset,
                value: { kind: "f32", value: 20 },
              },
            },
          ])
        ).ok,
        "Native field edit failed",
      );
      await wait(
        () => field?.status === "ready" && field.value === 20,
        "live native report",
      );
      const edited = await capture("field-observation-edited");
      check(
        edited > initial + 20,
        "Native observed value did not change rendered pixels",
      );
      check(
        watchCount === 1,
        "Routine observation renders recreated the native watch",
      );
      report.push(
        "native lifecycle reports drive actual Canvas pixels with one stable watch; unknown symbolic target is absent",
      );
    };
    if (present) await present(binding, section);
    else {
      const surface = await host.presentation.surface();
      const view = await host.presentation.select(surface, binding);
      try {
        await section(view);
      } finally {
        await host.presentation.clear(view);
      }
    }
    let completed = false;
    const closing = root.unmount().then(
      () => {
        completed = true;
      },
      (error) => {
        throw error;
      },
    );
    await new Promise((resolve) => setTimeout(resolve, 20));
    check(
      !completed && watchCount === 1,
      "Unmount failed to await pending accepted watch admission",
    );
    releaseAdmission();
    let failed = false;
    try {
      await closing;
    } catch {
      failed = true;
    }
    check(
      failed && removes === 1 && watchCount === 1,
      "Failed release discarded native watch ownership",
    );
    await root.unmount();
    check(
      Number(removes) === 2 && Number(watchCount) === 0,
      "Cleanup retry failed to release exact native watch",
    );
    check(errors.length >= 1, "Cleanup failure was silently swallowed");
    report.push(
      "unmount waits late admission, exposes failed release and retries the exact retained native handle",
    );
    return { failure: null, images, report };
  } catch (error) {
    return {
      failure:
        error instanceof Error ? (error.stack ?? error.message) : String(error),
      images,
      report,
    };
  } finally {
    releaseAdmission();
    await root.unmount().catch(() => {});
    if (binding) await host.clearRootOutput(binding).catch(() => {});
    await client.close();
    await host.destroyWorld(world);
  }
}
