/** Dense analytic paint keeps one alpha union and exact row hits through scaling. */
import type {
  Client,
  HostClientBase,
  PickingWorldClient,
  RootBinding,
} from "@ipp/client";
import {
  componentFields,
  successfulBatch,
} from "../../../examples/world-gallery/worlds/charts/shared/commands.js";
import {
  openWorkload,
  sample,
  type ChartContract,
} from "../../performance/chart-data-scenario.js";

function check(
  value: unknown,
  message = "Dense line invariant",
): asserts value {
  if (!value) throw new Error(message);
}

function equal(actual: unknown, expected: unknown) {
  check(
    actual === expected,
    `Dense line: ${String(actual)} != ${String(expected)}`,
  );
}

interface Frame {
  width: number;
  height: number;
  pixels: Uint8Array;
}

export async function exerciseDenseLines(
  host: HostClientBase<Client>,
  contract: ChartContract,
  font: Uint8Array<ArrayBuffer>,
  capture: (label: string, binding: RootBinding) => Promise<Frame>,
  record: (label: string, value: unknown) => Promise<void>,
) {
  const rows = 1000;
  const fixture = await openWorkload(
    host,
    contract,
    font,
    {
      name: "dense-line-integration",
      component: "PlotLine2d",
      rows,
      bindings: 1,
      labels: 0,
    },
    "integration",
  );
  const entity = fixture.entities[0]!;
  const style = async (values: Parameters<typeof componentFields>[2]) =>
    successfulBatch(
      await fixture.client.batch(
        componentFields(fixture.client, "CanvasStyle", values).map((field) => ({
          kind: "setField",
          entity: { kind: "handle", id: entity },
          component: fixture.client.components.CanvasStyle!.id,
          field,
        })),
      ),
    );
  const pick = async (scale = 1) => {
    const values = sample(rows - 1, rows);
    const y = (values[1] as { value: number }).value;
    // Independently project the final source row through the authored fixed
    // frame, its 56/24/20/48 padding and Canvas translation (8,8).
    const result = await (
      fixture.client as unknown as PickingWorldClient
    ).query({
      type: "GeometryPickQuery",
      view: { kind: "bound", binding: fixture.binding },
      x: (8 + 604 * scale) / 640,
      y: (8 + (336 - y * 156) * scale) / 400,
      includeViewPlane: false,
    });
    check(result.ok && result.hit);
    equal(result.hit.entity, entity);
    equal(result.hit.component, fixture.client.components.PlotLine2d!.id);
    check(
      result.hit.row?.series === 0 && result.hit.row.rowId === BigInt(rows),
      "dense final source row identity",
    );
    return result;
  };
  try {
    await fixture.verify();
    // A ready binding may still await font/chart preparation; two identical
    // empty frames are not paint readiness. Observe the ordinary PlotPage.
    const deadline = performance.now() + 30_000;
    for (;;) {
      const page = await host.datasets.bindingView(
        fixture.client.session,
        entity,
      );
      if (page.availability.reason === "Ready" && !page.dirty) break;
      check(
        performance.now() < deadline,
        `dense plot preparation timed out: ${page.availability.reason}, dirty=${page.dirty}`,
      );
      await fixture.client.waitForFrame();
    }
    const opaque = await capture("dense-line-opaque", fixture.binding);
    await record("dense-line-pick", await pick());
    await style({ alpha: 0.35 });
    const translucent = await capture("dense-line-alpha", fixture.binding);
    let covered = 0;
    // Dense crossings remain one analytic paint. Repeated alpha-blended
    // segment draws would approach opaque cyan instead of this bounded blue.
    for (let y = 120; y < 330; y++) {
      for (let x = 70; x < 595; x++) {
        const i = (y * opaque.width + x) * 4;
        if (
          opaque.pixels[i]! < 40 &&
          opaque.pixels[i + 1]! > 220 &&
          opaque.pixels[i + 2]! > 245
        ) {
          check(
            translucent.pixels[i + 2]! > 145 &&
              translucent.pixels[i + 2]! < 180,
            "dense contour overlap changed union alpha",
          );
          covered++;
        }
      }
    }
    check(covered > 500, "dense line paint absent");
    await style({ scale_x: 0.01, scale_y: 0.01 });
    await capture("dense-line-minified", fixture.binding);
    await record("dense-line-minified-pick", await pick(0.01));
    await style({ alpha: 1, scale_x: 1, scale_y: 1 });
    const restored = await capture("dense-line-restored", fixture.binding);
    check(
      restored.pixels.length === opaque.pixels.length &&
        restored.pixels.every((value, i) => value === opaque.pixels[i]),
      "restoring placement changed retained analytic paint",
    );
    await fixture.action("edit", 0);
    const edited = await capture("dense-line-edited", fixture.binding);
    await fixture.verify();
    check(
      edited.pixels.some((value, i) => value !== restored.pixels[i]),
      "dense row edit did not change paint",
    );
    await record("dense-line-edit-pick", await pick());
  } finally {
    await host.clearRootOutput(fixture.binding);
    await fixture.close();
  }
}
