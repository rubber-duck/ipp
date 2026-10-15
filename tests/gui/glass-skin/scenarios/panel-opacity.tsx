import type { Client, HostClientBase } from "@ipp/client";
import { Entity } from "@ipp/react";
import { Behavior, Box, Layout, Style } from "@ipp/react/gui";
import { Button, GuiKit, Panel, type GuiKitContract } from "@ipp/react/gui-kit";
import {
  GlassButton,
  GlassPanel,
  GlassSkin,
  glassSkin,
} from "@ipp/react/gui-glass";
import { cybertronSkin } from "@ipp/react/gui-cybertron";
import { check } from "../../../harness/page/checks.js";
import { defineSpecimen } from "../../skin-lab/specimen.js";
import {
  SpecimenSession,
  type Present,
} from "../../skin-lab/specimen-session.js";
import type { ThemeContract } from "../../skin-lab/theme.js";
import type { GlassSkinEnvironment } from "./glass-skin.js";

const EXTENT = [920, 320] as const;

function linear(value: number) {
  const srgb = value / 255;
  return srgb <= 0.04045 ? srgb / 12.92 : ((srgb + 0.055) / 1.055) ** 2.4;
}

/** Black/white backdrop changes measure actual transmitted light through the body. */
export async function exercisePanelOpacity(
  host: HostClientBase<Client>,
  contract: GuiKitContract & ThemeContract,
  font: Uint8Array<ArrayBuffer>,
  present: Present,
  environment: GlassSkinEnvironment,
) {
  const app = glassSkin(contract, { reducedMotion: true });
  const cybertron = cybertronSkin(contract);
  const reflectionSkins = [1, 0].map((reflectionStrength) =>
    glassSkin(contract, { reflectionStrength, reducedMotion: true }),
  );
  const specimen = (backdrop: number, lightX = 0.08) =>
    defineSpecimen({
      extent: EXTENT,
      states: [{ name: "panel-opacity", cell: [0, 0, ...EXTENT] }],
      render: (lab) => (
        <>
          <Entity id="opacity-backdrop">
            <Layout width={EXTENT[0]} height={EXTENT[1]} />
            <Style red={backdrop} green={backdrop} blue={backdrop} />
            <Box width={EXTENT[0]} height={EXTENT[1]} />
          </Entity>
          {[undefined, 0.82].map((opacity, index) => (
            <GlassSkin
              key={index}
              id={`opacity-direct-${index}`}
              contract={contract}
              font={lab.font}
              reducedMotion
              reflectionStrength={index === 0 ? 1 : 0}
              light={{ x: lightX }}
              {...(index === 1 ? { palette: { background: "#183A48" } } : {})}
              {...(opacity === undefined ? {} : { panelOpacity: opacity })}
            >
              <Entity id={`opacity-panel-${index}`}>
                <Layout
                  width={200}
                  height={160}
                  margin_left={20 + index * 230}
                  margin_top={20}
                />
                <GlassPanel />
              </Entity>
              <Entity id={`reflection-direct-button-${index}`}>
                <Layout
                  width={200}
                  height={60}
                  margin_left={20 + index * 230}
                  margin_top={220}
                />
                <Behavior enabled />
                <GlassButton />
              </Entity>
            </GlassSkin>
          ))}
          {reflectionSkins.map((skin, index) => (
            <GuiKit
              key={index}
              id={`reflection-kit-${index}`}
              contract={contract}
              font={lab.font}
              skin={skin}
              reducedMotion
            >
              <Button
                id={`reflection-kit-button-${index}`}
                label=""
                width={200}
                height={60}
                layout={{ margin_left: 480 + index * 230, margin_top: 220 }}
              />
            </GuiKit>
          ))}
          {[app, cybertron].map((skin, index) => (
            <GuiKit
              key={index}
              id={`opacity-kit-${index}`}
              contract={contract}
              font={lab.font}
              skin={skin}
              reducedMotion
            >
              <Panel
                id={`opacity-kit-panel-${index}`}
                width={200}
                height={160}
                layout={{ margin_left: 480 + index * 230, margin_top: 20 }}
              />
            </GuiKit>
          ))}
        </>
      ),
    });
  const session = await SpecimenSession.open({
    host,
    contract,
    font,
    specimen: specimen(0),
    themes: {},
    world: "glass-panel-opacity",
  });
  try {
    const dark = (await session.capture(present)).image;
    await session.update({ specimen: specimen(1) });
    const bright = (await session.capture(present)).image;
    await session.update({ specimen: specimen(0, 0.92) });
    const movedLight = (await session.capture(present)).image;
    const scale = bright.width / EXTENT[0];
    const transmission = [120, 350, 580, 810].map((center) => {
      let sum = 0;
      let count = 0;
      for (let y = 85 * scale; y < 115 * scale; y++)
        for (let x = (center - 15) * scale; x < (center + 15) * scale; x++) {
          const at = (y * bright.width + x) * 4;
          for (const channel of [0, 1, 2]) {
            sum += Math.max(
              0,
              linear(bright.pixels[at + channel]!) -
                linear(dark.pixels[at + channel]!),
            );
            count++;
          }
        }
      return sum / count;
    });
    for (const [name, frame] of [
      ["dark", dark],
      ["bright", bright],
      ["light-moved", movedLight],
    ] as const) {
      let binary = "";
      for (let offset = 0; offset < frame.pixels.length; offset += 0x8000)
        binary += String.fromCharCode(
          ...frame.pixels.subarray(offset, offset + 0x8000),
        );
      await environment.capture({
        name: `panel-opacity-${name}`,
        width: frame.width,
        height: frame.height,
        rgba: btoa(binary),
      });
    }
    for (const index of [0, 2])
      check(
        Math.abs(transmission[index]! - 0.02) < 0.006,
        `Default sheets transmit about 2% of backdrop light (${transmission[index]})`,
      );
    check(
      Math.abs(transmission[1]! - 0.18) < 0.01,
      `An explicit panelOpacity controls transmission (${transmission[1]})`,
    );
    check(
      transmission[3]! < 0.025,
      "Peer Cybertron panels keep their opaque body",
    );
    const lightChanges = [120, 350].map((center) => {
      let largest = 0;
      for (let y = 40 * scale; y < 160 * scale; y++)
        for (let x = (center - 70) * scale; x < (center + 70) * scale; x++) {
          const at = (y * dark.width + x) * 4;
          for (const channel of [0, 1, 2])
            largest = Math.max(
              largest,
              Math.abs(
                dark.pixels[at + channel]! - movedLight.pixels[at + channel]!,
              ),
              Math.abs(
                dark.pixels[at + channel]! -
                  dark.pixels[
                    (100 * scale * dark.width + center * scale) * 4 + channel
                  ]!,
              ),
            );
        }
      return largest;
    });
    check(
      lightChanges.every((change) => change <= 1),
      `Panel bodies stay flat when the material light moves (${lightChanges})`,
    );
    const reflections = [20, 480].map((left) => {
      let removedFaceEnergy = 0;
      let retainedEdgeEnergy = 0;
      let samples = 0;
      let changedFacePixels = 0;
      let faceBandPeak = 0;
      for (let y = 232 * scale; y < 268 * scale; y++)
        for (let x = (left + 12) * scale; x < (left + 188) * scale; x++) {
          const on = (y * dark.width + x) * 4;
          const off = on + 230 * scale * 4;
          let difference = 0;
          for (const channel of [0, 1, 2]) {
            difference = Math.max(
              difference,
              dark.pixels[on + channel]! - dark.pixels[off + channel]!,
            );
            removedFaceEnergy += Math.max(
              0,
              linear(dark.pixels[on + channel]!) -
                linear(dark.pixels[off + channel]!),
            );
          }
          if (difference > 2) changedFacePixels++;
          faceBandPeak = Math.max(faceBandPeak, difference);
          samples += 3;
        }
      for (let y = 220 * scale; y < 280 * scale; y++)
        for (let x = (left + 230) * scale; x < (left + 430) * scale; x++) {
          if (
            y > 228 * scale &&
            y < 272 * scale &&
            x > (left + 238) * scale &&
            x < (left + 422) * scale
          )
            continue;
          const at = (y * dark.width + x) * 4;
          retainedEdgeEnergy = Math.max(
            retainedEdgeEnergy,
            linear(dark.pixels[at + 1]!),
          );
        }
      return {
        removedFaceEnergy: removedFaceEnergy / samples,
        retainedEdgeEnergy,
        changedFaceFraction: changedFacePixels / (samples / 3),
        faceBandPeak,
      };
    });
    for (const response of reflections) {
      check(
        // Quiet idle bands need not emit the old absolute brightness. They
        // must affect a broad interior region above rounding noise, while the
        // independently shaded perimeter retains stronger reflected energy.
        response.changedFaceFraction > 0.2 &&
          response.faceBandPeak > 8 &&
          response.removedFaceEnergy > response.retainedEdgeEnergy * 0.01,
        `Turning off quiet bands removes coherent reflected face energy (${JSON.stringify(response)})`,
      );
      check(
        response.retainedEdgeEnergy > 0.02,
        `Turning off bands keeps the lit glass edge (${JSON.stringify(response)})`,
      );
    }
    const custom = (100 * scale * dark.width + 350 * scale) * 4;
    for (const [channel, value] of [0x18, 0x3a, 0x48].entries())
      check(
        Math.abs(
          linear(dark.pixels[custom + channel]!) - linear(value) * 0.82,
        ) < 0.004,
        "Flat panels preserve their configured palette background",
      );
    await environment.record({
      panelTransmission: transmission,
      panelLightChanges: lightChanges,
      reflections,
    });
  } finally {
    await session.close();
  }
}
