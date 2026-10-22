import type { Client, HostClientBase } from "@ipp/client";
import type { GuiKitContract } from "@ipp/react/gui-kit";
import type { RgbaImage } from "../../../../tools/shared-host/images.js";
import { check } from "../../../harness/page/checks.js";
import {
  SpecimenSession,
  type Present,
} from "../../skin-lab/specimen-session.js";
import type { ThemeContract } from "../../skin-lab/theme.js";
import {
  measureGlassEdges,
  measureGlassRadiance,
} from "../edge-quality-oracle.js";
import { EDGE_EXTENT, edgeQualitySpecimen } from "../specimens/edge-quality.js";
import type { GlassSkinEnvironment } from "./glass-skin.js";

/** Real skin assets and completed frames; the driver owns the Host/transport. */
export async function exerciseGlassEdges(
  host: HostClientBase<Client>,
  contract: GuiKitContract & ThemeContract,
  font: Uint8Array<ArrayBuffer>,
  present: Present,
  environment: GlassSkinEnvironment,
) {
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
  // Resolve a real high-density frame in linear light. This tests the complete
  // reflection/coverage integral, independently of the material's sample scheme.
  let referenceBlack: RgbaImage | undefined;
  let referenceWhite: RgbaImage | undefined;
  for (const scale of [6, 1, 2, 3]) {
    const session = await SpecimenSession.open({
      host,
      contract,
      font,
      themes: {},
      specimen: edgeQualitySpecimen(contract, 0),
      world: `glass-edges-${scale}`,
      captureScale: scale,
    });
    try {
      const black = (await session.capture(present)).image;
      await session.update({ specimen: edgeQualitySpecimen(contract, 1) });
      const white = (await session.capture(present)).image;
      await save(`edges-black-${scale}x`, black);
      await save(`edges-white-${scale}x`, white);
      if (scale === 6) {
        referenceBlack = black;
        referenceWhite = white;
        continue;
      }
      check(
        referenceBlack && referenceWhite,
        "High-density edge references are ready",
      );
      const edges = measureGlassEdges(black, white, scale);
      const radiance = [
        ...measureGlassRadiance(black, referenceBlack, scale).map((value) => ({
          backdrop: "black",
          ...value,
        })),
        ...measureGlassRadiance(white, referenceWhite, scale).map((value) => ({
          backdrop: "white",
          ...value,
        })),
      ];
      await environment.record({
        region: "edge-quality",
        scale,
        edges,
        radiance,
      });
      check(
        black.width === EDGE_EXTENT[0] * scale &&
          black.height === EDGE_EXTENT[1] * scale,
        "Physical capture density",
      );
      for (const edge of edges) {
        const label = `${edge.sample} at ${scale}x`;
        check(
          edge.opacity > 0.65 && edge.opacity < 0.8,
          `${label}: translucent material is ready (${edge.opacity})`,
        );
        // Allow byte quantization and the local tangent's curvature error, but
        // reject double attenuation and logical-unit blur at high pixel density.
        check(
          edge.meanError < 0.006 && edge.maxError < 0.04,
          `${label}: integrated pixel coverage (${edge.meanError}, ${edge.maxError})`,
        );
      }
      for (const value of radiance) {
        check(
          value.rmsError < 0.0025 && value.maxError < 0.015,
          `${value.sample} at ${scale}x on ${value.backdrop}: pixel-integrated rim color (${value.rmsError}, ${value.maxError})`,
        );
      }
      await session.update({ specimen: edgeQualitySpecimen(contract) });
      await save(
        `edges-preview-${scale}x`,
        (await session.capture(present)).image,
      );
    } finally {
      await session.close();
    }
  }
}
