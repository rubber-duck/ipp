/** Dense stroke paint keeps one alpha union and exact row hits through scaling. */
import { exerciseStrokeSurfaces } from "./stroke-surfaces.js";
import type {
  Client,
  HostClientBase,
  PickingWorldClient,
  RootBinding,
} from "@ipp/client";
import {
  componentFields,
  successfulBatch,
} from "../../../examples/demos/scenes/charts/shared/commands.js";
import {
  openWorkload,
  sample,
  type ChartContract,
} from "./chart-data-workload.js";
import { check as checkValue } from "../../harness/page/checks.js";

function check(
  value: unknown,
  message = "Dense line invariant",
): asserts value {
  checkValue(value, message);
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
  let rootCleared = false;
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
    // Strong minification must combine the covered pixel intervals before
    // applying alpha; nearest-segment distance alone loses this dense union.
    for (const [scale_x, scale_y] of [
      [1, 0.05],
      [0.05, 1],
    ] as const) {
      await style({ alpha: 1, scale_x, scale_y });
      const thin = await capture(
        `dense-line-thin-${scale_x}-${scale_y}`,
        fixture.binding,
      );
      await style({ alpha: 0.35 });
      const alpha = await capture(
        `dense-line-thin-alpha-${scale_x}-${scale_y}`,
        fixture.binding,
      );
      let ink = 0;
      for (let i = 0; i < thin.pixels.length; i += 4) {
        const x = (i / 4) % thin.width;
        const y = Math.floor(i / 4 / thin.width);
        // Exclude the axes and labels underneath the trace: their nonblack
        // backing deliberately changes the composited translucent colour.
        if (
          x <= 8 + 56 * scale_x + 1 ||
          x >= 8 + 600 * scale_x - 1 ||
          y <= 8 + 20 * scale_y + 1 ||
          y >= 8 + 336 * scale_y - 1
        )
          continue;
        if (
          thin.pixels[i]! < 40 &&
          thin.pixels[i + 1]! > 220 &&
          thin.pixels[i + 2]! > 245
        ) {
          check(
            alpha.pixels[i + 2]! > 145 && alpha.pixels[i + 2]! < 180,
            "thin contour overlap changed union alpha",
          );
          ink++;
        }
      }
      check(
        ink > 30,
        `thin analytic trace missing: ${scale_x}/${scale_y}: ${ink}`,
      );
    }
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
    await host.clearRootOutput(fixture.binding);
    rootCleared = true;
    await exerciseStrokeSurfaces(host, fixture.client, capture, record);
  } finally {
    if (!rootCleared) await host.clearRootOutput(fixture.binding);
    await fixture.close();
  }
}
