import type { Client, HostClientBase } from "@ipp/client";
import { createElement } from "react";
import type { GuiKitContract } from "@ipp/react/gui-kit";
import type { RgbaImage } from "../../../../tools/shared-host/images.js";
import { check } from "../../../harness/page/checks.js";
import type { Present } from "../../skin-lab/specimen-session.js";
import {
  openOpticalComponents,
  captureOpticalComponents,
  closeOpticalComponents,
  readyOpticalComponents,
} from "../optical-material/component-session.js";
import { ProductionSamples } from "../optical-material/production-scene.js";
import type { GlassSkinEnvironment } from "./glass-skin.js";

function changes(
  a: RgbaImage,
  b: RgbaImage,
  rect: readonly [number, number, number, number],
) {
  let count = 0;
  for (let y = rect[1]; y < rect[1] + rect[3]; y++)
    for (let x = rect[0]; x < rect[0] + rect[2]; x++) {
      const at = (y * a.width + x) * 4;
      if (
        [0, 1, 2].some(
          (c) => Math.abs(a.pixels[at + c]! - b.pixels[at + c]!) > 3,
        )
      )
        count++;
    }
  return count;
}

/** Actual native state and material on GLES/WebGL, independent of process arrangement. */
export async function exerciseLiquidOptics(
  host: HostClientBase<Client>,
  contract: GuiKitContract,
  font: Uint8Array<ArrayBuffer>,
  present: Present,
  environment: GlassSkinEnvironment,
) {
  const session = await openOpticalComponents(
    host,
    font,
    contract,
    "glass/liquid",
    "liquid",
  );
  const save = async (name: string, frame: RgbaImage) => {
    let binary = "";
    for (let i = 0; i < frame.pixels.length; i += 0x8000)
      binary += String.fromCharCode(...frame.pixels.subarray(i, i + 0x8000));
    await environment.capture({
      name,
      width: frame.width,
      height: frame.height,
      rgba: btoa(binary),
    });
  };
  try {
    const idle = await captureOpticalComponents(session, present);
    await save("liquid-idle", idle);
    check(
      idle.width === 1536 && idle.height === 1024,
      "Liquid sheet frame dimensions",
    );
    // Glyph-free face samples must show a dark visible body, not fallback white,
    // an empty framebuffer, or clipped radiance masquerading as glass.
    const body = [200, 280, 360].map(
      (x) => idle.pixels[(275 * idle.width + x) * 4 + 1]!,
    );
    check(
      body.every((v) => v > 25 && v < 95),
      `Smoky idle body (${body})`,
    );
    const right = await captureOpticalComponents(session, present, {
      light: 3,
    });
    await save("liquid-light-right", right);
    const rimChanges = changes(idle, right, [140, 190, 290, 9]);
    check(rimChanges > 50, `Light moves the polished edge (${rimChanges})`);
    check(
      changes(idle, right, [140, 180, 290, 7]) === 0,
      "No changing glow outside the glass",
    );
    const angle = await captureOpticalComponents(session, present, {
      angle: 0.45,
    });
    await save("liquid-view-angle", angle);
    check(
      changes(idle, angle, [80, 190, 414, 110]) > 4000,
      "Oblique Surface is rendered",
    );
    await present(session.binding, async (view) => {
      const sameView: Present = async (_binding, action) => action(view);
      const capture = () => captureOpticalComponents(session, sameView);
      await capture();
      const input = await host.input.open(view);
      try {
        await input.send({
          kind: "pointerMove",
          pointer: 53n,
          point: [280 / 1536, 245 / 1024],
        });
        const hovered = await capture();
        await save("liquid-hover", hovered);
        const hoverChanges = changes(idle, hovered, [150, 260, 270, 20]);
        check(
          hoverChanges > 1000,
          `Native hover brightens the face (${hoverChanges})`,
        );
        const toggle = session.controls.get("switch")!;
        check((await toggle.read()).checked === true, "Switch starts checked");
        const point = [1364 / 1536, 450 / 1024] as const;
        await input.send({ kind: "pointerDown", pointer: 53n, point });
        await input.send({ kind: "pointerUp", pointer: 53n, point });
        await capture();
        check(
          (await toggle.read()).checked === false,
          "Liquid switch keeps native interaction",
        );
        await environment.record({
          region: "liquid-optics",
          body,
          rimChanges,
          hoverChanges,
        });
      } finally {
        await input.send({ kind: "pointerCancel", pointer: 53n });
        await input.close();
      }
    });
    // Exercise the public six-vector skin adapter as well as the isolated
    // optical model, including resource replacement with retained controls.
    const production = async (finish: "optical" | "liquid") => {
      await session.canvasRoot.render(
        createElement(ProductionSamples, {
          font,
          contract,
          controls: session.controls,
          finish,
        }),
      );
      if (finish === "optical")
        await readyOpticalComponents({
          ...session,
          production: true,
          liquid: false,
        });
      return captureOpticalComponents(session, present);
    };
    const original = await production("optical");
    const identity = session.controls.get("button")!.target.entity;
    let liquid = await production("liquid");
    // Replacement skins own a fresh asset scope. Observe the completed visual
    // switch rather than waiting on the retired bundle's old shader ID.
    const deadline = Date.now() + 15000;
    while (changes(original, liquid, [230, 178, 710, 520]) <= 1000) {
      check(Date.now() < deadline, "Liquid appearance becomes ready");
      liquid = await captureOpticalComponents(session, present);
    }
    await save("liquid-production-components", liquid);
    check(
      session.controls.get("button")!.target.entity === identity,
      "Appearance switch retains native control identity",
    );
    check(
      changes(original, liquid, [230, 178, 710, 520]) > 1000,
      "Public skin adapter changes native control material",
    );
    const pixel = (frame: RgbaImage, x: number, y: number) =>
      frame.pixels.subarray(
        (y * frame.width + x) * 4,
        (y * frame.width + x) * 4 + 3,
      );
    const roundedCorners = (frame: RgbaImage) => {
      const backdrop = pixel(frame, 625, 248);
      for (const [x, y] of [
        [628, 253],
        [941, 253],
        [233, 181],
      ])
        check(
          pixel(frame, x!, y!).every(
            (value, axis) => Math.abs(value - backdrop[axis]!) < 5,
          ),
          "Capsule corners expose the backdrop rather than a square fill",
        );
      // Probe the face just inside the rounded end, beyond the thin polished
      // rim and dark shoulder: coverage must not depend on highlight width.
      check(
        pixel(frame, 628, 266)[1]! > backdrop[1]! + 20,
        "The capsule still covers the midpoint of its rounded end",
      );
    };
    roundedCorners(liquid);
    await present(session.binding, async (view) => {
      const same: Present = async (_binding, fn) => fn(view);
      const input = await host.input.open(view);
      const click = async (x: number, y: number) => {
        const point = [x / liquid.width, y / liquid.height] as const;
        await input.send({ kind: "pointerMove", pointer: 54n, point });
        await input.send({
          kind: "pointerDown",
          pointer: 54n,
          point,
          button: "primary",
        });
        await input.send({
          kind: "pointerUp",
          pointer: 54n,
          point,
          button: "primary",
        });
      };
      try {
        await click(890, 266);
        const selected = await captureOpticalComponents(session, same);
        roundedCorners(selected);
        check(
          changes(liquid, selected, [625, 250, 320, 32]) > 200,
          "Selected outer segment follows the same capsule",
        );
        await click(380, 194);
        const open = await captureOpticalComponents(session, same);
        await save("liquid-production-dropdown", open);
        for (const x of [233, 300, 527]) {
          const before = pixel(open, x, 208),
            after = pixel(open, x, 211);
          check(
            before[1]! > 35 &&
              after[1]! > 35 &&
              before.every(
                (value, axis) => Math.abs(value - after[axis]!) < 12,
              ),
            "Expanded liquid dropdown remains one continuous sheet",
          );
        }
      } finally {
        await input.close();
      }
    });
  } finally {
    await closeOpticalComponents(session);
  }
}
