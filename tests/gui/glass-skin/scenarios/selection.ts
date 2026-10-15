import type { Client, HostClientBase } from "@ipp/client";
import { image } from "../../../../tools/shared-host/presentation.js";
import { check } from "../../../harness/page/checks.js";
import {
  symbols,
  readControl,
  overlayVisible,
} from "../../composites/pages/composites.js";
import {
  SpecimenSession,
  type Present,
  type SpecimenInteraction,
} from "../../skin-lab/specimen-session.js";
import type { GlassSkinEnvironment } from "./glass-skin.js";
import {
  selectionSpecimen,
  SELECTION_EXTENT,
} from "../composites/selection-specimen.js";
import { contourSpecimen } from "../composites/contour-specimen.js";
import { switchSpecimen } from "../composites/switch-specimen.js";
import { checkSwitchGeometry } from "../switch-geometry-oracle.js";

export async function exerciseGlassSelection(
  host: HostClientBase<Client>,
  contract: Parameters<typeof selectionSpecimen>[0],
  font: Uint8Array<ArrayBuffer>,
  present: Present,
  environment: GlassSkinEnvironment,
) {
  const session = await SpecimenSession.open({
    host,
    contract,
    font,
    specimen: selectionSpecimen(contract),
    themes: {},
    world: "glass-selection-scenario",
  });
  const save = async (
    name: string,
    frame: { width: number; height: number; pixels: Uint8Array },
  ) => {
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
  let known: Map<string, bigint>;
  let client: Client;
  const read = async (name: string) => {
    const id = known.get(name);
    check(id, `Missing ${name}`);
    return readControl(client as Parameters<typeof readControl>[0], id);
  };
  const click = async (interaction: SpecimenInteraction, name: string) => {
    await environment.record({ probe: name, phase: "begin" });
    const control = await read(name);
    check(control, `No control ${name}`);
    const [x, y, w, h] = control.bounds;
    const point = [
      (x + w / 2) / SELECTION_EXTENT[0],
      (y + h / 2) / SELECTION_EXTENT[1],
    ] as const;
    for (const kind of ["pointerMove", "pointerDown", "pointerUp"] as const)
      await interaction.input.send({ kind, pointer: 811n, point });
    await interaction.capture();
  };
  try {
    await session.assetSettled("ipp-glass/shader");
    await session.interact(present, async (interaction) => {
      client = interaction.client;
      known = await symbols(client);
      await click(interaction, "glass-radio/amber/mark");
      check(
        (await read("glass-radio/amber/mark"))?.fields.selected === true,
        "Radio did not select native mark",
      );
      await click(interaction, "glass-radio/off/mark");
      check(
        (await read("glass-radio/amber/mark"))?.fields.selected === true,
        "Disabled radio changed selection",
      );
    });
    await session.interact(present, async (interaction) => {
      await click(interaction, "glass-segments/amber");
      check(
        (await read("glass-segments/amber"))?.fields.selected === true,
        "Segment selection failed",
      );
      await click(interaction, "glass-tabs/tab/amber");
      check(
        (await read("glass-tabs/tab/amber"))?.fields.selected === true,
        "Tab selection failed",
      );
    });
    await session.interact(present, async (interaction) => {
      await click(interaction, "glass-dropdown");
      check(
        await overlayVisible(client!, known.get("glass-dropdown/list")!),
        "Dropdown did not open",
      );
      await save("selection-dropdown-open", image(await interaction.capture()));
      await interaction.input.send({ kind: "key", key: "escape" });
    });
    await session.interact(present, async (interaction) => {
      await click(interaction, "glass-multi");
      await click(interaction, "glass-multi/options/amber");
      check(
        (await read("glass-multi/options/amber"))?.fields.selected === true,
        "Multi-select did not toggle native option",
      );
      await save("selection-multi-open", image(await interaction.capture()));
      await interaction.input.send({ kind: "key", key: "escape" });
    });
    await session.interact(present, async (interaction) => {
      const focus = await interaction
        .control("autocomplete")
        .action({ kind: "focus" });
      check(focus.ok, "Autocomplete focus refused");
      const changed = await interaction
        .control("autocomplete")
        .action({ kind: "text", value: "Amber" });
      check(changed.ok, "Autocomplete native edit refused");
      check(
        (await interaction.control("autocomplete").read()).text === "Amber",
        "Autocomplete lost committed native text",
      );
      await save("selection-editing", image(await interaction.capture()));
    });
    const identities = await symbols(client!);
    const before = await session.capture(present);
    await save("selection-cyan", before.image);
    await session.update({
      specimen: selectionSpecimen(
        contract,
        { primary: "#CF9CFF", secondary: "#785CFF" },
        { x: 0.9, y: 0.9, intensity: 1 },
      ),
    });
    const after = await session.capture(present);
    await save("selection-violet-light", after.image);
    let changed = 0;
    for (let i = 0; i < before.image.pixels.length; i += 4) {
      let delta = 0;
      for (let c = 0; c < 3; c++)
        delta += Math.abs(
          before.image.pixels[i + c]! - after.image.pixels[i + c]!,
        );
      if (delta > 24) changed++;
    }
    check(changed > 500, "Selection material ignored palette/light");
    const final = await symbols(client!);
    for (const [name, id] of identities)
      check(final.get(name) === id, `Retheme replaced ${name}`);
    await environment.record({
      family: "selection",
      changed,
      retainedIdentities: identities.size,
      nativeSelection: true,
      disabledRejection: true,
      dropdownEscape: true,
      nativeTextEdit: true,
    });
    await session.update({ specimen: contourSpecimen(contract) });
    const contours = await session.capture(present);
    await save("selection-contours-both-skins", contours.image);
    const pixel = (x: number, y: number) => {
      const offset =
        (Math.floor(y * 2) * contours.image.width + Math.floor(x * 2)) * 4;
      return contours.image.pixels.subarray(offset, offset + 3);
    };
    // The liquid capsule covers the middle of its end cap while its outer
    // corner remains clear. Selected/focus tint must follow that same contour.
    for (const y of [64, 352, 544]) {
      const rgb = pixel(512, y + 24);
      const outside = pixel(504, y + 24);
      check(
        rgb[1]! > outside[1]! + 60 && rgb[2]! > outside[2]! + 60,
        `Glass selected/focus shoulder is detached at ${y}: ${rgb} vs page ${outside}`,
      );
      const corner = pixel(512, y + 4);
      check(
        corner.every(
          (value, channel) => Math.abs(value - outside[channel]!) < 3,
        ),
        `Glass state fills the capsule's clear corner at ${y}: ${corner}`,
      );
    }
    // Focus follows the capsule: the old straight underline escaped these
    // lower corners. It must also remain visible along the inside upper rim.
    for (const [y, ends] of [
      [352, [10]],
      [448, [414]],
      [544, [10, 414]],
    ] as const) {
      const page = pixel(504, y + 50);
      for (const dx of ends) {
        const corner = pixel(508 + dx, y + 50);
        check(
          corner.every(
            (value, channel) => Math.abs(value - page[channel]!) < 3,
          ),
          `Glass focus escaped the capsule at ${y}/${dx}: ${corner}`,
        );
      }
    }
    check(
      pixel(548, 355.5)[1]! > pixel(548, 67.5)[1]! + 15,
      "Glass keyboard focus must remain visible inside the upper contour",
    );
    // The same baseline must remain visible continuously through the tab
    // underline and the unselected rail, in both client skins. The shared
    // 36-unit row starts at 680, so its last half-unit is the baseline.
    for (const x of [28, 508]) {
      for (let dx = 4; dx < 420; dx += 4) {
        const rgb = pixel(x + dx, 715.5);
        check(
          rgb[1]! > 60 && rgb[2]! > 70,
          `Broken tab rail at ${x + dx}: ${rgb}`,
        );
      }
      const above = pixel(x + 4, 690);
      if (x === 28)
        check(
          above[1]! < 80,
          `Cybertron idle tab grew a separate boxed frame: ${above}`,
        );
      else {
        // A shared glass parent can reflect here; reject an isolated inset ridge.
        const below = pixel(x + 4, 692);
        check(
          Math.abs(above[1]! - below[1]!) < 25,
          `Glass tab grew an inset bevel: ${above} / ${below}`,
        );
      }
    }
    await environment.record({
      family: "selection-contours",
      joinedCorners: true,
      continuousTabRail: true,
      skins: ["cybertron", "glass"],
    });
    await session.update({ specimen: switchSpecimen(contract) });
    const switches = await session.capture(present);
    await save("switch-contours-both-skins", switches.image);
    checkSwitchGeometry(switches.image);
  } finally {
    await session.close();
  }
}
