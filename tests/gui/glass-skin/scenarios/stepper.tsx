import type { Client, HostClientBase } from "@ipp/client";
import { cybertronSkin } from "@ipp/react/gui-cybertron";
import { glassSkin } from "@ipp/react/gui-glass";
import { image } from "../../../../tools/shared-host/presentation.js";
import { crop, type RgbaImage } from "../../../../tools/shared-host/images.js";
import { check } from "../../../harness/page/checks.js";
import { symbols, readControl } from "../../composites/pages/composites.js";
import {
  SpecimenSession,
  type Present,
} from "../../skin-lab/specimen-session.js";
import {
  stepperSpecimen,
  STEPPER_EXTENT,
  type StepperFixtureState,
} from "../../skin-lab/specimens/f08-stepper.js";
import type { GlassSkinEnvironment } from "./glass-skin.js";

/** Same native input, identity and pixel assertions on GLES and worker/WebGL. */
export async function exerciseStepper(
  host: HostClientBase<Client>,
  contract: Parameters<typeof SpecimenSession.open>[0]["contract"],
  font: Uint8Array<ArrayBuffer>,
  present: Present,
  environment: GlassSkinEnvironment,
) {
  const state: StepperFixtureState = { reports: [] };
  const cybertron = cybertronSkin(contract);
  const glass = glassSkin(contract, {
    palette: { primary: "#D9B6FF", secondary: "#9360D5" },
    reducedMotion: true,
  });
  const session = await SpecimenSession.open({
    host,
    contract,
    font,
    specimen: stepperSpecimen(contract, cybertron, state),
    themes: {},
    world: "wizard-stepper-scenario",
  });
  const save = async (name: string, frame: RgbaImage) => {
    let binary = "";
    for (let offset = 0; offset < frame.pixels.length; offset += 0x8000)
      binary += String.fromCharCode(
        ...frame.pixels.subarray(offset, offset + 0x8000),
      );
    await environment.capture({
      name,
      width: frame.width,
      height: frame.height,
      rgba: btoa(binary),
    });
  };
  const changed = (a: RgbaImage, b: RgbaImage) => {
    let count = 0;
    for (let i = 0; i < a.pixels.length; i += 4)
      if (
        [0, 1, 2].some(
          (c) => Math.abs(a.pixels[i + c]! - b.pixels[i + c]!) > 12,
        )
      )
        count++;
    return count;
  };
  let beforeSymbols = new Map<string, bigint>();
  try {
    const original = (await session.capture(present)).image;
    await save("stepper-cybertron", original);
    await session.interact(present, async (interaction) => {
      beforeSymbols = await symbols(interaction.client);
      const read = async (name: string) => {
        const entity = beforeSymbols.get(name);
        check(entity !== undefined, `Missing step ${name}`);
        const control = await readControl(
          interaction.client as Parameters<typeof readControl>[0],
          entity,
        );
        check(control, `No native step control ${name}`);
        return control;
      };
      const click = async (name: string) => {
        const [x, y, w, h] = (await read(name)).bounds;
        const point = [
          (x + w / 2) / STEPPER_EXTENT[0],
          (y + h / 2) / STEPPER_EXTENT[1],
        ] as const;
        for (const kind of ["pointerMove", "pointerDown", "pointerUp"] as const)
          await interaction.input.send({ kind, pointer: 945n, point });
        await interaction.capture();
      };
      check(
        (await read("wizard/step/review")).fields.selected === true,
        "Initial current step missing",
      );
      check(
        state.reports.length === 0,
        "Initial selection emitted a user change",
      );
      await click("wizard/step/verify");
      check(
        (await read("wizard/step/review")).fields.selected === true,
        "Disabled step changed selection",
      );
      await click("wizard/step/review");
      await interaction.input.send({ kind: "key", key: "right" });
      await interaction.capture();
      check(
        (await read("wizard/step/done")).focused,
        "Arrow navigation did not skip disabled Verify",
      );
      check(
        (await read("wizard/step/review")).fields.selected === true,
        "Focus movement advanced the wizard",
      );
      await interaction.input.send({ kind: "key", key: "enter" });
      const activated = image(await interaction.capture());
      check(
        (await read("wizard/step/done")).fields.selected === true,
        "Enter did not select Done",
      );
      const bounds = (await read("wizard/step/done")).bounds;
      const scale = original.width / STEPPER_EXTENT[0];
      const region = bounds.map((value) => Math.round(value * scale)) as [
        number,
        number,
        number,
        number,
      ];
      check(
        changed(crop(original, region), crop(activated, region)) > 200,
        "Current-step activation did not visibly change its button paint",
      );
      check(
        (await symbols(interaction.client)).has("stepper-content-done"),
        "Selected step content did not mount",
      );
      await click("wizard/step/details");
      check(
        (await read("wizard/step/details")).fields.selected === true,
        "Pointer did not revisit completed step",
      );
      check(
        state.reports.join(",") === "done,details",
        `Unexpected changes ${state.reports}`,
      );
      await click("vertical-wizard/step/details");
      await interaction.input.send({ kind: "key", key: "down" });
      await interaction.capture();
      check(
        (await read("vertical-wizard/step/review")).focused,
        "Vertical arrows did not move focus",
      );
      await interaction.input.send({ kind: "key", key: "space" });
      await interaction.capture();
      check(
        (await read("vertical-wizard/step/review")).fields.selected === true,
        "Space did not select the vertical step",
      );
      await click("wizard/step/review");
    });
    await session.update({ specimen: stepperSpecimen(contract, glass, state) });
    for (const asset of glass.assetIds ?? []) {
      const loaded = await session.assetSettled(`wizard-skin/${asset}`);
      check(
        loaded.status === "loaded",
        `Glass asset unavailable: ${loaded.error}`,
      );
    }
    const reskinned = (await session.capture(present)).image;
    check(
      changed(original, reskinned) > 1000,
      "Skin replacement did not change rendered appearance",
    );
    await save("stepper-glass", reskinned);
    state.reversed = true;
    await session.update({ specimen: stepperSpecimen(contract, glass, state) });
    await session.interact(present, async (interaction) => {
      const after = await symbols(interaction.client);
      for (const [name, identity] of beforeSymbols)
        if (name.startsWith("wizard/step/"))
          check(
            after.get(name) === identity,
            `Reskin/reorder replaced ${name}`,
          );
      const selected = await readControl(
        interaction.client as Parameters<typeof readControl>[0],
        after.get("wizard/step/review")!,
      );
      check(
        selected?.fields.selected === true,
        "Reskin/reorder lost current step",
      );
      check(
        selected.fields.label === "3",
        "Reordering did not update step numbering",
      );
    });
    await environment.record({
      wizardStepper: {
        reports: state.reports,
        disabledSkipped: true,
        keyboardActivation: true,
        verticalNavigation: true,
        stableReskinAndReorder: true,
      },
    });
  } finally {
    await session.close();
  }
}
