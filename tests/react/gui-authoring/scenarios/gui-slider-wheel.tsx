/** Hover wheel policy through real transport, surface mapping and completed pixels. */
import { createRef } from "react";
import type { Client, HostClientBase, RootBinding } from "@ipp/client";
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
import type { GuiControlHandle } from "@ipp/react/gui";
import {
  Button,
  Column,
  GuiKit,
  Knob,
  LabelledSlider,
  RangeSlider,
  type GuiKitContract,
} from "@ipp/react/gui-kit";
import { cybertronSkin } from "@ipp/react/gui-cybertron";
import { glassSkin } from "@ipp/react/gui-glass";
import type { GuiContract } from "../pages/gui-authoring.js";
import type { WidgetPresentation } from "./gui-widgets.js";
import { check } from "../../../harness/page/checks.js";

export async function guiSliderWheel(
  host: HostClientBase<Client>,
  contract: GuiContract & GuiKitContract,
  font: Uint8Array<ArrayBuffer>,
  present?: WidgetPresentation,
) {
  const world = (
    await host.createWorld(
      guiWorldOptions({
        temporary: true,
        canvas: { extent: [320, 320], unitsPerMetre: 200 },
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
  const focus = createRef<GuiControlHandle>();
  const refs = {
    slider: createRef<GuiControlHandle>(),
    knob: createRef<GuiControlHandle>(),
    range: createRef<GuiControlHandle>(),
  };
  const images: {
    label: string;
    width: number;
    height: number;
    pixels: number[];
    sequence: bigint;
  }[] = [];
  const report: string[] = [];
  let binding: RootBinding | undefined;
  let slider = 5,
    knob = 5;
  let range: readonly [number, number] = [2, 8];
  let disabled = false;
  let skin = cybertronSkin(contract);
  const render = () =>
    root.render(
      <>
        <Asset
          id="wheel-font"
          kind={17}
          data={font}
          encode={(bytes: Uint8Array<ArrayBuffer>) => bytes}
        />
        <GuiKit
          skin={skin}
          contract={contract}
          font={assetRef("wheel-font")}
          fontSize={16}
        >
          <Column width={320} height={320} padding={16} gap={8}>
            <Button
              id="wheel-focus"
              ref={focus}
              label="Keyboard focus stays here"
            />
            <LabelledSlider
              id="wheel-slider"
              ref={refs.slider}
              label="Energy"
              min={0}
              max={10}
              step={1}
              fineStep={0.25}
              bounds={false}
              value={slider}
              disabled={disabled}
              onChange={(value) => {
                slider = value;
                return render();
              }}
            />
            <Knob
              id="wheel-knob"
              ref={refs.knob}
              label="Gain"
              size={48}
              min={0}
              max={10}
              step={1}
              fineStep={0.25}
              bounds={false}
              value={knob}
              onChange={(value) => {
                knob = value;
                return render();
              }}
            />
            <RangeSlider
              id="wheel-range"
              ref={refs.range}
              min={0}
              max={10}
              step={1}
              fineStep={0.25}
              bounds={false}
              value={range}
              onChange={(value) => {
                range = value;
                return render();
              }}
            />
          </Column>
        </GuiKit>
      </>,
    );
  try {
    for (const [name, selected] of [
      ["cybertron", cybertronSkin(contract)],
      ["glass", glassSkin(contract)],
    ] as const) {
      skin = selected;
      for (const shape of ["flat", "cylinder"] as const) {
        slider = knob = 5;
        range = [2, 8];
        disabled = false;
        await render();
        await cameraRoot.render(
          <>
            <Entity id="wheel-camera">
              <Transform z={2.5} />
              <Camera projection={0} fov_y={Math.PI / 4} near={0.05} far={20} />
            </Entity>
            <Entity id="wheel-panel">
              {shape === "flat" ? (
                <FlatSurface width={1.6} height={1.6} />
              ) : (
                <CylinderSurface width={1.6} height={1.6} curvature={0.8} />
              )}
            </Entity>
            <AttachedWorld
              anchor="wheel-panel"
              child={{ borrow: world }}
              attachment={{ mode: "surface-canvas" }}
            />
          </>,
        );
        if (!binding)
          binding = await host.setRootOutput(
            await cameraRoot.bindOutput("wheel-camera", "camera"),
            { width: 400, height: 400, devicePixelRatio: 1 },
          );
        const project = (content: readonly [number, number]) => {
          const x = content[0] / 200 - 0.8,
            y = 0.8 - content[1] / 200;
          const k = shape === "flat" ? 0 : 0.8;
          const horizontal = k === 0 ? x : Math.sin(k * x) / k;
          const z = k === 0 ? 0 : (Math.cos(k * x) - 1) / k;
          const half = (2.5 - z) * Math.tan(Math.PI / 8);
          return [0.5 + horizontal / (2 * half), 0.5 - y / (2 * half)] as const;
        };
        const section: Parameters<WidgetPresentation>[1] = async (view) => {
          const input = await host.input.open(view);
          const label = `${name}-${shape}-wheel`;
          try {
            for (let count = 0; count < 100; count++) {
              await root.flush();
              const state = await client.inspect();
              check(
                !state.resources.some(
                  (resource) => resource.status === "failed",
                ),
                `${label}: resource failed`,
              );
              if (
                state.resources.every(
                  (resource) => resource.status === "loaded",
                ) &&
                focus.current &&
                Object.values(refs).every((ref) => ref.current)
              )
                break;
              check(count < 99, `${label}: controls did not become ready`);
              await client.waitForFrame(state.tick);
            }
            await focus.current!.blur();
            await host.presentation.frame(view, {
              afterOutputs: [{ kind: "canvas", world }],
            });
            const rects: Record<string, number[]> = {};
            const inspected = await client.inspect();
            for (const kind of ["slider", "knob", "range"] as const) {
              const fields = inspected.entities
                .find(
                  (entity) => entity.id === refs[kind].current!.target.entity,
                )!
                .components.find(
                  (component) =>
                    component.component === client.components.CanvasBounds!.id,
                )!.fields;
              rects[kind] = ["x", "y", "width", "height"].map((field) =>
                Number(fields[field]),
              );
            }
            const point = (kind: string, fraction = 0.5) => {
              const [x, y, width, height] = rects[kind]!;
              const edge = height! * 0.75;
              return project([
                x! +
                  (kind === "range"
                    ? edge / 2 + (width! - edge) * fraction
                    : width! / 2),
                y! + height! / 2,
              ]);
            };
            const frame = async (suffix: string) => {
              await root.flush();
              const capture = await host.presentation.capture(view, {
                afterOutputs: [{ kind: "canvas", world }],
              });
              check(capture.drawCalls > 0, `${label}: controls render`);
              images.push({
                label: `${label}-${suffix}`,
                width: 400,
                height: 400,
                pixels: Array.from(new Uint8Array(capture.pixels)),
                sequence: capture.sequence,
              });
              return new Uint8Array(capture.pixels);
            };
            const before = await frame("before");
            const wheel = async (
              kind: string,
              delta: number,
              shift = false,
              fraction = 0.5,
            ) => {
              const outcome = await input.send({
                kind: "wheel",
                point: point(kind, fraction),
                delta: [0, delta],
                shift,
              });
              check(
                outcome.disposition === "routed" &&
                  !outcome.rejected &&
                  !outcome.cancelled &&
                  !outcome.error,
                `${label}: ${kind} wheel routed`,
              );
              check(
                outcome.remaining === undefined ||
                  outcome.remaining.every((value) => value === 0),
                `${label}: ${kind} leaked wheel to scene`,
              );
              await root.flush();
              await host.presentation.frame(view);
            };
            await wheel("slider", 1);
            check(
              Number((await refs.slider.current!.read()).value) === 4,
              `${label}: unfocused slider did not step`,
            );
            check(
              !(await client.inspectPage({ collection: "guiFocus" })).guiFocus
                ?.length,
              `${label}: hover wheel acquired keyboard focus`,
            );
            await focus.current!.focus();
            await wheel("slider", -0.125, true);
            check(
              Number((await refs.slider.current!.read()).value) === 4.25,
              `${label}: Shift fine slider step`,
            );
            await wheel("knob", -1);
            await wheel("knob", -1, true);
            check(
              Number((await refs.knob.current!.read()).value) === 6.25,
              `${label}: hovered dial steps`,
            );
            await wheel("range", 1, false, 0.8);
            await wheel("range", -1, true, 0.7);
            await wheel("range", -1, false, 0.2);
            const interval = await refs.range.current!.read();
            check(
              Number(interval.value) === 3 && Number(interval.upper) === 7.25,
              `${label}: wheel chose wrong range thumb`,
            );
            slider = 10;
            await render();
            for (let count = 0; count < 30; count++) {
              await root.flush();
              await host.presentation.frame(view);
              if (Number((await refs.slider.current!.read()).value) === 10)
                break;
              check(
                count < 29,
                `${label}: external bound intent did not commit`,
              );
            }
            await wheel("slider", -1);
            check(
              Number((await refs.slider.current!.read()).value) === 10,
              `${label}: bound changes value`,
            );
            check(
              (
                await client.inspectPage({ collection: "guiFocus" })
              ).guiFocus?.some(
                (entry) => entry.target.entity === focus.current!.target.entity,
              ),
              `${label}: wheel stole keyboard focus`,
            );
            const after = await frame("after");
            for (const kind of ["slider", "knob", "range"] as const) {
              const [x, y, width, height] = rects[kind]!;
              const first = project([x!, y!]),
                last = project([x! + width!, y! + height!]);
              let changed = 0;
              for (
                let row = Math.floor(first[1] * 400);
                row < last[1] * 400;
                row++
              )
                for (
                  let column = Math.floor(first[0] * 400);
                  column < last[0] * 400;
                  column++
                ) {
                  const at = (row * 400 + column) * 4;
                  if (
                    Math.abs(before[at]! - after[at]!) +
                      Math.abs(before[at + 1]! - after[at + 1]!) +
                      Math.abs(before[at + 2]! - after[at + 2]!) >
                    30
                  )
                    changed++;
                }
              check(
                changed > 15,
                `${label}: ${kind} pixels did not follow wheel`,
              );
            }
            disabled = true;
            await render();
            await host.presentation.frame(view, {
              afterOutputs: [{ kind: "canvas", world }],
            });
            const ignored = await input.send({
              kind: "wheel",
              point: point("slider"),
              delta: [0, 1],
            });
            check(
              ignored.disposition !== "routed" &&
                Number((await refs.slider.current!.read()).value) === 10,
              `${label}: disabled slider took wheel`,
            );
            report.push(
              `${label}: unfocused/focused hover, fine steps, range thumb, bound consumption, disabled control and pixels`,
            );
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
    }
    check(
      errors.length === 0,
      `Wheel errors: ${errors.map((error) => error.message).join(",")}`,
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
