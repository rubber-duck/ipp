/** Sustained controlled drags through physical input on planar and curved surfaces. */
import {
  type Client,
  type HostClientBase,
  type PresentedCapture,
  type RootBinding,
} from "@ipp/client";
import {
  Asset,
  AttachedWorld,
  Camera,
  CylinderSurface,
  Entity,
  FlatSurface,
  Transform,
  assetRef,
  createRoot,
  guiWorldOptions,
  type ReactWorldClient,
} from "@ipp/react";
import {
  Column,
  GuiKit,
  Knob,
  LabelledSlider,
  type GuiKitContract,
} from "@ipp/react/gui-kit";
import { cybertronSkin } from "@ipp/react/gui-cybertron";
import type { GuiContract } from "../pages/gui-authoring.js";
import { check } from "../../../harness/page/checks.js";
import {
  pixelFor,
  projectedSurfacePoint,
  type Pose,
} from "./gui-projected-surfaces.js";
import type { WidgetPresentation } from "./gui-widgets.js";

export async function guiSliderDrags(
  host: HostClientBase<Client>,
  contract: GuiContract & GuiKitContract,
  font: Uint8Array<ArrayBuffer>,
  present?: WidgetPresentation,
) {
  const world = (
    await host.createWorld(
      guiWorldOptions({
        temporary: true,
        canvas: { extent: [160, 120], unitsPerMetre: 100 },
      }),
    )
  ).reference;
  const parent = (
    await host.createWorld({
      temporary: true,
      selectedSystems: [
        "ipp.animation",
        "ipp.asset-dependencies",
        "ipp.hierarchy",
        "ipp.look-at",
        "ipp.final-propagation",
        "ipp.geometry",
        "ipp.render",
        "ipp.camera",
        "ipp.surface",
        "ipp.world-attachment",
        "ipp.lifecycle-publisher",
      ],
    })
  ).reference;
  const client = await host.openWorld(world);
  const cameraClient = await host.openWorld(parent);
  const errors: Error[] = [];
  const root = createRoot(client as unknown as ReactWorldClient, {
    onError: (error) => errors.push(error),
  });
  const cameraRoot = createRoot(cameraClient as unknown as ReactWorldClient, {
    host,
    onError: (error) => errors.push(error),
  });
  const skin = cybertronSkin(contract);
  const images: {
    label: string;
    width: number;
    height: number;
    pixels: number[];
    sequence: bigint;
  }[] = [];
  const report: string[] = [];
  let binding: RootBinding | undefined;
  let intent = 0;
  let accept = true;
  let release = () => {};
  let gate = Promise.resolve();
  let updates: Promise<void>[] = [];
  const change = (value: number) => {
    const pending = gate.then(async () => {
      if (accept) intent = value;
      await render();
    });
    updates.push(pending);
    return pending;
  };
  const render = () =>
    root.render(
      <>
        <Asset
          id="drag-font"
          kind={17}
          data={font}
          encode={(bytes: Uint8Array<ArrayBuffer>) => bytes}
        />
        <GuiKit
          skin={skin}
          contract={contract}
          font={assetRef("drag-font")}
          fontSize={8}
        >
          <Column id="drag-form" width={160} height={120} padding={8} gap={8}>
            <LabelledSlider
              id="drag-slider"
              label="Accepted energy"
              value={intent}
              min={0}
              max={1}
              step={0.01}
              bounds={false}
              onChange={change}
            />
            <Knob
              id="drag-knob"
              label="Accepted gain"
              value={intent}
              min={0}
              max={1}
              step={0.01}
              size={32}
              bounds={false}
              onChange={change}
            />
          </Column>
        </GuiKit>
      </>,
    );
  const save = (label: string, frame: PresentedCapture) => {
    check(
      frame.drawCalls > 0 && frame.triangles > 0,
      `${label}: rendered controls`,
    );
    images.push({
      label,
      width: 400,
      height: 300,
      pixels: Array.from(new Uint8Array(frame.pixels)),
      sequence: frame.sequence,
    });
  };
  try {
    await render();
    for (const shape of ["flat", "cylinder"] as const) {
      const pose: Pose = { shape, curvature: 0.8, spacing: 0 };
      await cameraRoot.render(
        <>
          <Entity id="drag-camera">
            <Transform z={2.5} />
            <Camera projection={0} fov_y={Math.PI / 4} near={0.05} far={20} />
          </Entity>
          <Entity id="drag-panel">
            {shape === "flat" ? (
              <FlatSurface width={1.6} height={1.2} />
            ) : (
              <CylinderSurface width={1.6} height={1.2} curvature={0.8} />
            )}
          </Entity>
          <AttachedWorld
            anchor="drag-panel"
            child={{ borrow: world }}
            attachment={{ mode: "surface-canvas" }}
          />
        </>,
      );
      if (!binding)
        binding = await host.setRootOutput(
          await cameraRoot.bindOutput("drag-camera", "camera"),
          { width: 400, height: 300, devicePixelRatio: 1 },
        );
      const section: Parameters<WidgetPresentation>[1] = async (view) => {
        const input = await host.input.open(view);
        try {
          for (const kind of ["slider", "knob"] as const) {
            for (accept of [true, false]) {
              intent = 0;
              await render();
              await host.presentation.frame(view, {
                afterOutputs: [{ kind: "canvas", world }],
              });
              const entity = (await client.inspect()).entities.find(
                (item) =>
                  item.metadata.symbolicId ===
                  `drag-${kind}/${kind === "slider" ? "slider" : "dial"}`,
              );
              check(entity, `Missing ${kind}`);
              const bounds = entity.components.find(
                (component) =>
                  component.component === client.components.CanvasBounds!.id,
              )?.fields;
              check(bounds, `Missing ${kind} bounds`);
              const rect = ["x", "y", "width", "height"].map((field) =>
                Number(bounds[field]),
              );
              const point = (fraction: number) => {
                const content =
                  kind === "slider"
                    ? ([
                        rect[0]! + rect[2]! * fraction,
                        rect[1]! + rect[3]! / 2,
                      ] as const)
                    : ([
                        rect[0]! + rect[2]! / 2,
                        rect[1]! + rect[3]! / 2 - fraction * rect[3]! * 2,
                      ] as const);
                const pixel = pixelFor(projectedSurfacePoint(pose, content));
                return [pixel[0] / 400, pixel[1] / 300] as const;
              };
              const values: number[] = [];
              const watch = await client.watchLifecycle(
                [
                  {
                    target: {
                      kind: "value",
                      entity: entity.id,
                      component: client.components.GuiSlider!.id,
                      fields: [
                        client.components.GuiSlider!.fields.value!.offset,
                      ],
                    },
                    kinds: 128,
                  },
                ],
                (event) => {
                  if (event.kind === "value" && event.values)
                    values.push(Number(event.values[0]!.value));
                },
              );
              gate = new Promise<void>((resolve) => {
                release = resolve;
              });
              updates = [];
              const label = `${shape}-${kind}-${accept ? "accepted" : "refused"}`;
              try {
                const initial = await host.presentation.capture(view);
                await input.send({
                  kind: "pointerMove",
                  pointer: 1n,
                  point: point(kind === "slider" ? 0.08 : 0),
                });
                const down = await input.send({
                  kind: "pointerDown",
                  pointer: 1n,
                  point: point(kind === "slider" ? 0.08 : 0),
                });
                check(
                  down.disposition === "routed",
                  `${label}: real down routed`,
                );
                for (let index = 1; index <= 16; index++) {
                  const fraction =
                    kind === "slider" ? 0.08 + index * 0.045 : index * 0.035;
                  await input.send({
                    kind: "pointerMove",
                    pointer: 1n,
                    point: point(fraction),
                  });
                  if (index % 4 === 0) await render(); // Unrelated parent rerenders retain stale intent.
                  await host.presentation.frame(view, {
                    afterOutputs: [{ kind: "canvas", world }],
                  });
                  if (index === 8 && accept)
                    save(
                      `${label}-held`,
                      await host.presentation.capture(view),
                    );
                }
                await input.send({
                  kind: "pointerUp",
                  pointer: 1n,
                  point: point(kind === "slider" ? 0.8 : 0.56),
                });
                await root.flush();
                const heldValues = values.slice();
                check(
                  updates.length >= 8,
                  `${label}: sustained native reports kept flowing while acceptance waited`,
                );
                check(
                  values.length >= 8 && values.at(-1)! > 0.4,
                  `${label}: drag changed native value continuously`,
                );
                check(
                  values.every(
                    (value, index) =>
                      index === 0 || value >= values[index - 1]! - 1e-5,
                  ),
                  `${label}: native drag reversed: ${values.join(",")}`,
                );
                release();
                await Promise.all(updates);
                for (let count = 0; count < 30; count++) {
                  await root.flush();
                  await host.presentation.frame(view);
                  const fields = (await client.inspect()).entities
                    .find((item) => item.id === entity.id)!
                    .components.find(
                      (component) =>
                        component.component === client.components.GuiSlider!.id,
                    )!.fields;
                  if (Math.abs(Number(fields.value) - intent) < 1e-5) break;
                  check(
                    count < 29,
                    `${label}: settled controlled intent did not converge`,
                  );
                }
                const final = await host.presentation.capture(view);
                if (accept) {
                  const before = new Uint8Array(initial.pixels);
                  const after = new Uint8Array(final.pixels);
                  check(
                    after.filter(
                      (value, index) => Math.abs(value - before[index]!) > 20,
                    ).length > 40,
                    `${label}: rendered thumb/readout did not change`,
                  );
                } else
                  check(
                    intent === 0,
                    `${label}: rejection retained application value`,
                  );
                save(`${label}-settled`, final);
                report.push(
                  `${label}: ${heldValues.length} monotonic held samples, async convergence and completed pixels`,
                );
              } finally {
                release();
                await Promise.allSettled(updates);
                await watch.remove();
              }
            }
          }
        } catch (error) {
          const frame = await host.presentation
            .capture(view)
            .catch(() => undefined);
          if (frame) save(`${shape}-drag-failure`, frame);
          throw error;
        } finally {
          await input.close();
        }
      };
      if (present) await present(binding, section);
      else {
        const view = await host.presentation.select(
          await host.presentation.surface(),
          binding,
        );
        try {
          await section(view);
        } finally {
          await host.presentation.clear(view);
        }
      }
    }
    check(
      errors.length === 0,
      `Slider errors: ${errors.map((error) => error.message).join(", ")}`,
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
    release();
    await Promise.allSettled(updates);
    await cameraRoot.render(null);
    await cameraRoot.unmount();
    await root.render(null);
    await root.unmount();
    if (binding) await host.clearRootOutput(binding);
    await cameraClient.close();
    await client.close();
    await host.destroyWorld(parent);
    await host.destroyWorld(world);
  }
}
