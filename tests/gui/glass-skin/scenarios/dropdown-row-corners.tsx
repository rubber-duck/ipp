import type { Client, HostClientBase } from "@ipp/client";
import { Dropdown, GuiKit, type GuiKitContract } from "@ipp/react/gui-kit";
import { glassSkin } from "@ipp/react/gui-glass";
import { check } from "../../../harness/page/checks.js";
import { defineSpecimen } from "../../skin-lab/specimen.js";
import {
  SpecimenSession,
  type Present,
} from "../../skin-lab/specimen-session.js";
import type { ThemeContract } from "../../skin-lab/theme.js";
import { image } from "../../../../tools/shared-host/presentation.js";
import type { RgbaImage } from "../../../../tools/shared-host/images.js";
import type { GlassSkinEnvironment } from "./glass-skin.js";

const EXTENT = [360, 180] as const;

/** Selected and physically hovered row tints stop at rounded inset corners. */
export async function exerciseDropdownRowCorners(
  host: HostClientBase<Client>,
  contract: GuiKitContract & ThemeContract,
  font: Uint8Array<ArrayBuffer>,
  present: Present,
  environment: GlassSkinEnvironment,
) {
  const skin = glassSkin(contract, {
    reflectionStrength: 0,
    reducedMotion: true,
  });
  const specimen = (selected: string) =>
    defineSpecimen({
      extent: EXTENT,
      states: [{ name: "dropdown-rows", cell: [0, 0, ...EXTENT] }],
      render: (lab) => (
        <GuiKit
          id="row-corners-skin"
          contract={contract}
          font={lab.font}
          skin={skin}
          reducedMotion
        >
          <Dropdown
            id="row-corners"
            label="Rows"
            open
            value={selected}
            options={[
              { key: "hovered", label: "Hovered row" },
              { key: "selected", label: "Selected row" },
            ]}
            layout={{ width: 300, margin_left: 20, margin_top: 20 }}
          />
        </GuiKit>
      ),
    });
  const session = await SpecimenSession.open({
    host,
    contract,
    font,
    specimen: specimen("selected"),
    themes: {},
    world: "glass-dropdown-row-corners",
  });
  try {
    const selected = (await session.capture(present)).image;
    await session.update({ specimen: specimen("none") });
    const idle = (await session.capture(present)).image;
    const { hovered, bounds } = await session.interact(
      present,
      async ({ client, input, capture }) => {
        const snapshot = await client.inspect();
        const bounds = ["hovered", "selected"].map((name) => {
          const fields = snapshot.entities
            .find(
              (entity) =>
                entity.metadata.symbolicId === `row-corners/options/${name}`,
            )
            ?.components.find(
              (component) =>
                component.component === client.components.CanvasBounds!.id,
            )?.fields;
          check(fields, `Evaluated ${name} row bounds exist`);
          return {
            x: Number(fields.x),
            y: Number(fields.y),
            width: Number(fields.width),
            height: Number(fields.height),
          };
        });
        const first = bounds[0]!;
        const outcome = await input.send({
          kind: "pointerMove",
          pointer: 51n,
          point: [
            (first.x + first.width / 2) / EXTENT[0],
            (first.y + first.height / 2) / EXTENT[1],
          ],
        });
        check(
          outcome.disposition === "routed",
          "Native hover reaches the dropdown row",
        );
        return { hovered: image(await capture()), bounds };
      },
    );
    const scale = idle.width / EXTENT[0];
    const difference = (frame: RgbaImage, x: number, y: number) => {
      const at =
        (Math.floor(y * scale) * idle.width + Math.floor(x * scale)) * 4;
      return Math.max(
        ...[0, 1, 2].map((channel) =>
          Math.abs(frame.pixels[at + channel]! - idle.pixels[at + channel]!),
        ),
      );
    };
    const responses = [hovered, selected].map((frame, index) => {
      const rect = bounds[index]!;
      const face = difference(
        frame,
        rect.x + rect.width * 0.8,
        rect.y + rect.height / 2,
      );
      const corner = difference(frame, rect.x + 0.5, rect.y + 0.5);
      return { state: index === 0 ? "hovered" : "selected", face, corner };
    });
    for (const [name, frame] of [
      ["idle", idle],
      ["hovered", hovered],
      ["selected", selected],
    ] as const) {
      let binary = "";
      for (let offset = 0; offset < frame.pixels.length; offset += 0x8000)
        binary += String.fromCharCode(
          ...frame.pixels.subarray(offset, offset + 0x8000),
        );
      await environment.capture({
        name: `dropdown-rows-${name}`,
        width: frame.width,
        height: frame.height,
        rgba: btoa(binary),
      });
    }
    for (const response of responses) {
      check(
        response.face > 3,
        `${response.state} row has a visible tint (${response.face})`,
      );
      check(
        response.corner < response.face * 0.25 + 1,
        `${response.state} row tint leaves its rounded corner clear (${response.corner})`,
      );
    }
    await environment.record({ dropdownRowCorners: responses });
  } finally {
    await session.close();
  }
}
