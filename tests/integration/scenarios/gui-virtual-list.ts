/** A 100000-item VirtualList through a generated client, independent of
 * process launch and wire layout.
 *
 * The scenario declares ordinary item entities only for the ranges the
 * runtime publishes, with measured extents that differ from the estimate,
 * and checks the published ranges, offsets, anchors and scroll capacity
 * against an independent model of item positions: physical wheel scrolling
 * without the client, anchoring when measurements above the viewport change,
 * convergence once the range is declared, semantic scroll-to-index, and a
 * restored World republishing its range from the persisted anchor. Completed
 * frames check that the list clips its items below the header and that the
 * scroll bar thumb follows the offset over the capacity.
 */
import type { Client, Command, WorldReference } from "@ipp/client";
import { guiAction } from "../gui-actions.js";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../camera-fixtures.js";
import {
  alias,
  applied,
  changedPixels,
  check,
  cleanup,
  control,
  encoded,
  entitiesByName,
  handle,
  LAYOUT,
  named,
  openGui,
  PANEL_CANVAS,
  pixel,
  place,
  presentGui,
  routed,
  type GuiFrame,
  type GuiControlState,
  type GuiHost,
  type GuiTestClient,
  type PresentedGui,
} from "./gui-lifecycle.js";
import { LIFECYCLE, GUI, selectSystems } from "../system-selections.js";

/** Items, estimate and overscan of the list under test. */
const COUNT = 100_000;
const ESTIMATE = 0.25;
const OVERSCAN = 2;
/** The list is 4 x 2 below a 4 x 1 header on the 4 x 3 panel. */
const VIEWPORT = 2;

/** Declared item height: every third item is twice the estimate. */
function height(index: number): number {
  return index % 3 === 0 ? 0.5 : 0.25;
}

/** Independent model of item positions: the estimate for every item, with
 * each declared item's measured extent replacing it. */
function position(declared: ReadonlyMap<number, bigint>, index: number) {
  let at = index * ESTIMATE;
  for (const item of declared.keys())
    if (item < index) at += height(item) - ESTIMATE;
  return at;
}

function contentExtent(declared: ReadonlyMap<number, bigint>): number {
  return position(declared, COUNT);
}

/** Wanted range of the model at a main-axis offset. */
function wanted(
  declared: ReadonlyMap<number, bigint>,
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

/** Committed scroll value and evaluated view of the list. */
interface ListState {
  readonly snapshot: GuiControlState;
  readonly view: NonNullable<GuiControlState["scroll"]>;
  readonly offset: number;
  readonly anchorIndex: number;
  readonly anchorOffset: number;
}

async function listState(
  client: GuiTestClient,
  entity: bigint,
): Promise<ListState> {
  const snapshot = await control(client, entity);
  check(
    snapshot.kind === "virtualList" &&
      snapshot.scroll !== null &&
      snapshot.value.kind === "scroll",
    `The list has no virtual scroll semantics: ${encoded(snapshot)}`,
  );
  return {
    snapshot,
    view: snapshot.scroll,
    offset: snapshot.value.offset[1],
    anchorIndex: snapshot.value.anchorIndex,
    anchorOffset: snapshot.value.anchorOffset,
  };
}

/** Summed colour channels of one frame pixel at a panel logical point. */
function luminance(
  presentation: PresentedGui,
  frame: GuiFrame,
  point: [number, number],
): number {
  const [r, g, b] = pixel(frame, presentation.point(0, point));
  return r + g + b;
}

/** Pixels inside a logical rectangle that differ between two frames. */
function changedIn(
  presentation: PresentedGui,
  a: GuiFrame,
  b: GuiFrame,
  [left, top, right, bottom]: [number, number, number, number],
): number {
  const [x0, y0] = presentation.point(0, [left, top]);
  const [x1, y1] = presentation.point(0, [right, bottom]);
  const crop = (frame: GuiFrame): GuiFrame => {
    const width = Math.floor((x1 - x0) * frame.width);
    const height = Math.floor((y1 - y0) * frame.height);
    const pixels = new Uint8Array(width * height * 4);
    for (let row = 0; row < height; row += 1) {
      const from =
        ((Math.floor(y0 * frame.height) + row) * frame.width +
          Math.floor(x0 * frame.width)) *
        4;
      pixels.set(
        frame.pixels.subarray(from, from + width * 4),
        row * width * 4,
      );
    }
    return { width, height, pixels };
  };
  return changedPixels(crop(a), crop(b));
}

/**
 * Exercise a presented 100000-item VirtualList: ranges, anchoring, physical
 * wheel scrolling, semantic scroll-to-index, completed frames and restore.
 */
export async function exerciseGuiVirtualList(host: GuiHost) {
  const worlds: WorldReference[] = [];
  const sessions: Client[] = [];
  let presentation: PresentedGui | undefined;
  let completed = false;
  try {
    const created = await host.createWorld({
      selectedSystems: selectSystems(GUI, LIFECYCLE),
      symbolicId: "gui-virtual-list",
      canvas: PANEL_CANVAS,
    });
    worlds.push(created.reference);
    const client = await openGui(host, created.reference);
    sessions.push(client);
    const outcome = successfulBatch(
      await client.batch([
        createEntity(1, "gui-virtual-panel"),
        insertComponent(client, "GuiLayout", alias(1), {
          kind: LAYOUT.column,
          width: 4,
          height: 3,
        }),
        createEntity(2, "gui-virtual-header"),
        insertComponent(client, "GuiLayout", alias(2), {
          kind: LAYOUT.sizedBox,
          width: 4,
          height: 1,
        }),
        insertComponent(client, "CanvasBox", alias(2), { width: 4, height: 1 }),
        insertComponent(client, "CanvasStyle", alias(2), {
          red: 0.8,
          green: 0.1,
          blue: 0.1,
        }),
        place(alias(2), alias(1)),
        createEntity(3, "gui-virtual-list"),
        // A 0.1-unit bar flush with the list's right side and ends.
        insertComponent(client, "GuiVirtualList", alias(3), {
          item_count: COUNT,
          item_extent: ESTIMATE,
          overscan: OVERSCAN,
          axis: 1,
          bar_thickness: 0.1,
          bar_inset: 0,
          bar_end_inset: 0,
        }),
        insertComponent(client, "GuiLayout", alias(3), {
          width: 4,
          height: VIEWPORT,
        }),
        place(alias(3), alias(1)),
      ]),
    );
    const list = aliasId(outcome, 3);
    presentation = await presentGui(host, [{ child: created.reference }]);
    const p = presentation;

    const settled = async (
      accept: (state: ListState) => boolean,
      observer: GuiTestClient = client,
      entity: bigint = list,
    ): Promise<ListState> => {
      const deadline = Date.now() + 10_000;
      for (;;) {
        await observer.waitForFrame();
        await p.frame();
        const state = await listState(observer, entity);
        if (accept(state)) return state;
        check(
          Date.now() < deadline,
          `The list did not settle: ${encoded(state)}`,
        );
      }
    };

    // Attach publishes the visible eight estimated items plus the overscan.
    const attached = await settled((state) => state.view.last > 0);
    check(
      attached.view.first === 0 &&
        attached.view.last === 10 &&
        attached.view.itemCount === COUNT,
      `Attach range ${encoded(attached.view)}`,
    );

    // Declare exactly the wanted range, keyed by index, until it stops
    // moving.
    const declared = new Map<number, bigint>();
    let declarations = 0;
    const converge = async (): Promise<ListState> => {
      for (let round = 0; round < 20; round += 1) {
        const state = await settled(() => true);
        const { first, last } = state.view;
        const removed: Command[] = [];
        for (const [index, entity] of declared)
          if (index < first || index >= last) {
            removed.push({ kind: "delete", entity: handle(entity) });
            declared.delete(index);
          }
        const added: Command[] = [];
        const indices: number[] = [];
        for (let index = first; index < last; index += 1) {
          if (declared.has(index)) continue;
          const at = indices.length + 1;
          indices.push(index);
          added.push(
            createEntity(at, `gui-virtual-item-${index}`),
            insertComponent(client, "GuiVirtualItem", alias(at), { index }),
            insertComponent(client, "GuiLayout", alias(at), {
              width: 4,
              height: height(index),
            }),
            insertComponent(client, "CanvasBox", alias(at), {
              width: 4,
              height: height(index),
            }),
            insertComponent(
              client,
              "CanvasStyle",
              alias(at),
              index % 2 === 0
                ? { red: 0.1, green: 0.7, blue: 0.1 }
                : { red: 0.1, green: 0.1, blue: 0.8 },
            ),
            place(alias(at), handle(list)),
          );
        }
        if (removed.length === 0 && added.length === 0) return state;
        declarations += removed.length + indices.length;
        const result = successfulBatch(
          await client.batch([...removed, ...added]),
        );
        indices.forEach((index, offset) =>
          declared.set(index, aliasId(result, offset + 1)),
        );
      }
      throw new Error("The VirtualList range did not converge");
    };

    const initial = await converge();
    const initialRange = wanted(declared, 0);
    check(
      initial.view.first === initialRange[0] &&
        initial.view.last === initialRange[1] &&
        declared.size <= 14,
      `Initial range ${encoded([initial.view.first, initial.view.last])} is not ${encoded(initialRange)}`,
    );
    check(
      initial.offset === 0 &&
        close(initial.view.capacity[1], contentExtent(declared) - VIEWPORT) &&
        initial.view.itemCount === COUNT,
      `Initial scroll ${encoded(initial)}`,
    );
    const topFrame = await p.settled();

    // A wheel over the list scrolls it from the count alone: no declaration
    // gates the offset, and the anchor names the item under the new top.
    const wheeled = await p.send({
      kind: "wheel",
      point: p.point(0, [2, 2]),
      delta: [0, 10],
    });
    check(routed(wheeled), `The wheel reached no list: ${encoded(wheeled)}`);
    const scrolled = await settled((state) => close(state.offset, 10));
    const anchor = {
      index: scrolled.anchorIndex,
      offset: scrolled.anchorOffset,
    };
    check(
      close(position(declared, anchor.index) + anchor.offset, 10) &&
        anchor.offset >= 0 &&
        anchor.offset < height(anchor.index) + 1e-3,
      `Anchor ${encoded(anchor)} does not name offset 10`,
    );

    // Declaring the new range measures items above the viewport and drops
    // the old window: the offset follows the anchored item, which stays put.
    const moved = await converge();
    const offset = position(declared, anchor.index) + anchor.offset;
    check(
      moved.anchorIndex === anchor.index &&
        close(moved.anchorOffset, anchor.offset) &&
        close(moved.offset, offset) &&
        close(moved.view.capacity[1], contentExtent(declared) - VIEWPORT),
      `Anchored scroll ${encoded(moved)} is not offset ${offset}`,
    );
    const movedRange = wanted(declared, offset);
    check(
      moved.view.first === movedRange[0] && moved.view.last === movedRange[1],
      `Scrolled range ${encoded([moved.view.first, moved.view.last])} is not ${encoded(movedRange)}`,
    );

    // Scroll-to-index anchors the middle item at the top; the thumb then
    // sits half way along its travel.
    applied(
      await guiAction(client, moved.snapshot.target, {
        kind: "scrollToIndex",
        index: COUNT / 2,
        offset: 0,
      }),
    );
    const middle = await converge();
    const middleOffset = position(declared, COUNT / 2);
    check(
      middle.anchorIndex === COUNT / 2 &&
        close(middle.offset, middleOffset, 0.02) &&
        close(middle.offset / middle.view.capacity[1], 0.5, 0.01) &&
        middle.view.first === COUNT / 2 - OVERSCAN,
      `Scroll-to-index ${encoded(middle)} is not at ${middleOffset}`,
    );

    // Frames: the header never shows list content, the list shows scrolled
    // items, and the thumb moved from the top of its travel, below the
    // track's pointed end, to its middle.
    const middleFrame = await p.settled();
    const header = changedIn(p, topFrame, middleFrame, [0.1, 0.1, 3.8, 0.9]);
    const content = changedPixels(topFrame, middleFrame);
    const thumbTop = [
      luminance(p, topFrame, [3.95, 1.15]),
      luminance(p, middleFrame, [3.95, 1.15]),
    ];
    const thumbMiddle = [
      luminance(p, topFrame, [3.95, 2]),
      luminance(p, middleFrame, [3.95, 2]),
    ];
    const frames = {
      header,
      content,
      thumbTopBefore: thumbTop[0]!,
      thumbTopAfter: thumbTop[1]!,
      thumbMiddleBefore: thumbMiddle[0]!,
      thumbMiddleAfter: thumbMiddle[1]!,
    };
    check(
      header === 0 &&
        content > 100 &&
        thumbTop[0]! > thumbTop[1]! + 60 &&
        thumbMiddle[1]! > thumbMiddle[0]! + 60,
      `VirtualList frames: ${encoded(frames)}`,
    );

    // A restored World republishes the range from its persisted anchor.
    const bytes = await host.saveWorld(client.session);
    const loaded = await host.loadWorld(bytes, {
      symbolicId: "gui-virtual-list-restored",
    });
    worlds.push(...loaded.created.values());
    const restored = await openGui(host, loaded.root);
    sessions.push(restored);
    const rows = await entitiesByName(restored);
    await p.retarget(0, { child: loaded.root });
    const republished = await settled(
      (state) => state.view.last > 0,
      restored,
      named(rows, "gui-virtual-list").id,
    );
    check(
      republished.snapshot.target.world.id === loaded.root.id &&
        republished.view.first === middle.view.first &&
        republished.view.last === middle.view.last &&
        republished.anchorIndex === COUNT / 2 &&
        close(republished.offset, middle.offset),
      `Restored range or anchor differs: ${encoded(republished)}`,
    );
    completed = true;
    return {
      attached: [attached.view.first, attached.view.last],
      initial: [initial.view.first, initial.view.last],
      scrolled: [moved.view.first, moved.view.last],
      middle: [middle.view.first, middle.view.last],
      anchor: [anchor.index, anchor.offset],
      declarations,
      declaredAtOnce: declared.size,
      restored: [republished.view.first, republished.view.last],
      frames,
    };
  } finally {
    await presentation?.close().catch(() => {});
    await cleanup(host, sessions, worlds, completed);
  }
}
