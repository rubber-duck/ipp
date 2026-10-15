import { createElement } from "react";
import {
  checkQuietGlassIdle,
  glassRimBrightness,
  type ControlRect,
} from "../idle-oracle.js";
import { ProductionSamples } from "../optical-material/production-scene.js";
import type { Client, HostClientBase } from "@ipp/client";
import type { GuiKitContract } from "@ipp/react/gui-kit";
import { check } from "../../../harness/page/checks.js";
import type { Present } from "../../skin-lab/specimen-session.js";
import type { RgbaImage } from "../../../../tools/shared-host/images.js";
import type { GlassSkinEnvironment } from "./glass-skin.js";
import {
  openOpticalComponents,
  captureOpticalComponents,
  closeOpticalComponents,
} from "../optical-material/component-session.js";

function changed(a: RgbaImage, b: RgbaImage, rect: readonly number[]) {
  let count = 0;
  for (let y = Math.floor(rect[1]!); y < Math.ceil(rect[1]! + rect[3]!); y++)
    for (
      let x = Math.floor(rect[0]!);
      x < Math.ceil(rect[0]! + rect[2]!);
      x++
    ) {
      const at = (y * a.width + x) * 4;
      if (
        [0, 1, 2].some(
          (c) => Math.abs(a.pixels[at + c]! - b.pixels[at + c]!) > 8,
        )
      )
        count++;
    }
  return count;
}

/** The join has no separate horizontal rims, corner notches or backdrop gap. */
function checkContinuousSheet(frame: RgbaImage, seam: number, glass = true) {
  for (const x of [233, 250, 300, 500, 527]) {
    const before = ((seam - 2) * frame.width + x) * 4;
    const after = ((seam + 1) * frame.width + x) * 4;
    if (glass)
      check(
        frame.pixels[before + 1]! > 35 && frame.pixels[after + 1]! > 35,
        `Joined surface covers both sides at ${x},${seam}`,
      );
    for (const c of [0, 1, 2])
      check(
        Math.abs(frame.pixels[before + c]! - frame.pixels[after + c]!) < 12,
        `Continuous surface across ${x},${seam}, channel ${c}`,
      );
  }
}

function dialArcResponse(a: RgbaImage, b: RgbaImage, bounds: ControlRect) {
  const [left, top, width, height] = bounds;
  const cx = left + width / 2,
    cy = top + height / 2;
  // The authored 16px specimen uses a 6px tick inset, 2px ticks, 4px gap
  // and 4px value stroke. Placement follows evaluated bounds, including caption gaps.
  const radius = Math.min(width, height) / 2 - 6 - 2 - 4 - 2;
  let changed = 0,
    peak = 0;
  for (let y = Math.floor(cy - radius - 2); y <= cy + radius + 2; y++)
    for (let x = Math.floor(cx - radius - 2); x <= cx + radius + 2; x++) {
      const distance = Math.hypot(x + 0.5 - cx, y + 0.5 - cy);
      if (distance < radius - 1.5 || distance > radius + 1.5) continue;
      const at = (y * a.width + x) * 4;
      const difference = Math.max(
        ...[0, 1, 2].map((c) =>
          Math.abs(a.pixels[at + c]! - b.pixels[at + c]!),
        ),
      );
      // Dim idle reflections move across the arc without the old white rim.
      // Require a coherent response above one-level rounding noise, plus a visible glint.
      if (difference > 2) changed++;
      peak = Math.max(peak, difference);
    }
  return { changed, peak };
}

/** Production skin, actual scene camera/light, routed input and completed frames. */
export async function exerciseProductionOptics(
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
    "glass/production-optics",
    true,
  );
  const save = async (name: string, frame: RgbaImage) => {
    let data = "";
    for (let i = 0; i < frame.pixels.length; i += 0x8000)
      data += String.fromCharCode(...frame.pixels.subarray(i, i + 0x8000));
    await environment.capture({
      name,
      width: frame.width,
      height: frame.height,
      rgba: btoa(data),
    });
  };
  try {
    const front = await captureOpticalComponents(session, present);
    await save("production-glass", front);
    const green = (x: number, y: number) =>
      front.pixels[(y * front.width + x) * 4 + 1]!;
    check(
      green(700, 562) > green(880, 562) + 50,
      "Slider fill remains clearly distinct from its unfilled track",
    );
    check(
      Math.abs(green(1051, 391) - green(1048, 391)) < 3 &&
        green(1100, 391) > green(1048, 391) + 20,
      "The exact colour field has a rounded, visible backing",
    );
    const inspected = await session.canvasClient.inspect();
    const radio = inspected.entities
      .find(
        (entity) =>
          entity.metadata.symbolicId === "production-radio/clear/mark",
      )!
      .components.find(
        (component) =>
          component.component ===
          session.canvasClient.components.CanvasBounds!.id,
      )!.fields;
    const radioX = Number(radio.x) + Number(radio.width) / 2;
    const radioY = Number(radio.y) + Number(radio.height) / 2;
    const dotRegion = Math.min(Number(radio.width), Number(radio.height)) * 0.3;
    let dotCount = 0,
      dotX = 0,
      dotY = 0;
    for (let y = Math.floor(radioY - dotRegion); y < radioY + dotRegion; y++)
      for (
        let x = Math.floor(radioX - dotRegion);
        x < radioX + dotRegion;
        x++
      ) {
        const offset = (y * front.width + x) * 4;
        if (front.pixels[offset + 1]! > 170) {
          dotCount++;
          dotX += x + 0.5;
          dotY += y + 0.5;
        }
      }
    check(
      dotCount > 6 &&
        Math.abs(dotX / dotCount - radioX) < 1.5 &&
        Math.abs(dotY / dotCount - radioY) < 1.5,
      "The selected radio dot is visible and centered in its circular sheet",
    );
    const controlBounds = Object.fromEntries(
      ["button", "input", "dial", "checkbox", "switch", "number"].map(
        (name) => {
          const control = session.controls.get(name)!;
          const fields = inspected.entities
            .find((entity) => entity.id === control.target.entity)!
            .components.find(
              (component) =>
                component.component ===
                session.canvasClient.components.CanvasBounds!.id,
            )!.fields;
          return [
            name,
            [
              Number(fields.x),
              Number(fields.y),
              Number(fields.width),
              Number(fields.height),
            ] as ControlRect,
          ];
        },
      ),
    );
    const idleReflections = checkQuietGlassIdle(front, controlBounds);
    await present(session.binding, async (view) => {
      const same: Present = async (_binding, fn) => fn(view);
      const input = await host.input.open(view);
      const button = session.controls.get("button")!;
      const [x, y, width, height] = controlBounds.button!;
      const point = [(x + width / 2) / 1536, (y + height / 2) / 1024] as const;
      try {
        await input.send({ kind: "pointerMove", pointer: 41n, point });
        const hovered = await captureOpticalComponents(session, same);
        await save("production-glass-hover", hovered);
        const hoverRim = glassRimBrightness(hovered, controlBounds.button!);
        check(
          hoverRim.mean > idleReflections.button!.mean + 8,
          `Hover does not raise the quiet glass highlight: ${JSON.stringify({ idle: idleReflections.button, hover: hoverRim, bounds: controlBounds.button })}`,
        );
        await input.send({ kind: "pointerMove", pointer: 41n, point: [0, 0] });
        await button.focus();
        const focused = await captureOpticalComponents(session, same);
        await save("production-glass-focus", focused);
        check(
          changed(front, focused, controlBounds.button!) > 80,
          "Focus does not remain visible on quiet glass",
        );
        await button.blur();
      } finally {
        await input.close();
      }
    });
    const right = await captureOpticalComponents(session, present, {
      light: 3,
    });
    await save("production-glass-light-right", right);
    const regions = {
      choices: [230, 178, 1180, 132],
      values: [230, 390, 1020, 190],
      feedback: [230, 606, 1180, 56],
      surfaces: [230, 704, 1180, 168],
      rings: [230, 904, 1180, 96],
    };
    const responses = Object.fromEntries(
      Object.entries(regions).map(([name, rect]) => [
        name,
        changed(front, right, rect),
      ]),
    );
    for (const [name, count] of Object.entries(responses))
      check(count > 150, `${name} responds to scene light (${count})`);
    const dialArcLightResponse = dialArcResponse(
      front,
      right,
      controlBounds.dial!,
    );
    check(
      dialArcLightResponse.changed > 80 && dialArcLightResponse.peak > 10,
      `Quiet dial value arc itself responds to the source, independently of its housing: ${JSON.stringify(dialArcLightResponse)}`,
    );
    const angle = await captureOpticalComponents(session, present, {
      angle: 0.45,
    });
    await save("production-glass-view-angle", angle);
    check(
      changed(front, angle, [230, 178, 1180, 694]) > 15000,
      "Production glass responds to the actual camera",
    );
    const ids = Object.fromEntries(
      [...session.controls].map(([name, control]) => [
        name,
        control.target.entity,
      ]),
    );
    await present(session.binding, async (view) => {
      const same: Present = async (_binding, fn) => fn(view);
      const capture = () => captureOpticalComponents(session, same);
      await capture();
      const input = await host.input.open(view);
      const click = async (x: number, y: number) => {
        const point = [x / 1536, y / 1024] as const;
        await input.send({ kind: "pointerDown", pointer: 42n, point });
        await input.send({ kind: "pointerUp", pointer: 42n, point });
      };
      try {
        // Exercise both compact step cells through routed pointer input.
        const [nx, ny, nw, nh] = controlBounds.number!;
        await click(nx + nw - 8, ny + nh / 2);
        await capture();
        check(
          (await session.controls.get("number")!.read()).value === 25,
          "Compact numeric stepper increments through its visible step cell",
        );
        await click(nx + 8, ny + nh / 2);
        await capture();
        check(
          (await session.controls.get("number")!.read()).value === 24,
          "Compact numeric stepper decrements through its visible step cell",
        );
        await click(258, 504);
        await capture();
        check(
          (await session.controls.get("switch")!.read()).checked === false,
          "Native production switch toggles",
        );
        await click(332, 504);
        await capture();
        check(
          (await session.controls.get("checkbox")!.read()).checked === false,
          "Native production checkbox toggles",
        );
        await click(878, 562);
        await capture();
        check(
          Number((await session.controls.get("slider")!.read()).value) > 80,
          "Native production slider follows pointer",
        );
        await session.controls.get("input")!.focus();
        await capture();
        check(input.nativeText, "Production field opens native text editing");
        await input.editText(input.nativeText.fence, {
          kind: "selection",
          start: 0,
          end: 12,
        });
        await capture();
        await input.editText(input.nativeText!.fence, {
          kind: "text",
          text: "Natural glass",
        });
        await capture();
        check(
          (await session.controls.get("input")!.read()).text ===
            "Natural glass",
          "Production field retains native edits",
        );
        await click(380, 194);
        const dropdown = await capture();
        await save("production-glass-dropdown", dropdown);
        checkContinuousSheet(dropdown, 210);
        check(
          changed(front, dropdown, [230, 210, 300, 140]) > 1000,
          "Searchable dropdown renders native overlay rows",
        );
        await input.send({ kind: "key", key: "escape" });
        await capture();
        await click(860, 720);
        const tab = await capture();
        await save("production-glass-tabs", tab);
        check(
          changed(front, tab, [625, 704, 360, 168]) > 1500,
          "Production tabs change content and state tint",
        );
        await click(1142, 802);
        const dialog = await capture();
        await save("production-glass-dialog", dialog);
        check(
          changed(tab, dialog, [400, 300, 800, 400]) > 2000,
          "Production confirmation dialog uses glass surfaces",
        );
        await input.send({ kind: "key", key: "escape" });
        await capture();
        await session.canvasRoot.render(
          createElement(ProductionSamples, {
            font: session.font,
            contract: session.contract,
            controls: session.controls,
            dropdownY: 920,
          }),
        );
        await capture();
        await click(380, 936);
        const flipped = await capture();
        await save("production-glass-dropdown-flipped", flipped);
        checkContinuousSheet(flipped, 920);
        check(
          changed(front, flipped, [230, 780, 300, 140]) > 1000,
          "Dropdown opens upward when the lower edge has no room",
        );
        await input.send({ kind: "key", key: "escape" });
        await capture();
        // The same production selection control and native geometry in the peer skin.
        for (const dropdownY of [178, 920]) {
          await session.canvasRoot.render(
            createElement(ProductionSamples, {
              font: session.font,
              contract: session.contract,
              controls: session.controls,
              skin: "cybertron",
              dropdownY,
            }),
          );
          // A skin switch can acknowledge before observer-triggered appearance
          // commits. Require the native perimeter and label, not a quiet empty frame.
          const ready = async () => {
            const deadline = Date.now() + 30_000;
            for (;;) {
              const frame = await capture();
              const edge = ((dropdownY + 12) * frame.width + 230) * 4;
              let textPixels = 0;
              for (let y = dropdownY + 8; y < dropdownY + 30; y++)
                for (let x = 242; x < 400; x++) {
                  const at = (y * frame.width + x) * 4;
                  if (frame.pixels[at]! > 140 && frame.pixels[at + 1]! > 160)
                    textPixels++;
                }
              if (
                frame.pixels[edge + 2]! > 75 &&
                frame.pixels[edge + 1]! > 75 &&
                textPixels > 40
              )
                return frame;
              check(
                Date.now() < deadline,
                `Cybertron dropdown appearance ready at ${dropdownY}: edge=${frame.pixels[edge]}, text=${textPixels}`,
              );
            }
          };
          await ready();
          await click(380, dropdownY + 16);
          const cybertron = await ready();
          await save(`production-cybertron-dropdown-${dropdownY}`, cybertron);
          // Cybertron's control height is 40; the upward popup joins at the top.
          checkContinuousSheet(cybertron, dropdownY === 178 ? 218 : 920, false);
          await input.send({ kind: "key", key: "escape" });
          await capture();
        }
        for (const [name, id] of Object.entries(ids))
          check(
            session.controls.get(name)!.target.entity === id,
            `Presentation preserved ${name} identity`,
          );
      } finally {
        await input.close();
      }
    });
    await environment.record({
      productionOptics: true,
      idleReflections,
      dialArcLightResponse,
      lightResponses: responses,
      nativeInteraction: true,
      preservedIdentities: Object.keys(ids).length,
    });
  } finally {
    await closeOpticalComponents(session);
  }
}
