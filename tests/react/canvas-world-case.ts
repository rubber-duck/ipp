import * as React from "react";
import {
  canvasOutput,
  type CanvasStateRecord,
  type Client,
  type HostClientBase,
  type WorldReference,
} from "@ipp/client";
import {
  AttachedWorld,
  CanvasWorld,
  Entity,
  Surface,
  createRoot,
  type AttachedWorldHandle,
  type CanvasWorldHandle,
} from "@ipp/react";
import { findEntity } from "./fixture-helpers.js";
import {
  CANVAS,
  REACT_ROOT,
  selectSystems,
} from "../integration/system-selections.js";

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

const encoded = (value: unknown) =>
  JSON.stringify(value, (_key, field) =>
    typeof field === "bigint" ? String(field) : field,
  );

/** An error with the causes an AggregateError carries. */
function describe(error: unknown): string {
  if (error instanceof AggregateError)
    return `${error.message} (${error.errors.map(describe).join("; ")})`;
  return error instanceof Error ? error.message : String(error);
}

async function canvasState(client: Client): Promise<CanvasStateRecord> {
  const record = (await client.inspectPage({ collection: "canvas" })).canvas;
  check(record, "The canvas World reported no Canvas state");
  return record;
}

/** Wait until the World's Canvas state is `expected`; updates carry no reply. */
async function awaitCanvasState(
  client: Client,
  expected: CanvasStateRecord["state"],
): Promise<void> {
  const deadline = performance.now() + 5000;
  let state = (await canvasState(client)).state;
  while (encoded(state) !== encoded(expected)) {
    if (performance.now() > deadline)
      throw new Error(
        `Canvas state ${encoded(state)} never became ${encoded(expected)}`,
      );
    await new Promise<void>((resolve) => setTimeout(resolve, 16));
    state = (await canvasState(client)).state;
  }
}

function attachmentOf(
  client: Client,
  parent: Awaited<ReturnType<Client["inspect"]>>,
  anchor: string,
) {
  const component = client.components.WorldAttachment;
  check(component, "The parent World does not select WorldAttachment");
  return findEntity(parent, anchor)?.components.find(
    (candidate) => candidate.component === component.id,
  )?.fields;
}

/**
 * `<CanvasWorld>` and SurfaceCanvas attachments against a real runtime: a
 * CanvasWorld creates its World with its canvas state, presents that World's
 * canvas on a parent anchor without naming an output, sends Canvas state
 * updates for changed props without recreating the World and destroys it on
 * removal; a borrowed canvas World stays when its attachment goes.
 */
export async function exerciseCanvasWorlds(
  host: HostClientBase<Client>,
): Promise<string[]> {
  const report: string[] = [];
  const worlds: WorldReference[] = [];
  const sessions: Client[] = [];
  const failures: Error[] = [];
  const exists = async (world: WorldReference) =>
    (await host.listWorlds()).some((item) => item.id === world.id);
  const parentWorld = (
    await host.createWorld({ selectedSystems: selectSystems(REACT_ROOT) })
  ).reference;
  worlds.push(parentWorld);
  const parent = await host.openWorld(parentWorld);
  sessions.push(parent);
  const root = createRoot(parent, {
    host,
    onError: (error) => {
      failures.push(error);
    },
  });
  try {
    const anchor = (name: string) =>
      React.createElement(
        Entity,
        { key: name, id: name },
        React.createElement(Surface, { width: 2, height: 1 }),
      );
    const handle = React.createRef<CanvasWorldHandle>();
    const panel = (extent: readonly [number, number], density?: number) =>
      React.createElement(
        React.Fragment,
        null,
        anchor("panel-anchor"),
        React.createElement(
          CanvasWorld,
          {
            create: { selectedSystems: selectSystems(CANVAS) },
            extent,
            ...(density !== undefined ? { unitsPerMetre: density } : {}),
            presentation: { anchor: "panel-anchor" },
            ref: handle,
          },
          React.createElement(Entity, { id: "panel-content" }),
        ),
      );
    await root.render(panel([320, 200], 2));
    const owned = handle.current;
    check(
      owned,
      `CanvasWorld did not become ready: ${failures.map(describe).join("; ")}`,
    );
    const world = owned.world;
    check(
      encoded(owned.output) === encoded(canvasOutput(world)),
      "CanvasWorld did not report its World's canvas output",
    );
    const attachment = attachmentOf(
      parent,
      await parent.inspect(),
      "panel-anchor",
    );
    check(
      attachment?.mode === 1 &&
        attachment.output === null &&
        encoded(attachment.child) === encoded(world),
      `SurfaceCanvas attachment named an output: ${encoded(attachment)}`,
    );
    const child = await host.openWorld(world);
    sessions.push(child);
    check(
      findEntity(await child.inspect(), "panel-content"),
      "CanvasWorld children were not declared in its World",
    );
    await awaitCanvasState(child, { extent: [320, 200], unitsPerMetre: 2 });
    report.push(
      "CanvasWorld creates its World with its canvas state and presents it on a SurfaceCanvas anchor without an output",
    );

    await root.render(panel([160, 100]));
    await awaitCanvasState(child, { extent: [160, 100], unitsPerMetre: 2 });
    check(
      handle.current?.world.id === world.id &&
        handle.current.world.incarnation === world.incarnation,
      "A changed extent recreated the CanvasWorld's World",
    );
    report.push(
      "changed CanvasWorld props update the Canvas state in place and omitted density keeps its value",
    );

    const closed = handle.current.closed;
    await root.render(null);
    await closed;
    check(!(await exists(world)), "Removing CanvasWorld left its World");
    report.push("removing CanvasWorld destroys the World it created");

    const borrowed = (
      await host.createWorld({
        selectedSystems: selectSystems(CANVAS),
        canvas: { extent: [64, 32], unitsPerMetre: 1 },
      })
    ).reference;
    worlds.push(borrowed);
    const attached = React.createRef<AttachedWorldHandle>();
    await root.render(
      React.createElement(
        React.Fragment,
        null,
        anchor("borrowed-anchor"),
        React.createElement(AttachedWorld, {
          anchor: "borrowed-anchor",
          child: { borrow: borrowed },
          attachment: { mode: "surface-canvas" },
          ref: attached,
        }),
      ),
    );
    const attachedHandle = attached.current;
    check(attachedHandle, "Borrowed SurfaceCanvas attachment did not attach");
    check(
      encoded(attachedHandle.output) === encoded(canvasOutput(borrowed)),
      "SurfaceCanvas handle did not report the child World's canvas",
    );
    await root.render(anchor("borrowed-anchor"));
    await attachedHandle.closed;
    check(
      attachmentOf(parent, await parent.inspect(), "borrowed-anchor") ===
        undefined,
      "Removing the SurfaceCanvas attachment left it on its anchor",
    );
    check(await exists(borrowed), "A borrowed canvas World was destroyed");
    report.push(
      "a borrowed canvas World presented through SurfaceCanvas stays when its attachment is removed",
    );
    check(!failures.length, `React reported: ${failures.join("; ")}`);
    return report;
  } finally {
    await root.unmount().catch(() => {});
    for (const session of sessions.reverse())
      if (!session.closure) await session.close().catch(() => {});
    for (const world of worlds.reverse())
      if (await exists(world)) await host.destroyWorld(world).catch(() => {});
  }
}
