/** A 100000-item VirtualList through a generated client, independent of
 * process launch and wire layout.
 *
 * The scenario declares children only for the ranges the runtime publishes,
 * with measured extents that differ from the estimate, and checks the
 * published ranges, offsets, anchors and scroll capacity against an
 * independent model of item positions: wheel scrolling without the client,
 * anchoring when measurements above the viewport change, convergence once
 * the range is declared, scroll-to-index, and a restored World republishing
 * its range from the persisted anchor. With a frame capture it also checks
 * the completed frames: the list clips its items below the header and the
 * scroll bar thumb follows the offset over the capacity.
 */
import type {
  GuiEdit,
  GuiSemanticNode,
  GuiVirtualRangeChangedEffect,
  WorldPersistenceHostClient,
} from "@ipp/client";
import { aliasId, createEntity, insertComponent } from "../camera-fixtures.js";
import {
  activatePanelCamera,
  changedPixels,
  panelViewportPoint,
  settledFrame,
  type GuiFrame,
  type GuiFrameCapture,
  type GuiTestClient,
} from "./gui-lifecycle.js";

function expect(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

/** Items, estimate and overscan of the list under test. */
const COUNT = 100_000;
const ESTIMATE = 0.25;
const OVERSCAN = 2;
/** The list is 4 x 2 below a 4 x 1 header on the 4 x 3 panel. */
const VIEWPORT = 2;
const LIST = 3;

/** Declared item height: every third item is twice the estimate. */
function height(index: number): number {
  return index % 3 === 0 ? 0.5 : 0.25;
}

/** Independent model of item positions: the estimate for every item, with
 * each declared item's measured extent replacing it. */
function position(declared: ReadonlyMap<number, number>, index: number) {
  let at = index * ESTIMATE;
  for (const item of declared.keys())
    if (item < index) at += height(item) - ESTIMATE;
  return at;
}

function contentExtent(declared: ReadonlyMap<number, number>): number {
  return position(declared, COUNT);
}

/** Wanted range of the model at a main-axis offset. */
function wanted(
  declared: ReadonlyMap<number, number>,
  offset: number,
): [number, number] {
  const itemAt = (at: number) => {
    let low = 0;
    let high = COUNT - 1;
    while (low < high) {
      const middle = Math.ceil((low + high) / 2);
      if (position(declared, middle) <= at) low = middle;
      else high = middle - 1;
    }
    return low;
  };
  const first = itemAt(offset);
  let last = itemAt(offset + VIEWPORT);
  if (last > first && position(declared, last) >= offset + VIEWPORT) last -= 1;
  return [Math.max(0, first - OVERSCAN), Math.min(COUNT, last + 1 + OVERSCAN)];
}

const close = (a: number, b: number, tolerance = 1e-3) =>
  Math.abs(a - b) <= tolerance;

/** Linear sRGB-ish luminance of one frame pixel at a panel logical point. */
function luminance(frame: GuiFrame, point: [number, number]): number {
  const [nx, ny] = panelViewportPoint(frame, point);
  const x = Math.min(frame.width - 1, Math.floor(nx * frame.width));
  const y = Math.min(frame.height - 1, Math.floor(ny * frame.height));
  const pixels = new Uint8Array(frame.pixels);
  const offset = (y * frame.width + x) * 4;
  return pixels[offset]! + pixels[offset + 1]! + pixels[offset + 2]!;
}

/** Pixels inside a logical rectangle that differ between two frames. */
function changedIn(
  a: GuiFrame,
  b: GuiFrame,
  [left, top, right, bottom]: [number, number, number, number],
): number {
  const [x0, y0] = panelViewportPoint(a, [left, top]);
  const [x1, y1] = panelViewportPoint(a, [right, bottom]);
  const crop = (frame: GuiFrame) => {
    const width = Math.floor((x1 - x0) * frame.width);
    const height = Math.floor((y1 - y0) * frame.height);
    const source = new Uint8Array(frame.pixels);
    const pixels = new Uint8Array(width * height * 4);
    for (let row = 0; row < height; row += 1) {
      const from =
        ((Math.floor(y0 * frame.height) + row) * frame.width +
          Math.floor(x0 * frame.width)) *
        4;
      pixels.set(source.subarray(from, from + width * 4), row * width * 4);
    }
    return { width, height, pixels: pixels.buffer as ArrayBuffer };
  };
  return changedPixels(crop(a), crop(b));
}

/**
 * Exercise a 100000-item VirtualList: ranges, anchoring, scrolling,
 * scroll-to-index and restore, with completed-frame checks when `capture`
 * is supplied.
 */
export async function exerciseGuiVirtualList(
  host: WorldPersistenceHostClient<GuiTestClient>,
  capture?: GuiFrameCapture,
) {
  const client = await host.createWorld({ symbolicId: "gui-virtual-list" });
  const ref = { kind: "alias", alias: 1 } as const;
  const entity = aliasId(
    await client.batch([
      createEntity(1, "gui-virtual-panel"),
      insertComponent(client, "Transform", ref),
      insertComponent(client, "Surface", ref, { width: 4, height: 3 }),
      insertComponent(client, "GuiRoot", ref),
    ]),
    1,
  );
  const { rootIncarnation } = await client.inspectGui({ entity });
  const ranges: GuiVirtualRangeChangedEffect[] = [];
  const stop = client.subscribeGuiObservations((batch) => {
    for (const range of batch.virtualRanges ?? [])
      if (range.entity === entity && range.node === LIST) ranges.push(range);
  });
  await client.editGuiBatch([
    {
      action: "insert",
      entity,
      rootIncarnation,
      id: 1,
      index: 0,
      data: { kind: "container", containerKind: "column" },
      style: { width: 4, height: 3 },
    },
    {
      action: "insert",
      entity,
      rootIncarnation,
      id: 2,
      parent: 1,
      index: 0,
      data: { kind: "container", containerKind: "sizedBox" },
      style: { width: 4, height: 1, backgroundColor: [0.8, 0.1, 0.1, 1] },
    },
    {
      action: "insert",
      entity,
      rootIncarnation,
      id: LIST,
      parent: 1,
      index: 1,
      data: { kind: "container", containerKind: "virtualList" },
      values: {
        itemCount: COUNT,
        itemExtent: ESTIMATE,
        overscan: OVERSCAN,
        axis: 1,
        anchorIndex: 0,
        anchorOffset: 0,
      },
      style: {
        width: 4,
        height: VIEWPORT,
        backgroundColor: [0.05, 0.05, 0.05, 1],
      },
    },
  ]);
  if (capture) await activatePanelCamera(client);
  const handle = (node: number) =>
    client.createGuiNodeHandle(entity, rootIncarnation, node);

  const latest = async (after = 0): Promise<GuiVirtualRangeChangedEffect> => {
    const deadline = Date.now() + 10_000;
    for (;;) {
      const range = ranges.at(-1);
      if (range !== undefined && range.revision > after) return range;
      expect(Date.now() < deadline, "The VirtualList published no range");
      await client.waitForFrame();
    }
  };

  // Attach publishes the visible eight estimated items plus the overscan.
  const attached = await latest();
  expect(
    attached.first === 0 && attached.last === 10 && attached.revision === 1,
    `Attach range ${JSON.stringify([attached.first, attached.last, attached.revision])}`,
  );

  // Declare exactly the wanted range, keyed by index, until it stops moving.
  const declared = new Map<number, number>();
  let nextId = LIST + 1;
  let declarations = 0;
  const converge = async (): Promise<GuiVirtualRangeChangedEffect> => {
    for (let round = 0; round < 20; round += 1) {
      await client.waitForFrame();
      await client.waitForFrame();
      const range = await latest();
      const edits: GuiEdit[] = [];
      for (const [index, node] of declared)
        if (index < range.first || index >= range.last) {
          edits.push({ action: "remove", handle: handle(node) });
          declared.delete(index);
        }
      for (let index = range.first; index < range.last; index += 1) {
        if (declared.has(index)) continue;
        declared.set(index, nextId);
        edits.push({
          action: "insert",
          entity,
          rootIncarnation,
          id: nextId,
          parent: LIST,
          index,
          data: { kind: "container", containerKind: "sizedBox" },
          style: {
            width: 4,
            height: height(index),
            backgroundColor:
              index % 2 === 0 ? [0.1, 0.7, 0.1, 1] : [0.1, 0.1, 0.8, 1],
          },
        });
        nextId += 1;
      }
      if (edits.length === 0) return range;
      declarations += edits.length;
      await client.editGuiBatch(edits);
    }
    throw new Error("The VirtualList range did not converge");
  };

  const list = async (): Promise<GuiSemanticNode> => {
    const node = (await client.semanticSnapshot({ entity })).nodes.find(
      (item) => item.id === LIST,
    );
    expect(node?.scroll && node.virtualList, "The list has no semantics");
    return node;
  };
  const settled = async (check: (node: GuiSemanticNode) => boolean) => {
    const deadline = Date.now() + 10_000;
    for (;;) {
      const node = await list();
      if (check(node)) return node;
      expect(
        Date.now() < deadline,
        `The list did not settle: ${JSON.stringify(node)}`,
      );
      await client.waitForFrame();
    }
  };

  const initial = await converge();
  const initialRange = wanted(declared, 0);
  expect(
    initial.first === initialRange[0] &&
      initial.last === initialRange[1] &&
      declared.size <= 14,
    `Initial range ${JSON.stringify([initial.first, initial.last])} is not ${JSON.stringify(initialRange)}`,
  );
  const top = await settled(
    (node) =>
      node.virtualList!.loadedFirst === initial.first &&
      node.virtualList!.loadedLast === initial.last,
  );
  expect(
    top.scroll!.offset[1] === 0 &&
      close(top.scroll!.maxOffset[1], contentExtent(declared) - VIEWPORT) &&
      top.virtualList!.itemCount === COUNT,
    `Initial scroll ${JSON.stringify(top)}`,
  );
  const topFrame = capture ? await settledFrame(client, capture) : undefined;
  const point = (logical: [number, number]): [number, number] =>
    topFrame ? panelViewportPoint(topFrame, logical) : logical;

  // A wheel over the list scrolls it from the count alone: no declaration
  // gates the offset, and the anchor names the item under the new top.
  const wheeled = await client.submitGuiInput({
    kind: "scroll",
    position: point([2, 2]),
    delta: [0, 10],
  });
  expect(wheeled.unhandled === undefined, "The wheel reached no list");
  const scrolled = await settled((node) => close(node.scroll!.offset[1], 10));
  const anchor = scrolled.virtualList!;
  expect(
    close(position(declared, anchor.anchorIndex) + anchor.anchorOffset, 10) &&
      anchor.anchorOffset >= 0 &&
      anchor.anchorOffset < height(anchor.anchorIndex) + 1e-3,
    `Anchor ${JSON.stringify(anchor)} does not name offset 10`,
  );

  // Declaring the new range measures items above the viewport and drops
  // the old window: the offset follows the anchored item, which stays put.
  const moved = await converge();
  const anchored = await settled(
    (node) =>
      node.virtualList!.loadedFirst === moved.first &&
      node.virtualList!.loadedLast === moved.last,
  );
  const offset = position(declared, anchor.anchorIndex) + anchor.anchorOffset;
  expect(
    anchored.virtualList!.anchorIndex === anchor.anchorIndex &&
      close(anchored.virtualList!.anchorOffset, anchor.anchorOffset) &&
      close(anchored.scroll!.offset[1], offset) &&
      close(anchored.scroll!.maxOffset[1], contentExtent(declared) - VIEWPORT),
    `Anchored scroll ${JSON.stringify(anchored)} is not offset ${offset}`,
  );
  const movedRange = wanted(declared, offset);
  expect(
    moved.first === movedRange[0] && moved.last === movedRange[1],
    `Scrolled range ${JSON.stringify([moved.first, moved.last])} is not ${JSON.stringify(movedRange)}`,
  );

  // Scroll-to-index anchors the middle item at the top; the thumb then sits
  // half way along its travel.
  await client.editGui({
    action: "scrollToIndex",
    handle: handle(LIST),
    index: COUNT / 2,
    offset: 0,
  });
  const middle = await converge();
  const centred = await settled(
    (node) =>
      node.virtualList!.anchorIndex === COUNT / 2 &&
      node.virtualList!.loadedFirst === middle.first,
  );
  const middleOffset = position(declared, COUNT / 2);
  expect(
    close(centred.scroll!.offset[1], middleOffset, 0.02) &&
      close(
        centred.scroll!.offset[1] / centred.scroll!.maxOffset[1],
        0.5,
        0.01,
      ) &&
      middle.first === COUNT / 2 - OVERSCAN,
    `Scroll-to-index ${JSON.stringify(centred)} is not at ${middleOffset}`,
  );

  // Frames: the header never shows list content, the list shows scrolled
  // items, and the thumb moved from the top of the track to its middle.
  let frames: Record<string, number> | undefined;
  if (capture && topFrame) {
    const middleFrame = await settledFrame(client, capture);
    const header = changedIn(topFrame, middleFrame, [0.1, 0.1, 3.8, 0.9]);
    const content = changedPixels(topFrame, middleFrame);
    const thumbTop = [
      luminance(topFrame, [3.95, 1.05]),
      luminance(middleFrame, [3.95, 1.05]),
    ];
    const thumbMiddle = [
      luminance(topFrame, [3.95, 2]),
      luminance(middleFrame, [3.95, 2]),
    ];
    expect(
      header === 0 &&
        content > 100 &&
        thumbTop[0]! > thumbTop[1]! + 60 &&
        thumbMiddle[1]! > thumbMiddle[0]! + 60,
      `VirtualList frames: ${JSON.stringify({ header, content, thumbTop, thumbMiddle })}`,
    );
    frames = {
      header,
      content,
      thumbTopBefore: thumbTop[0]!,
      thumbTopAfter: thumbTop[1]!,
      thumbMiddleBefore: thumbMiddle[0]!,
      thumbMiddleAfter: thumbMiddle[1]!,
    };
  }
  stop();

  // A restored World republishes the range from its persisted anchor.
  const bytes = await host.saveWorld();
  await host.detachWorld();
  const restored = await host.loadWorld(bytes, {
    symbolicId: "gui-virtual-list-restored",
  });
  const restoredRanges: GuiVirtualRangeChangedEffect[] = [];
  const restoredStop = restored.subscribeGuiObservations((batch) => {
    for (const range of batch.virtualRanges ?? [])
      if (range.node === LIST) restoredRanges.push(range);
  });
  const panel = (await restored.inspect()).entities.find(
    (item) => item.metadata.symbolicId === "gui-virtual-panel",
  );
  expect(panel, "Restored World omitted the VirtualList panel");
  const deadline = Date.now() + 10_000;
  while (restoredRanges.length === 0) {
    expect(Date.now() < deadline, "The restored list published no range");
    await restored.waitForFrame();
  }
  restoredStop();
  const republished = restoredRanges.at(-1)!;
  const restoredList = (
    await restored.semanticSnapshot({ entity: panel.id })
  ).nodes.find((item) => item.id === LIST);
  expect(
    republished.entity === panel.id &&
      republished.first === middle.first &&
      republished.last === middle.last &&
      restoredList?.virtualList?.anchorIndex === COUNT / 2 &&
      close(restoredList.scroll?.offset[1] ?? -1, centred.scroll!.offset[1]),
    `Restored range ${JSON.stringify([republished.first, republished.last])} or anchor ${JSON.stringify(restoredList?.virtualList)} differs`,
  );
  await host.detachWorld();

  return {
    attached: [attached.first, attached.last],
    initial: [initial.first, initial.last],
    scrolled: [moved.first, moved.last],
    middle: [middle.first, middle.last],
    anchor: [anchor.anchorIndex, anchor.anchorOffset],
    declarations,
    declaredAtOnce: declared.size,
    restored: [republished.first, republished.last],
    frames: frames ?? "no presentation in this environment",
  };
}
