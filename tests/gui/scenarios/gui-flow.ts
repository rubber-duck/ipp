/** Native flow sizing and retained frames through an ordinary generated client.
 * The driver supplies the process arrangement and artifact storage. */
import type { Command } from "@ipp/client";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../../fixtures/commands.js";
import {
  GUI,
  LIFECYCLE,
  selectSystems,
} from "../../fixtures/system-selections.js";
import { check } from "../../harness/page/checks.js";
import {
  alias,
  cleanup,
  control,
  encoded,
  openGui,
  PANEL_CANVAS,
  pixel,
  place,
  presentGui,
  setFields,
  type GuiContract,
  type GuiFrame,
  type GuiHost,
  type PresentedGui,
} from "./gui-lifecycle.js";

export async function exerciseGuiFlow(
  host: GuiHost,
  contract: GuiContract,
  capture: (name: string, frame: GuiFrame) => Promise<void>,
) {
  const world = await host.createWorld({
    symbolicId: "gui-flow",
    selectedSystems: selectSystems(GUI, LIFECYCLE),
    canvas: PANEL_CANVAS,
  });
  const client = await openGui(host, world.reference);
  let presentation: PresentedGui | undefined;
  let completed = false;
  try {
    const commands: Command[] = [
      createEntity(1, "flow-row"),
      insertComponent(client, "GuiLayout", alias(1), {
        kind: 1,
        width: 4,
        height: 1,
        gap: 0.25,
        align_items: 0,
        padding_left: 0.25,
        padding_right: 0.25,
        padding_top: 0.25,
        padding_bottom: 0.25,
      }),
      createEntity(9, "flow-paint"),
      insertComponent(client, "GuiTheme", alias(9), {
        parts: contract.GuiTheme.encodeParts({
          nextSlot: 1,
          rows: new Map([
            [
              0,
              {
                part: contract.guiPaintPartIndex({ part: "background" }),
                color: [1, 0, 0, 1],
                corner_radius: [0, 0],
                border_width: 0,
              },
            ],
          ]),
        }),
      }),
    ];
    const button = (id: number, layout: Record<string, number>) => {
      commands.push(
        createEntity(id, `flow-button-${id}`),
        insertComponent(client, "GuiButton", alias(id)),
        insertComponent(client, "GuiLayout", alias(id), layout),
        {
          kind: "insertComponent",
          entity: alias(id),
          component: client.components.GuiSkin!.id,
          fields: [
            {
              offset: client.components.GuiSkin!.fields.theme!.offset,
              value: { kind: "entity", value: alias(9) },
            },
          ],
        },
        place(alias(id), alias(1)),
      );
    };
    button(2, { width: 0.5, height: 0.25 });
    // Closed presentation gates and overlay entries sit between ordinary children.
    for (const id of [5, 6, 7]) {
      commands.push(
        createEntity(id, `flow-excluded-${id}`),
        insertComponent(client, "GuiLayout", alias(id), {
          width: 2,
          height: 2,
        }),
        place(alias(id), alias(1)),
      );
      if (id !== 7)
        commands.push(
          insertComponent(client, "PresentationGate", alias(id), {
            open: false,
          }),
        );
      else
        commands.push(
          insertComponent(client, "GuiBehavior", alias(id), { visible: false }),
        );
      if (id === 7)
        commands.push(insertComponent(client, "GuiOverlay", alias(id)));
    }
    button(3, { flex: 1, height: 0.5 });
    button(4, { flex: 1, height: 0.5, align_y: -1 });
    const outcome = successfulBatch(await client.batch(commands));
    const root = aliasId(outcome, 1);
    const ids = [2, 3, 4].map((id) => aliasId(outcome, id));
    presentation = await presentGui(host, [{ child: world.reference }]);
    const rectangles = async (expected: number[][]) => {
      const deadline = Date.now() + 10_000;
      for (;;) {
        await presentation!.frame();
        const actual = await Promise.all(
          ids.map(async (id) => (await control(client, id)).bounds),
        );
        if (
          actual.every((rect, index) =>
            rect.every(
              (value, axis) =>
                Math.abs(value - expected[index]![axis]!) < 0.001,
            ),
          )
        )
          return actual;
        check(
          Date.now() < deadline,
          `Flow rectangles differ: ${encoded({ actual, expected })}`,
        );
      }
    };
    const initial = await rectangles([
      [0.25, 0.375, 0.5, 0.25],
      [1, 0.25, 1.25, 0.5],
      [2.5, 0.25, 1.25, 0.5],
    ]);
    const before = await presentation.settled();
    await capture("flow-centered", before);
    const red = (frame: GuiFrame, point: [number, number]) => {
      const [r, g, b] = pixel(frame, presentation!.point(0, point));
      return r > 240 && g < 10 && b < 10;
    };
    check(
      red(before, [0.5, 0.5]) && red(before, [1.1, 0.3]),
      "Completed flow frame missed painted children",
    );
    check(
      !red(before, [0.875, 0.5]) && !red(before, [0.5, 0.3]),
      "Gap or alignment painted outside its box",
    );
    successfulBatch(
      await client.batch(
        setFields(client, root, "GuiLayout", { gap: 0.5, align_items: 1 }),
      ),
    );
    const changed = await rectangles([
      [0.25, 0.5, 0.5, 0.25],
      [1.25, 0.25, 1, 0.5],
      [2.75, 0.25, 1, 0.5],
    ]);
    const after = await presentation.settled();
    await capture("flow-ended", after);
    check(
      !red(after, [1.1, 0.3]) && red(after, [0.5, 0.65]),
      "Gap/alignment writes did not change completed paint",
    );
    check(!red(after, [0.5, 0.4]), "Old alignment remained painted");
    // Open a gate: the ordinary child's width and one additional gap enter flow.
    successfulBatch(
      await client.batch([
        ...setFields(client, aliasId(outcome, 5), "GuiLayout", {
          width: 0.25,
          height: 0.25,
        }),
        ...setFields(client, aliasId(outcome, 5), "PresentationGate", {
          open: true,
        }),
      ]),
    );
    const revealed = await rectangles([
      [0.25, 0.5, 0.5, 0.25],
      [2, 0.25, 0.625, 0.5],
      [3.125, 0.25, 0.625, 0.5],
    ]);
    const visible = await presentation.settled();
    await capture("flow-revealed", visible);
    check(
      !red(visible, [1.5, 0.3]) && red(visible, [2.2, 0.3]),
      "Revealed child did not reflow retained paint",
    );
    completed = true;
    return { initial, changed, revealed, frames: 3 };
  } finally {
    await presentation?.close();
    await cleanup(host, [client], [world.reference], completed);
  }
}
