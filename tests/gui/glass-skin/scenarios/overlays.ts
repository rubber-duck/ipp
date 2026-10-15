import type { Client, HostClientBase } from "@ipp/client";
import { image } from "../../../../tools/shared-host/presentation.js";
import { check } from "../../../harness/page/checks.js";
import {
  symbols,
  readControl,
  overlayVisible,
  componentFields,
} from "../../composites/pages/composites.js";
import {
  SpecimenSession,
  type Present,
  type SpecimenInteraction,
} from "../../skin-lab/specimen-session.js";
import type { GlassSkinEnvironment } from "./glass-skin.js";
import {
  overlaysSpecimen,
  OVERLAYS_EXTENT,
} from "../composites/overlays-specimen.js";

export async function exerciseGlassOverlays(
  host: HostClientBase<Client>,
  contract: Parameters<typeof overlaysSpecimen>[0],
  font: Uint8Array<ArrayBuffer>,
  present: Present,
  environment: GlassSkinEnvironment,
) {
  const session = await SpecimenSession.open({
    host,
    contract,
    font,
    specimen: overlaysSpecimen(contract),
    themes: {},
    world: "glass-overlays-scenario",
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
  let client: Client;
  let known: Map<string, bigint>;
  const read = async (name: string) => {
    const id = known.get(name);
    check(id, `Missing ${name}`);
    return readControl(client as Parameters<typeof readControl>[0], id);
  };
  const click = async (interaction: SpecimenInteraction, name: string) => {
    await environment.record({ probe: name, phase: "begin" });
    const control = await read(name);
    check(control, `No native control ${name}`);
    const [x, y, w, h] = control.bounds;
    const point = [
      (x + w / 2) / OVERLAYS_EXTENT[0],
      (y + h / 2) / OVERLAYS_EXTENT[1],
    ] as const;
    for (const kind of ["pointerMove", "pointerDown", "pointerUp"] as const)
      await interaction.input.send({ kind, pointer: 812n, point });
    return interaction.capture();
  };
  try {
    await session.assetSettled("ipp-glass/shader");
    await session.interact(present, async (interaction) => {
      client = interaction.client;
      known = await symbols(client);
      const popover = await click(interaction, "glass-popover");
      check(
        (await read("glass-popover"))?.fields.selected === true,
        "Popover did not open",
      );
      await save("overlays-popover-open", image(popover));
      await interaction.input.send({ kind: "key", key: "escape" });
      await interaction.capture();
      check(
        (await read("glass-popover"))?.fields.selected === false,
        "Escape did not dismiss popover",
      );
    });
    await session.interact(present, async (interaction) => {
      await interaction.input.send({
        kind: "pointerMove",
        pointer: 812n,
        point: [700 / 900, 50 / 640],
      });
      const deadline = Date.now() + 5000;
      while (
        !(await overlayVisible(client!, known.get("glass-tooltip")!)) &&
        Date.now() < deadline
      ) {
        await interaction.capture();
        await new Promise((resolve) => setTimeout(resolve, 100));
      }
      check(
        await overlayVisible(client!, known.get("glass-tooltip")!),
        "Native tooltip did not open",
      );
      await save("overlays-tooltip-open", image(await interaction.capture()));
    });
    await session.interact(present, async (interaction) => {
      await click(interaction, "context-target");
      await interaction.input.send({ kind: "key", key: "contextMenu" });
      const context = await interaction.capture();
      check(
        await overlayVisible(client!, known.get("glass-context/surface")!),
        "Context menu did not open",
      );
      await save("overlays-context-open", image(context));
      await interaction.input.send({ kind: "key", key: "escape" });
    });
    await session.interact(present, async (interaction) => {
      const dialog = await click(interaction, "dialog-trigger");
      check(
        await overlayVisible(client!, known.get("glass-dialog")!),
        "Dialog did not open",
      );
      await save("overlays-dialog-open", image(dialog));
      await interaction.input.send({ kind: "key", key: "escape" });
      await interaction.capture();
    });
    await session.interact(present, async (interaction) => {
      await click(interaction, "dialog-trigger");
      const confirmed = await click(interaction, "glass-dialog/action");
      check(
        !(await overlayVisible(client!, known.get("glass-dialog")!)),
        "Confirm did not close dialog",
      );
      await save("overlays-dialog-confirmed", image(confirmed));
    });
    const identities = await symbols(client!);
    const before = await session.capture(present);
    await save("overlays-cyan", before.image);
    await session.update({
      specimen: overlaysSpecimen(
        contract,
        "overview",
        { primary: "#CF9CFF", secondary: "#785CFF" },
        { x: 0.9, y: 0.9, intensity: 1 },
      ),
    });
    const after = await session.capture(present);
    await save("overlays-violet-light", after.image);
    let changed = 0;
    for (let i = 0; i < before.image.pixels.length; i += 4) {
      let d = 0;
      for (let c = 0; c < 3; c++)
        d += Math.abs(before.image.pixels[i + c]! - after.image.pixels[i + c]!);
      if (d > 24) changed++;
    }
    check(changed > 500, "Overlay material ignored palette/light");
    const final = await symbols(client!);
    for (const [name, id] of identities)
      check(final.get(name) === id, `Retheme replaced ${name}`);
    await session.update({ specimen: overlaysSpecimen(contract, "toast") });
    const toasts = await session.capture(present);
    await save("overlays-toasts", toasts.image);
    const toastProof = await assertToastFrame(client!, toasts.image);
    await environment.record({ toastProof });
    await environment.record({
      family: "overlays",
      changed,
      retainedIdentities: identities.size,
      escapeDismissal: true,
      confirm: true,
      contextMenu: true,
    });
  } finally {
    await session.close();
  }
}

export async function assertToastFrame(
  client: Client,
  frame: { width: number; height: number; pixels: Uint8Array },
) {
  const known = await symbols(client!);
  const toastProof: object[] = [];
  let previousBottom = 0;
  for (const key of ["saved", "warning", "error"]) {
    const control = await readControl(
      client as Parameters<typeof readControl>[0],
      known.get(`glass-toasts/${key}`)!,
    );
    check(control, `Missing toast ${key}`);
    const [x, y, width, height] = control.bounds;
    check(
      Math.abs(width - 260) < 0.1 && Math.abs(height - 38) < 0.1,
      `Toast ${key} collapsed: ${control.bounds}`,
    );
    check(
      x >= 0 && y >= 0 && x + width <= 900 && y + height <= 640,
      `Toast ${key} lies outside the canvas`,
    );
    check(y >= previousBottom, `Toast ${key} overlaps another card`);
    previousBottom = y + height;
    const textId = known.get(`glass-toasts/${key}/text`);
    check(textId, `Missing toast ${key} label`);
    const page = await client!.inspectPage({
      collection: "entities",
      target: textId,
      limit: 1,
    });
    const bounds = componentFields(
      client! as Parameters<typeof componentFields>[0],
      page,
      textId,
      "CanvasBounds",
    );
    check(bounds, `Missing toast ${key} text bounds`);
    const tx = Number(bounds.x),
      ty = Number(bounds.y);
    const tw = Number(bounds.width),
      th = Number(bounds.height);
    check(
      tx >= x && ty >= y && tx + tw <= x + width && ty + th <= y + height,
      `Toast ${key} label extends outside its card`,
    );
    const sx = frame.width / 900,
      sy = frame.height / 640;
    let glyphPixels = 0,
      facePixels = 0,
      faceSamples = 0;
    for (let py = Math.ceil(ty * sy); py < Math.floor((ty + th) * sy); py++)
      for (let px = Math.ceil(tx * sx); px < Math.floor((tx + tw) * sx); px++) {
        const i = (py * frame.width + px) * 4;
        if (
          frame.pixels[i]! > 190 &&
          frame.pixels[i + 1]! > 190 &&
          frame.pixels[i + 2]! > 190
        )
          glyphPixels++;
      }
    for (
      let py = Math.ceil((y + 5) * sy);
      py < Math.floor((y + height - 5) * sy);
      py++
    )
      for (
        let px = Math.ceil((x + 5) * sx);
        px < Math.floor((x + width - 5) * sx);
        px++
      ) {
        const i = (py * frame.width + px) * 4;
        faceSamples++;
        if (
          Math.abs(frame.pixels[i]!) +
            Math.abs(frame.pixels[i + 1]! - 18) +
            Math.abs(frame.pixels[i + 2]! - 28) >
          8
        )
          facePixels++;
      }
    check(glyphPixels > 80, `Toast ${key} label has no visible glyphs`);
    check(
      facePixels / faceSamples > 0.8,
      `Toast ${key} has no complete painted card`,
    );
    if (key === "saved") {
      // The specimen's help label runs behind the toast in this otherwise empty
      // band, beyond "Changes saved" and before the dismiss button.
      let leakedGlyphs = 0;
      for (
        let py = Math.ceil((y + 13) * sy);
        py < Math.floor((y + 27) * sy);
        py++
      )
        for (
          let px = Math.ceil((x + 135) * sx);
          px < Math.floor((x + 155) * sx);
          px++
        ) {
          const i = (py * frame.width + px) * 4;
          if (Math.min(...frame.pixels.subarray(i, i + 3)) > 130)
            leakedGlyphs++;
        }
      check(
        leakedGlyphs === 0,
        `Toast underlay text competes with its label: ${leakedGlyphs}`,
      );
    }
    let darkFace = 0,
      darkSamples = 0,
      railHits = 0,
      railSamples = 0;
    for (
      let py = Math.ceil((y + 8) * sy);
      py < Math.floor((y + height - 8) * sy);
      py++
    ) {
      for (
        let px = Math.ceil((x + 20) * sx);
        px < Math.floor((x + width - 30) * sx);
        px++
      ) {
        const i = (py * frame.width + px) * 4;
        darkSamples++;
        if (
          Math.max(
            frame.pixels[i]!,
            frame.pixels[i + 1]!,
            frame.pixels[i + 2]!,
          ) < 110
        )
          darkFace++;
      }
    }
    // A polished sheet has directional glints, not a uniformly bright semantic rail.
    for (let py = Math.floor(y * sy); py < Math.ceil((y + 2) * sy); py++)
      for (
        let px = Math.ceil((x + 10) * sx);
        px < Math.floor((x + width - 10) * sx);
        px++
      ) {
        const i = (py * frame.width + px) * 4;
        railSamples++;
        if (Math.max(...frame.pixels.subarray(i, i + 3)) > 80) railHits++;
      }
    check(railHits > 30, `Toast ${key} polished edge is missing`);
    check(
      darkFace / darkSamples > 0.8,
      `Toast ${key} semantic color flooded its face`,
    );
    toastProof.push({
      key,
      bounds: control.bounds,
      textBounds: [tx, ty, tw, th],
      glyphPixels,
      railFraction: railHits / railSamples,
      darkFaceFraction: darkFace / darkSamples,
      paintedFaceFraction: facePixels / faceSamples,
    });
  }

  return toastProof;
}
