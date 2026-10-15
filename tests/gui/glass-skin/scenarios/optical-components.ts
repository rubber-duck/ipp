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
  for (let y = rect[1]!; y < rect[1]! + rect[3]!; y++)
    for (let x = rect[0]!; x < rect[0]! + rect[2]!; x++) {
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

// State tint may change the face, but must not add a bright ridge above it.
// Subtract the idle sheet and its face-tint change so genuine parent reflections
// remain allowed. This glyph-free band excludes the outside rim and dividers.
function assertSharedTabEdge(frame: RgbaImage, idle: RgbaImage, left: number) {
  let ridges = 0;
  for (let x = left; x < left + 100; x++) {
    const face = (790 * frame.width + x) * 4 + 1;
    const faceTint = frame.pixels[face]! - idle.pixels[face]!;
    for (let y = 777; y < 785; y++) {
      const at = (y * frame.width + x) * 4 + 1;
      if (frame.pixels[at]! - idle.pixels[at]! - faceTint > 12) ridges++;
    }
  }
  check(
    ridges <= 10,
    `Shared tab surface has no inset optical rim (${ridges})`,
  );
}

/** Real transport, shader assets, native controls and completed frames on both devices. */
export async function exerciseOpticalComponents(
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
    "glass/component-study",
  );
  const save = async (name: string, frame: RgbaImage) => {
    let text = "";
    for (let i = 0; i < frame.pixels.length; i += 0x8000)
      text += String.fromCharCode(...frame.pixels.subarray(i, i + 0x8000));
    await environment.capture({
      name,
      width: frame.width,
      height: frame.height,
      rgba: btoa(text),
    });
  };
  try {
    const front = await captureOpticalComponents(session, present);
    await save("optical-components", front);
    check(
      front.width === 1536 && front.height === 1024,
      "Component sheet dimensions",
    );
    const right = await captureOpticalComponents(session, present, {
      light: 3,
    });
    await save("optical-components-light-right", right);
    const regions = {
      buttons: [230, 187, 988, 76],
      choices: [230, 359, 1245, 61],
      inputs: [230, 502, 1245, 65],
      values: [230, 660, 1245, 50],
      surfaces: [230, 775, 1245, 172],
    };
    const lightChanges = Object.fromEntries(
      Object.entries(regions).map(([name, rect]) => [
        name,
        changed(front, right, rect),
      ]),
    );
    for (const [name, count] of Object.entries(lightChanges))
      check(
        count > 100,
        `${name}: light must change material pixels (${count})`,
      );
    const turned = await captureOpticalComponents(session, present, {
      angle: 0.45,
    });
    await save("optical-components-view-angle", turned);
    check(
      changed(front, turned, [230, 187, 1245, 760]) > 10000,
      "View changes the presented sheet",
    );
    await present(session.binding, async (view) => {
      const sameView: Present = async (_binding, action) => action(view);
      const capture = () => captureOpticalComponents(session, sameView);
      await capture();
      const input = await host.input.open(view);
      const click = async (x: number, y: number) => {
        const point = [x / 1536, y / 1024] as const;
        await input.send({ kind: "pointerDown", pointer: 31n, point });
        await input.send({ kind: "pointerUp", pointer: 31n, point });
      };
      try {
        check(
          (await session.controls.get("switch")!.read()).checked === false,
          "Initial switch off",
        );
        await click(280, 388);
        const toggled = await capture();
        check(
          (await session.controls.get("switch")!.read()).checked === true,
          "Native switch toggles",
        );
        check(
          changed(front, toggled, [230, 365, 108, 46]) > 600,
          "Switch thumb and track repaint",
        );
        await input.send({
          kind: "pointerMove",
          pointer: 31n,
          point: [358 / 1536, 680 / 1024],
        });
        const hovered = await capture();
        check(
          changed(toggled, hovered, [344, 666, 30, 30]) > 10,
          "Native thumb hover changes its optical response",
        );
        check(
          Number((await session.controls.get("slider")!.read()).value) === 58,
          "Hover preserves slider value",
        );
        await click(420, 680);
        await capture();
        const value = Number(
          (await session.controls.get("slider")!.read()).value,
        );
        check(
          value > 80 && value < 95,
          `Native slider value follows pointer (${value})`,
        );
        await click(790, 535);
        await capture();
        check(input.nativeText, "Native field editing is active");
        await input.editText(input.nativeText.fence, {
          kind: "selection",
          start: 0,
          end: 12,
        });
        await capture();
        await input.editText(input.nativeText!.fence, {
          kind: "text",
          text: "Glass study",
        });
        await capture();
        await click(700, 740);
        await capture();
        check(
          (await session.controls.get("input")!.read()).text === "Glass study",
          "Native text edit commits",
        );
        await click(950, 805);
        await session.canvasRoot.flush();
        const tabs = await capture();
        await save("optical-components-connected-activity", tabs);
        check(
          changed(front, tabs, [665, 775, 550, 171]) > 2000,
          "Tab selection and content change together",
        );
        check(
          (await session.controls.get("tabs-1")!.read()).selected === true,
          "Activity tab is selected",
        );
        assertSharedTabEdge(tabs, front, 900);
        await input.send({
          kind: "pointerMove",
          pointer: 31n,
          point: [1130 / 1536, 805 / 1024],
        });
        const hoverTab = await capture();
        await save("optical-components-connected-hover", hoverTab);
        assertSharedTabEdge(hoverTab, front, 1080);
        check(
          changed(tabs, hoverTab, [1050, 785, 140, 40]) > 500,
          "Unselected tab hover tints its region",
        );
        check(
          (await session.controls.get("tabs-1")!.read()).selected === true &&
            (await session.controls.get("tabs-2")!.read()).selected === false,
          "Region hover preserves selection",
        );
        await click(1130, 805);
        const lastTab = await capture();
        await save("optical-components-connected-selected", lastTab);
        assertSharedTabEdge(lastTab, front, 1080);
        check(
          (await session.controls.get("tabs-2")!.read()).selected === true,
          "Rightmost region selects without adding an inner rim",
        );
        // Focusing an already pointer-focused target is idempotent. Blur first
        // to request a visible programmatic focus transition through the API.
        await session.controls.get("tabs-2")!.blur();
        await session.controls.get("tabs-2")!.focus();
        const focusedTab = await capture();
        await save("optical-components-connected-focus", focusedTab);
        assertSharedTabEdge(focusedTab, front, 1080);
        check(
          changed(lastTab, focusedTab, [1050, 825, 140, 6]) > 50,
          "Region focus draws an inset underline",
        );
        await click(1150, 389);
        const segmented = await capture();
        check(
          (await session.controls.get("segment-0")!.read()).selected === true &&
            (await session.controls.get("segment-1")!.read()).selected ===
              false,
          "Segmented regions retain native exclusive selection",
        );
        check(
          changed(front, segmented, [1103, 359, 372, 61]) > 1000,
          "Segment selection changes the shared surface tint",
        );
        await click(1200, 535);
        await session.canvasRoot.flush();
        const dropdown = await capture();
        check(
          changed(tabs, dropdown, [1084, 350, 391, 380]) > 1000,
          "Dropdown paints its choices",
        );
        await save("optical-components-interaction", dropdown);
        await click(1200, 641);
        await session.canvasRoot.flush();
        await capture();
        check(
          (await session.controls.get("dropdown")!.read()).label ===
            "Frosted glass",
          "The separate second dropdown row selects its value",
        );
        await environment.record({
          componentLightChanges: lightChanges,
          nativeSwitch: true,
          sliderValue: value,
          nativeText: "Glass study",
          tabs: true,
          connectedSurfaces: true,
          dropdown: true,
        });
      } finally {
        await input.close();
      }
    });
  } finally {
    await closeOpticalComponents(session);
  }
}
