import type { Client, HostClientBase } from "@ipp/client";
import { check } from "../../../harness/page/checks.js";
import {
  SpecimenSession,
  type Present,
} from "../../skin-lab/specimen-session.js";
import type { GlassSkinEnvironment } from "./glass-skin.js";
import type { RgbaImage } from "../../../../tools/shared-host/images.js";
import { valuesSpecimen } from "../composites/values-specimen.js";
import { readControl } from "../../composites/pages/composites.js";

function base64(bytes: Uint8Array) {
  let result = "";
  for (let start = 0; start < bytes.length; start += 0x8000)
    result += String.fromCharCode(...bytes.subarray(start, start + 0x8000));
  return btoa(result);
}

function differences(a: RgbaImage, b: RgbaImage) {
  let changed = 0;
  for (let i = 0; i < a.pixels.length; i += 4)
    if (
      Math.max(
        ...[0, 1, 2].map((c) => Math.abs(a.pixels[i + c]! - b.pixels[i + c]!)),
      ) > 8
    )
      changed++;
  return changed;
}

/** Real native controls, live appearance replacement and completed frame evidence. */
export async function exerciseGlassValues(
  host: HostClientBase<Client>,
  contract: Parameters<typeof valuesSpecimen>[0],
  font: Uint8Array<ArrayBuffer>,
  present: Present,
  environment: GlassSkinEnvironment,
) {
  const session = await SpecimenSession.open({
    host,
    contract,
    font,
    specimen: valuesSpecimen(contract),
    themes: {},
    world: "glass-composite-values",
  });
  const save = async (name: string, image: RgbaImage) =>
    environment.capture({
      name,
      width: image.width,
      height: image.height,
      rgba: base64(image.pixels),
    });
  try {
    const asset = await session.assetSettled("ipp-glass/shader");
    check(asset.status === "loaded", `Glass values paint ${asset.status}`);
    const first = await session.capture(present, ["values-feedback"]);
    check(
      first.leftover.length === 0,
      "Values reference left native input feedback",
    );
    await save("composite-values-cyan", first.image);
    const sample = (x: number, y: number) =>
      [0, 1, 2].map(
        (c) => first.image.pixels[(y * first.image.width + x) * 4 + c]!,
      );
    const colorBounds = await session.interact(
      present,
      async (interaction) =>
        (await readControl(
          interaction.client as Parameters<typeof readControl>[0],
          interaction.control("color").target.entity,
        ))!.bounds,
    );
    // This fixture uses 12px text at 2x capture density: its native 144-unit
    // SV field is 216px square, inset 12px from the evaluated control bounds.
    const fieldX = Math.round(colorBounds[0] * 2 + 12);
    const fieldY = Math.round(colorBounds[1] * 2 + 12);
    const white = sample(fieldX + 12, fieldY + 4),
      cyan = sample(fieldX + 204, fieldY + 4),
      black = sample(fieldX + 112, fieldY + 204),
      red = sample(fieldX + 252, fieldY + 4);
    check(
      Math.min(...white) > 180 && Math.max(...black) < 40,
      "Native SV field lost its white-to-black value range",
    );
    check(
      cyan[2]! > 180 && cyan[1]! > 100 && cyan[0]! < 80,
      "Native SV field lost its authored blue/cyan hue",
    );
    check(
      red[0]! > 220 && red[1]! < 80,
      "Native hue field lost the red endpoint",
    );

    const controls = ["labelled", "range", "dial", "number", "color"];
    const before = await session.interact(present, async (interaction) =>
      Promise.all(
        controls.map(async (name) => ({
          name,
          target: interaction.control(name).target,
          fields: await interaction.control(name).read(),
        })),
      ),
    );
    await session.update({
      specimen: valuesSpecimen(
        contract,
        { primary: "#CF9CFF", secondary: "#785CFF" },
        { x: 0.9, y: 0.9, intensity: 1 },
      ),
    });
    const violet = await session.capture(present, ["values-feedback"]);
    const changed = differences(first.image, violet.image);
    let colorFieldChanges = 0;
    for (let y = fieldY + 4; y < fieldY + 212; y++)
      for (let x = fieldX + 4; x < fieldX + 212; x++) {
        // Native marker chrome may follow the palette; the HSV field must not.
        if ((x - fieldX - 151) ** 2 + (y - fieldY - 43) ** 2 < 18 ** 2)
          continue;
        const offset = (y * first.image.width + x) * 4;
        if (
          [0, 1, 2].some(
            (c) =>
              first.image.pixels[offset + c] !==
              violet.image.pixels[offset + c],
          )
        )
          colorFieldChanges++;
      }
    check(
      colorFieldChanges === 0,
      "Glass palette tinted the native saturation/value color field",
    );
    check(changed > 500, "Composite palette/light failed to alter real pixels");
    await save("composite-values-violet", violet.image);
    await session.update({
      specimen: valuesSpecimen(
        contract,
        { primary: "#CF9CFF", secondary: "#785CFF" },
        { x: 0.15, y: 0.1, intensity: 1 },
      ),
    });
    const lightLeft = await session.capture(present, ["values-feedback"]);
    const lightChanges = differences(violet.image, lightLeft.image);
    check(
      lightChanges > 200,
      "Composite procedural glass did not respond to isolated light movement",
    );
    await save("composite-values-light-left", lightLeft.image);

    await session.update({
      specimen: valuesSpecimen(
        contract,
        { primary: "#CF9CFF", secondary: "#785CFF", background: "#E55A88" },
        { x: 0.9, y: 0.9, intensity: 1 },
      ),
    });
    const contrast = await session.capture(present, ["values-feedback"]);
    let bodyChanges = 0;
    for (let y = 67; y < 72; y++)
      for (let x = 800; x < 1120; x++) {
        const offset = (y * violet.image.width + x) * 4;
        if (
          [0, 1, 2].some(
            (c) =>
              contrast.image.pixels[offset + c] !==
              violet.image.pixels[offset + c],
          )
        )
          bodyChanges++;
      }
    check(bodyChanges === 0, "Filled progress glass leaked the backdrop");
    check(
      differences(violet.image, contrast.image) > 3000,
      "Contrast background did not change",
    );
    await save("composite-values-opacity", contrast.image);
    await session.update({
      specimen: valuesSpecimen(
        contract,
        { primary: "#CF9CFF", secondary: "#785CFF" },
        { x: 0.9, y: 0.9, intensity: 1 },
      ),
    });

    await session.interact(present, async (interaction) => {
      for (const previous of before) {
        const current = interaction.control(previous.name);
        check(
          current.target.entity === previous.target.entity &&
            current.target.component === previous.target.component &&
            current.target.incarnation === previous.target.incarnation,
          `Appearance replaced ${previous.name} native identity`,
        );
        const fields = await current.read();
        for (const field of ["value", "upper", "hue", "saturation", "alpha"])
          if (field in previous.fields)
            check(
              fields[field] === previous.fields[field],
              `Appearance changed ${previous.name}.${field}`,
            );
      }
      for (const name of ["labelled", "dial"]) {
        check(
          (await interaction.control(name).action({ kind: "focus", part: 0 }))
            .ok,
          `Cannot focus ${name}`,
        );
        await interaction.input.send({ kind: "key", key: "end" });
        check(
          Number((await interaction.control(name).read()).value) === 100,
          `${name} native End did not reach maximum`,
        );
      }
      check(
        (await interaction.control("range").action({ kind: "focus", part: 1 }))
          .ok,
        "Cannot focus upper range",
      );
      await interaction.input.send({ kind: "key", key: "end" });
      check(
        Number((await interaction.control("range").read()).upper) === 100,
        "Range upper End did not reach maximum",
      );
      check(
        (await interaction.control("number").action({ kind: "focus", part: 0 }))
          .ok,
        "Cannot focus numeric field",
      );
      await interaction.input.send({ kind: "key", key: "up" });
      check(
        Number((await interaction.control("number").read()).value) === 1.5,
        "Native numeric step failed",
      );
    });
    const stateBase = await session.capture(present, ["values-feedback"]);
    const states = await session.capture(present, [
      "slider-hover",
      "slider-focus",
      "dial-focus",
      "numeric-focus",
    ]);
    check(
      states.leftover.length === 0,
      "Value states left native focus/pointers",
    );
    for (const state of states.states) {
      check(
        differences(stateBase.image, state.image) > 20,
        `No rendered response for ${state.name}`,
      );
      await save(`composite-values-${state.name}`, state.image);
    }
    await session.update({
      specimen: valuesSpecimen(
        contract,
        { primary: "#CF9CFF", secondary: "#785CFF" },
        { x: 0.9, y: 0.9, intensity: 1 },
        true,
      ),
    });
    const disabled = await session.capture(present, ["values-feedback"]);
    await save("composite-values-disabled", disabled.image);
    await session.interact(present, async (interaction) => {
      for (const name of ["labelled", "range", "dial", "number", "color"]) {
        check(
          !(await interaction.control(name).action({ kind: "focus", part: 0 }))
            .ok,
          `Disabled ${name} accepted focus`,
        );
      }
    });
    await environment.record({
      region: "composite-values",
      paletteChangedPixels: changed,
      isolatedLightChangedPixels: lightChanges,
      nativeColorFieldChangedPixels: colorFieldChanges,
      opaqueProgressChangedPixels: bodyChanges,
      nativeIdentityPreserved: true,
      nativeKeyboardValues: true,
      families: controls.concat([
        "progress",
        "spinner",
        "circular-progress",
        "status",
        "alerts",
        "empty",
      ]),
    });
  } finally {
    await session.close();
  }
}
