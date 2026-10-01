import {
  canvasOutput,
  type Client,
  type HostClientBase,
  type PresentedCapture,
  type Command,
} from "@ipp/client";
import { createRef } from "react";
import {
  Entity,
  Children,
  Asset,
  CanvasWorld,
  assetRef,
  assetField,
  type CanvasWorldHandle,
} from "@ipp/react";
import {
  Style,
  Layout,
  Theme,
  Skin,
  Button,
  Text,
  Drawing,
  Image,
  Font,
  Box,
  VirtualList,
  type GuiControlHandle,
} from "@ipp/react/gui";
import { CanvasWorldSession } from "@ipp/react/web";
import { check, deferred, type GuiContract } from "./gui-authoring.js";
export { nativePresentationTransport } from "../../packages/ipp-client/src/native-presentation.js";
export { workerTransport } from "../../packages/ipp-client/src/worker.js";

export interface GuiPaintAssets {
  font: Uint8Array<ArrayBuffer>;
  drawing: Uint8Array<ArrayBuffer>;
  bitmap: Uint8Array<ArrayBuffer>;
}

function encoded(bytes: Uint8Array<ArrayBuffer>): Uint8Array<ArrayBuffer> {
  return bytes;
}

export async function guiPaint(
  host: HostClientBase<Client>,
  contract: GuiContract,
  assets: GuiPaintAssets,
) {
  const world = (
    await host.createWorld({
      selectedSystems: [
        "ipp.animation",
        "ipp.gui",
        "ipp.gui-layout",
        "ipp.canvas",
        "ipp.asset-dependencies",
        "ipp.lifecycle-publisher",
      ],
    })
  ).reference;
  const client = await host.openWorld(world);
  const writes: Command[][] = [];
  const batch = client.batch.bind(client);
  client.batch = async (commands) => {
    writes.push(commands);
    return batch(commands);
  };
  const errors: Error[] = [];
  const session = new CanvasWorldSession({
    host,
    client,
    onError: (error) => errors.push(error),
  });
  const root = session.createRoot();
  const press = createRef<GuiControlHandle>();
  const callbackPaint = deferred<Promise<void>>();
  const images: {
    label: string;
    width: number;
    height: number;
    pixels: number[];
    sequence: bigint;
  }[] = [];
  const part = contract.guiPaintPartIndex({ part: "background" });
  const skin = contract.GuiSkin.encodeParts({
    nextSlot: 1,
    rows: new Map([
      [
        0,
        { part, color: [0, 0, 1, 1], corner_radius: [0, 0], border_width: 0 },
      ],
    ]),
  });
  const labelParts = contract.GuiSkin.encodeParts({
    nextSlot: 6,
    rows: new Map([[5, { part, color: [1, 1, 1, 1] }]]),
  });
  function scene(width: number, green: boolean) {
    const theme = contract.GuiTheme.encodeParts({
      nextSlot: 2,
      rows: new Map([
        [
          1,
          {
            part,
            color: green ? [0, 1, 0, 1] : [1, 0, 0, 1],
            corner_radius: [0, 0],
            border_width: 0,
          },
        ],
      ]),
    });
    return (
      <>
        <Asset id="font" kind={17} data={assets.font} encode={encoded} />
        <Asset id="drawing" kind={18} data={assets.drawing} encode={encoded} />
        <Asset id="bitmap" kind={2} data={assets.bitmap} encode={encoded} />
        <Entity id="theme">
          <Theme parts={theme} />
        </Entity>
        <Entity id="canvas">
          <Layout
            kind={3}
            width={width}
            height={64}
            align_x={-1}
            align_y={-1}
          />
          <Children>
            <Entity id="background">
              <Layout width={width} height={64} align_x={-1} align_y={-1} />
              <Style red={0} green={0} blue={0} />
              <Box width={width} height={64} />
            </Entity>
            <Entity id="clip">
              <Layout
                kind={3}
                width={64}
                height={24}
                align_x={-1}
                align_y={-1}
              />
              <Style
                clipped
                clip_min_x={0}
                clip_min_y={0}
                clip_max_x={48}
                clip_max_y={24}
              />
              <Children>
                <Entity id="red">
                  <Layout width={32} height={24} align_x={-1} align_y={-1} />
                  <Skin theme="theme" />
                  <Button
                    label=""
                    ref={press}
                    onPress={() =>
                      callbackPaint.resolve(root.render(scene(width, true)))
                    }
                  />
                </Entity>
                <Entity id="blue">
                  <Layout width={32} height={24} align_x={-1} align_y={-1} />
                  <Style x={32} />
                  <Skin theme="theme" parts={skin} />
                  <Button label="" />
                </Entity>
              </Children>
            </Entity>
            <Entity id="text">
              <Layout width={24} height={20} align_x={-1} align_y={-1} />
              <Style y={32} />
              <Text text="M" source={assetRef("font")} font_size={18} />
            </Entity>
            <Entity id="label">
              <Layout width={24} height={20} align_x={-1} align_y={-1} />
              <Style x={24} y={32} />
              <Font source={assetRef("font")} font_size={12} />
              <Skin parts={labelParts} />
              <Skin
                fields={[
                  assetField(
                    contract.GuiSkin.partsOffset(5, "asset"),
                    assetRef("bitmap"),
                  ),
                ]}
              />
              <Button label="A" />
            </Entity>
            <Entity id="drawing">
              <Layout width={16} height={16} align_x={-1} align_y={-1} />
              <Style x={50} y={32} scale_x={0.5} scale_y={0.5} />
              <Drawing source={assetRef("drawing")} />
            </Entity>
            <Entity id="bitmap">
              <Layout width={16} height={16} align_x={-1} align_y={-1} />
              <Style x={72} y={36} />
              <Image source={assetRef("bitmap")} width={16} height={16} />
            </Entity>
          </Children>
        </Entity>
      </>
    );
  }
  function region(
    frame: PresentedCapture,
    bounds: readonly [number, number, number, number],
    expected: readonly [number, number, number],
  ): boolean {
    const width = frame.view.binding.viewport.width;
    const pixels = new Uint8Array(frame.pixels);
    for (let row = bounds[1]; row < bounds[3]; row++) {
      for (let column = bounds[0]; column < bounds[2]; column++) {
        const offset = (row * width + column) * 4;
        if (
          expected.some(
            (value, channel) => Math.abs(pixels[offset + channel]! - value) > 8,
          )
        )
          return false;
      }
    }
    return true;
  }
  async function capture(label: string, width: number, green: boolean) {
    const deadline = performance.now() + 20_000;
    let sequence: bigint | undefined;
    for (;;) {
      const frame = await session.capture(
        sequence === undefined ? {} : { afterSequence: sequence },
      );
      const matches =
        frame.view.binding.viewport.width === width &&
        region(frame, [4, 4, 28, 20], green ? [0, 255, 0] : [255, 0, 0]) &&
        region(frame, [36, 4, 44, 20], [0, 0, 255]) &&
        region(frame, [52, 4, width - 4, 20], [0, 0, 0]) &&
        coloredPixels(
          frame,
          [0, 32, 22, 54],
          (red, green, blue) => red > 160 && green > 160 && blue > 160,
        ) > 8 &&
        coloredPixels(
          frame,
          [24, 32, 48, 54],
          (red, green, blue) => red > 160 && green > 160 && blue > 160,
        ) > 3 &&
        coloredPixels(
          frame,
          [50, 32, 66, 46],
          (red, green, blue) => red > 150 && green > 100 && blue < 130,
        ) > 2 &&
        coloredPixels(
          frame,
          [74, 38, 86, 50],
          (red, green, blue) => red > 120 && green > 60 && blue < 100,
        ) > 30;
      if (matches) {
        images.push({
          label,
          width,
          height: 64,
          pixels: [...new Uint8Array(frame.pixels)],
          sequence: frame.sequence,
        });
        return frame;
      }
      if (performance.now() >= deadline) {
        images.push({
          label: `failed-${label}`,
          width,
          height: 64,
          pixels: [...new Uint8Array(frame.pixels)],
          sequence: frame.sequence,
        });
        throw new Error(
          `Independent GUI paint regions failed: ${label}; samples=${[8, 40, 52].map((column) => Array.from(new Uint8Array(frame.pixels).slice((8 * width + column) * 4, (8 * width + column) * 4 + 4)).join(",")).join(";")}`,
        );
      }
      sequence = frame.sequence;
    }
  }
  function coloredPixels(
    frame: PresentedCapture,
    bounds: readonly [number, number, number, number],
    matches: (red: number, green: number, blue: number) => boolean,
  ): number {
    const width = frame.view.binding.viewport.width;
    const pixels = new Uint8Array(frame.pixels);
    let count = 0;
    for (let row = bounds[1]; row < bounds[3]; row++) {
      for (let column = bounds[0]; column < bounds[2]; column++) {
        const offset = (row * width + column) * 4;
        if (matches(pixels[offset]!, pixels[offset + 1]!, pixels[offset + 2]!))
          count++;
      }
    }
    return count;
  }
  try {
    await root.render(scene(96, false));
    // The session's World selects the Canvas System, so it is the canvas.
    const output = canvasOutput(world);
    await session.selectOutput(output, {
      width: 96,
      height: 64,
      devicePixelRatio: 1,
    });
    await capture("theme-red-clip-blue-override", 96, false);
    const labelEntity = (await client.inspect()).entities.find(
      (entry) => entry.metadata.symbolicId === "label",
    );
    const labelTable = labelEntity?.components.find(
      (entry) => entry.component === contract.GuiSkin.id,
    )?.fields.parts;
    check(
      labelTable &&
        typeof labelTable === "object" &&
        "rows" in labelTable &&
        labelTable.rows.get(5)?.asset,
      `Generated row asset reference was not acknowledged: ${JSON.stringify({ labelTable, writes: writes.flat().filter((command) => (command.kind === "setField" || command.kind === "insertComponent") && command.component === contract.GuiSkin.id) }, (_, value) => (value instanceof Map ? [...value] : typeof value === "bigint" ? String(value) : value))}; errors=${errors.map((error) => error.message).join("; ")}`,
    );
    check(press.current, "Callback form omitted its acknowledged target");
    const pressed = await press.current.action({ kind: "press" });
    check(pressed.ok, "Callback form press rejected");
    await callbackPaint.promise;
    await capture("callback-theme-green-preserves-blue", 96, true);
    await root.render(scene(128, true));
    await session.selectOutput(output, {
      width: 128,
      height: 64,
      devicePixelRatio: 1,
    });
    await capture("resized-explicit-canvas", 128, true);
    const refs = Array.from({ length: 140 }, () =>
      createRef<GuiControlHandle>(),
    );
    const gridParts = (green: boolean) =>
      contract.GuiTheme.encodeParts({
        nextSlot: 1,
        rows: new Map([
          [
            0,
            {
              part,
              color: green ? [0, 1, 0, 1] : [1, 0, 0, 1],
              corner_radius: [0, 0],
              border_width: 0,
            },
          ],
        ]),
      });
    await root.render(
      <>
        <Entity id="red-theme">
          <Theme parts={gridParts(false)} />
        </Entity>
        <Entity id="green-theme">
          <Theme parts={gridParts(true)} />
        </Entity>
        <Entity id="grid">
          <Layout kind={3} width={56} height={40} align_x={-1} align_y={-1} />
          <Children>
            {refs.map((ref, index) => (
              <Entity key={index} id={`pixel-control-${index}`}>
                <Layout width={4} height={4} align_x={-1} align_y={-1} />
                <Style x={(index % 14) * 4} y={Math.floor(index / 14) * 4} />
                <Skin theme={index % 2 ? "green-theme" : "red-theme"} />
                <Button label="" ref={ref} />
              </Entity>
            ))}
          </Children>
        </Entity>
      </>,
    );
    check(
      refs.every((ref) => ref.current),
      "Large painted scene omitted acknowledged refs",
    );
    await session.selectOutput(output, {
      width: 56,
      height: 40,
      devicePixelRatio: 1,
    });
    const deadline = performance.now() + 20_000;
    let sequence: bigint | undefined;
    for (;;) {
      const frame = await session.capture(
        sequence === undefined ? {} : { afterSequence: sequence },
      );
      const matches = refs.every((_, index) => {
        const column = (index % 14) * 4;
        const row = Math.floor(index / 14) * 4;
        return region(
          frame,
          [column + 1, row + 1, column + 3, row + 3],
          index % 2 ? [0, 255, 0] : [255, 0, 0],
        );
      });
      if (matches || performance.now() >= deadline) {
        images.push({
          label: matches
            ? "140-indexed-control-refs"
            : "failed-140-indexed-control-refs",
          width: 56,
          height: 40,
          pixels: [...new Uint8Array(frame.pixels)],
          sequence: frame.sequence,
        });
        check(
          matches,
          "140 control refs did not produce independent alternating paint regions",
        );
        break;
      }
      sequence = frame.sequence;
    }
    const retained = refs[0]!.current!;
    const scroll = createRef<GuiControlHandle>();
    const listScene = (count: number) => (
      <Entity id="scroll-pixels">
        <Layout width={56} height={40} />
        <Skin
          parts={contract.GuiSkin.encodeParts({
            nextSlot: 1,
            rows: new Map([[0, { part, color: [0, 0, 0, 1] }]]),
          })}
        />
        <VirtualList
          ref={scroll}
          item_count={count}
          item_extent={8}
          overscan={1}
          renderItem={(index) => (
            <>
              <Layout width={56} height={8} />
              <Style
                red={index % 2 ? 0 : 1}
                green={index % 2 ? 1 : 0}
                blue={0}
              />
              <Box width={56} height={8} />
            </>
          )}
        />
      </Entity>
    );
    await root.render(listScene(100_000));
    await session.selectOutput(output, {
      width: 56,
      height: 40,
      devicePixelRatio: 1,
    });
    const listCapture = async (label: string, first: number, count: number) => {
      const deadline = performance.now() + 20_000;
      let sequence: bigint | undefined;
      for (;;) {
        const frame = await session.capture(
          sequence === undefined ? {} : { afterSequence: sequence },
        );
        const rowsMatch = Array.from({ length: 5 }, (_, row) => {
          const index = first + row;
          const color: [number, number, number] =
            index >= count ? [0, 0, 0] : index % 2 ? [0, 255, 0] : [255, 0, 0];
          return region(frame, [2, row * 8 + 2, 51, row * 8 + 6], color);
        }).every(Boolean);
        const thumbLength = Math.max(4, (40 * 40) / (count * 8));
        const thumbCenter =
          count > 5
            ? Math.floor(
                ((40 - thumbLength) * first * 8) / (count * 8 - 40) +
                  thumbLength / 2,
              )
            : 2;
        const thumbPixel = (thumbCenter * 56 + 55) * 4;
        const bar = new Uint8Array(frame.pixels).slice(
          thumbPixel,
          thumbPixel + 3,
        );
        const matches =
          rowsMatch &&
          (count <= 5 ||
            (bar[0]! > 40 &&
              Math.abs(bar[0]! - bar[1]!) < 8 &&
              Math.abs(bar[1]! - bar[2]!) < 8));
        if (matches || performance.now() >= deadline) {
          images.push({
            label: matches ? label : `failed-${label}`,
            width: 56,
            height: 40,
            pixels: [...new Uint8Array(frame.pixels)],
            sequence: frame.sequence,
          });
          check(
            matches,
            `Ordinary virtual item placement/clip pixels failed: ${label}`,
          );
          return;
        }
        sequence = frame.sequence;
      }
    };
    await listCapture("virtual-list-initial-range", 0, 100_000);
    check(scroll.current, "Painted virtual list omitted acknowledged ref");
    check(
      (await scroll.current.action({ kind: "scrollToIndex", index: 99_991 }))
        .ok,
      "Painted virtual list scroll-to-index failed",
    );
    await listCapture("virtual-list-distant-ordinary-items", 99_991, 100_000);
    await root.render(listScene(4));
    await listCapture("virtual-list-shrink-clip", 0, 4);
    await root.render(listScene(100_000));
    await listCapture("virtual-list-regrow-keeps-clamped-anchor", 0, 100_000);
    // An omitted IppCanvas output presents the root CanvasWorld's canvas;
    // an explicit one would conflict with it.
    await session.selectOutput(undefined, {
      width: 56,
      height: 40,
      devicePixelRatio: 1,
    });
    const panel = createRef<CanvasWorldHandle>();
    await root.render(
      <CanvasWorld
        create={{
          selectedSystems: ["ipp.canvas", "ipp.gui", "ipp.gui-layout"],
        }}
        extent={[56, 40]}
        presentation={{ root: true }}
        ref={panel}
      >
        <Entity id="owned-panel">
          <Layout width={56} height={40} align_x={-1} align_y={-1} />
          <Style red={0} green={0} blue={1} />
          <Box width={56} height={40} />
        </Entity>
      </CanvasWorld>,
    );
    const readyBy = performance.now() + 20_000;
    while (!panel.current && performance.now() < readyBy)
      await new Promise<void>((resolve) => setTimeout(resolve, 16));
    const owned = panel.current;
    check(owned, "Root CanvasWorld did not become ready");
    const ownedWorld = owned.world;
    check(
      owned.output.kind === "canvas" &&
        owned.output.world.id === ownedWorld.id &&
        owned.output.world.incarnation === ownedWorld.incarnation,
      "Root CanvasWorld did not report its World's canvas",
    );
    while (
      session.view?.binding.output.world.id !== ownedWorld.id &&
      performance.now() < readyBy
    )
      await new Promise<void>((resolve) => setTimeout(resolve, 16));
    check(
      session.view?.binding.output.world.id === ownedWorld.id,
      "The Canvas root did not present the root CanvasWorld",
    );
    const ownedBy = performance.now() + 20_000;
    let ownedSequence: bigint | undefined;
    for (;;) {
      const frame = await session.capture(
        ownedSequence === undefined ? {} : { afterSequence: ownedSequence },
      );
      const matches =
        frame.view.binding.output.world.id === ownedWorld.id &&
        region(frame, [2, 2, 54, 38], [0, 0, 255]);
      if (matches || performance.now() >= ownedBy) {
        images.push({
          label: matches ? "canvas-world-root" : "failed-canvas-world-root",
          width: 56,
          height: 40,
          pixels: [...new Uint8Array(frame.pixels)],
          sequence: frame.sequence,
        });
        check(matches, "Root CanvasWorld did not paint its own World");
        break;
      }
      ownedSequence = frame.sequence;
    }
    const ownedClosed = owned.closed;
    await root.render(null);
    await ownedClosed;
    check(
      !(await host.listWorlds()).some((item) => item.id === ownedWorld.id),
      "Removing the root CanvasWorld left its World",
    );
    await root.unmount();
    check(
      refs.every((ref) => ref.current === null),
      "Painted refs survived unmount",
    );
    await retained.read().then(
      () => {
        throw new Error("Painted retired handle remained live");
      },
      () => {},
    );
    check(
      errors.length === 0,
      `GUI paint root errors: ${errors.map((error) => error.message).join(", ")}`,
    );
    return {
      images,
      assertions: [
        "shared theme",
        "observed press callback reauthors theme before completed pixels",
        "per-control precedence",
        "layout",
        "clip",
        "resize",
        "raw font/text",
        "drawing",
        "bitmap",
        "generated row asset references",
        "140 live indexed refs with independent painted regions and exact cleanup",
        "root CanvasWorld presents and destroys its own World",
      ],
      failure: null,
    };
  } catch (error) {
    return {
      images,
      assertions: [],
      failure: error instanceof Error ? error.message : String(error),
    };
  } finally {
    await session.close();
    await client.close();
    await host.destroyWorld(world);
  }
}
