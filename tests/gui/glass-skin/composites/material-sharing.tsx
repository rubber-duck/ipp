import type { Client, HostClientBase } from "@ipp/client";
import { Children, Entity, assetRef } from "@ipp/react";
import { Box, Layout, Paint, Style } from "@ipp/react/gui";
import { GlassSkin, resolveGlassPalette } from "@ipp/react/gui-glass";
import type { GlassKitContract } from "../../../../packages/ipp-react/src/gui-glass/kit-theme.js";
import { check } from "../../../harness/page/checks.js";
import { defineSpecimen } from "../../skin-lab/specimen.js";
import {
  SpecimenSession,
  type Present,
} from "../../skin-lab/specimen-session.js";
import type { ThemeContract } from "../../skin-lab/theme.js";
import type { GlassSkinEnvironment } from "../scenarios/glass-skin.js";

type Contract = GlassKitContract & ThemeContract;
const extent = [840, 284] as const;

function specimen(contract: Contract, edited: boolean) {
  const p = resolveGlassPalette();
  return defineSpecimen({
    extent,
    states: [{ name: "shared-materials", cell: [0, 0, ...extent] }],
    render: (lab) => (
      <GlassSkin contract={contract} font={lab.font} reducedMotion>
        <Entity id="material-grid">
          <Layout
            width={extent[0]}
            height={extent[1]}
            kind={3}
            align_x={-1}
            align_y={-1}
          />
          <Children>
            {Array.from({ length: 40 }, (_, index) => (
              <Entity key={index} id={`material-${index}`}>
                <Layout
                  kind={0}
                  width={92}
                  height={38}
                  margin_left={10 + (index % 8) * 104}
                  margin_top={10 + Math.floor(index / 8) * 54}
                  align_x={-1}
                  align_y={-1}
                />
                <Style
                  red={p.surface[0]}
                  green={p.surface[1]}
                  blue={p.surface[2]}
                  alpha={1}
                />
                <Box width={92} height={38} />
                <Paint
                  source={assetRef("ipp-glass/shader")}
                  accent={edited && index === 0 ? p.warning : p.primary}
                  glassTint={p.secondary}
                  light={[0.2, 0.1, 1.2, 1]}
                  finish={[1.5, edited && index === 0 ? 0.65 : 0.2, 0.8, 1]}
                  lighting={[0, 0, 0, 0]}
                  body={[6, 0, 1, 0]}
                />
              </Entity>
            ))}
          </Children>
        </Entity>
      </GlassSkin>
    ),
  });
}

/** Real-frame proof that 40 independent paint owners share, then split safely. */
export async function exerciseGlassMaterialSharing(
  host: HostClientBase<Client>,
  contract: Contract,
  font: Uint8Array<ArrayBuffer>,
  present: Present,
  environment: GlassSkinEnvironment,
) {
  const session = await SpecimenSession.open({
    host,
    contract,
    font,
    specimen: specimen(contract, false),
    themes: {},
    world: "glass/material-sharing",
  });
  try {
    check(
      (await session.assetSettled("ipp-glass/shader")).status === "loaded",
      "Glass shared material must load",
    );
    const before = (await session.capture(present)).image;
    await session.update({ specimen: specimen(contract, true) });
    const after = (await session.capture(present)).image;
    const scale = before.width / extent[0];
    let changed = 0,
      elsewhere = 0;
    for (let y = 0; y < before.height; y++)
      for (let x = 0; x < before.width; x++) {
        const i = (y * before.width + x) * 4;
        const difference = Math.max(
          ...[0, 1, 2].map((c) =>
            Math.abs(before.pixels[i + c]! - after.pixels[i + c]!),
          ),
        );
        if (difference > 2) {
          if (
            x >= 10 * scale &&
            x < 102 * scale &&
            y >= 10 * scale &&
            y < 48 * scale
          )
            changed++;
          else elsewhere++;
        }
      }
    check(
      changed > 30,
      "Editing one shared paint must change its optical finish",
    );
    check(
      elsewhere === 0,
      "Editing one shared paint must not mutate any other owner",
    );
    // A fallback would leave later instances flat: every far cell must retain a bright rim.
    for (let index = 0; index < 40; index++) {
      const x = Math.floor((10 + (index % 8) * 104 + 0.25) * scale),
        y = Math.floor((10 + Math.floor(index / 8) * 54 + 19) * scale);
      const i = (y * before.width + x) * 4;
      check(
        before.pixels[i + 1]! > 35,
        `Material ${index} lost its shader to the uniform budget`,
      );
    }
    let bytes = "";
    for (let offset = 0; offset < after.pixels.length; offset += 0x8000)
      bytes += String.fromCharCode(
        ...after.pixels.subarray(offset, offset + 0x8000),
      );
    await environment.capture({
      name: "glass-material-sharing",
      width: after.width,
      height: after.height,
      rgba: btoa(bytes),
    });
    await environment.record({
      independentPaintOwners: 40,
      changedPixels: changed,
      otherOwnersChangedPixels: elsewhere,
    });
  } finally {
    await session.close();
  }
}
