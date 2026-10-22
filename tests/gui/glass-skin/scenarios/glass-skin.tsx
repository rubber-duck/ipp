import { exerciseProductionOptics } from "./production-optics.js";
import { exerciseLiquidOptics } from "./liquid-optics.js";
import { exerciseGlassEdges } from "./edge-quality.js";
import { exercisePanelOpacity } from "./panel-opacity.js";
import { exerciseDropdownRowCorners } from "./dropdown-row-corners.js";
import { exerciseStepper } from "./stepper.js";
import { exerciseSkinOwnership } from "./skin-ownership.js";
import { exerciseOpticalContext } from "./optical-context.js";
import { exerciseOpticalComponents } from "./optical-components.js";
import { exerciseGlassValues } from "./composite-values.js";
import { exerciseGlassContainers } from "./containers.js";
import { exerciseGlassSelection } from "./selection.js";
import { exerciseGlassOverlays } from "./overlays.js";
import { exerciseGlassMaterialSharing } from "../composites/material-sharing.js";
import type {
  Client,
  HostClientBase,
  PresentationView,
  RootBinding,
} from "@ipp/client";
import { Entity, Children } from "@ipp/react";
import { Box, Font, Layout, Style } from "@ipp/react/gui";
import type { GuiKitContract } from "@ipp/react/gui-kit";
import { GlassButton } from "../../../../packages/ipp-react/src/gui-glass/actions.js";
import { GlassSkin } from "../../../../packages/ipp-react/src/gui-glass/skin.js";
import { resolveGlassPalette } from "../../../../packages/ipp-react/src/gui-glass/palette.js";
import type {
  GlassContract,
  GlassLight,
  GlassPaletteInput,
} from "../../../../packages/ipp-react/src/gui-glass/types.js";
import { crop, type RgbaImage } from "../../../../tools/shared-host/images.js";
import { image } from "../../../../tools/shared-host/presentation.js";
import { check } from "../../../harness/page/checks.js";
import {
  SpecimenSession,
  type Present,
} from "../../skin-lab/specimen-session.js";
import {
  defineSpecimen,
  type SpecimenContext,
} from "../../skin-lab/specimen.js";
import type { ThemeContract } from "../../skin-lab/theme.js";
import {
  ActionsSpecimen,
  ACTION_SPECIMEN_EXTENT,
  actionSpecimenBounds,
  actionStates,
} from "../actions-specimen.js";
import {
  probeGlassButtonStates,
  probeGlassOpticalFinish,
} from "../actions-oracle.js";
import { GlassValuesSpecimen } from "../specimens/values.js";
import switchScaleReference from "../specimens/switch-scale-reference.js";
import switchLightReference from "../specimens/switch-light-reference.js";
import { checkSwitchFocusScale } from "../switch-focus-oracle.js";
import { checkSwitchRimLight } from "../switch-light-oracle.js";
import { GLASS_VALUES_EXTENT } from "../fixtures/values.js";
import { glassActions } from "./actions.js";
import { glassValuesScenario } from "./values.js";
import { valuesDriver } from "../drivers/values.js";
import {
  checkGlassTransmission,
  checkGlassSliderThumb,
  checkGlassSwitchPlacement,
  checkGlassValueLightResponse,
} from "../values-oracle.js";

export { nativePresentationTransport } from "../../../../packages/ipp-client/src/native-presentation.js";
export { workerTransport } from "../../../../packages/ipp-client/src/worker.js";

type Contract = ThemeContract & GuiKitContract & GlassContract;
export interface GlassSkinImage {
  readonly name: string;
  readonly width: number;
  readonly height: number;
  readonly rgba: string;
}
export interface GlassSkinEnvironment {
  capture(image: GlassSkinImage): Promise<void>;
  record(value: object): Promise<void>;
}

function base64(bytes: Uint8Array): string {
  let text = "";
  for (let start = 0; start < bytes.length; start += 0x8000)
    text += String.fromCharCode(...bytes.subarray(start, start + 0x8000));
  return btoa(text);
}

export function glassSpecimen(
  contract: Contract,
  family: "actions" | "values" | "reference",
  palette: Partial<GlassPaletteInput> = {},
  light: Partial<GlassLight> = {},
) {
  const extent =
    family === "actions"
      ? ACTION_SPECIMEN_EXTENT
      : family === "values"
        ? GLASS_VALUES_EXTENT
        : ([262, 112] as const);
  const background = resolveGlassPalette(palette).background;
  return defineSpecimen({
    extent,
    states:
      family === "reference"
        ? [
            {
              name: "approved-button",
              cell: [0, 0, 262, 112],
              pin: [{ kind: "hover", at: [100, 54] }],
            },
          ]
        : family === "actions"
          ? actionStates
          : [{ name: "values", cell: [0, 0, ...extent] }],
    render: (lab: SpecimenContext) => (
      <GlassSkin
        contract={contract}
        font={lab.font}
        fontSize={16}
        palette={palette}
        light={light}
        reducedMotion
      >
        <Entity id="glass-page">
          <Layout
            kind={0}
            width={extent[0]}
            height={extent[1]}
            align_x={-1}
            align_y={-1}
          />
          <Style
            red={background[0]}
            green={background[1]}
            blue={background[2]}
            alpha={1}
          />
          <Box width={extent[0]} height={extent[1]} />
        </Entity>
        <Entity id="glass-content">
          <Layout
            kind={3}
            width={extent[0]}
            height={extent[1]}
            align_x={-1}
            align_y={-1}
          />
          <Font source={lab.font} font_size={16} />
          <Children>
            {family === "reference" ? (
              <Entity id="reference-button">
                <Layout
                  kind={0}
                  width={213.56}
                  height={59.2}
                  margin_left={20}
                  margin_top={25.8}
                  align_x={-1}
                  align_y={-1}
                />
                <Font source={lab.font} font_size={21.5} />
                <GlassButton
                  label="Action"
                  ref={lab.control("reference-button")}
                />
              </Entity>
            ) : family === "actions" ? (
              <ActionsSpecimen control={lab.control} />
            ) : (
              <GlassValuesSpecimen control={lab.control} />
            )}
          </Children>
        </Entity>
      </GlassSkin>
    ),
  });
}

export async function glassSkin(
  host: HostClientBase<Client>,
  contract: Contract,
  font: Uint8Array<ArrayBuffer>,
  environment: GlassSkinEnvironment,
  region:
    | "edge-quality"
    | "liquid-optics"
    | "optical-components"
    | "production-optics"
    | "panel-backgrounds"
    | "optical-context"
    | "stepper"
    | "skin-ownership"
    | "appearance"
    | "editing"
    | "values"
    | "reference"
    | "focus-scale"
    | "material-sharing"
    | "composite-values"
    | "composite-containers"
    | "composite-selection"
    | "composite-overlays",
) {
  const present: Present = async <T,>(
    binding: RootBinding,
    section: (view: PresentationView) => Promise<T>,
  ) => {
    const view = await host.presentation.select(
      await host.presentation.surface(),
      binding,
    );
    try {
      return await section(view);
    } finally {
      await host.presentation.clear(view).catch(() => {});
    }
  };
  const save = async (name: string, frame: RgbaImage) =>
    environment.capture({
      name,
      width: frame.width,
      height: frame.height,
      rgba: base64(frame.pixels),
    });
  if (region === "production-optics") {
    await exerciseProductionOptics(host, contract, font, present, environment);
    return { region };
  }
  if (region === "liquid-optics") {
    await exerciseLiquidOptics(host, contract, font, present, environment);
    return { region };
  }
  if (region === "edge-quality") {
    await exerciseGlassEdges(host, contract, font, present, environment);
    return { region };
  }
  if (region === "panel-backgrounds") {
    await exercisePanelOpacity(host, contract, font, present, environment);
    await exerciseDropdownRowCorners(
      host,
      contract,
      font,
      present,
      environment,
    );
    return { region };
  }
  if (region === "stepper") {
    await exerciseStepper(host, contract, font, present, environment);
    return { region };
  }
  if (region === "skin-ownership") {
    await exerciseSkinOwnership(host, contract, font, present, environment);
    return { region };
  }
  if (region === "optical-context") {
    await exerciseOpticalContext(host, font, present, environment);
    return { region };
  }
  if (region === "optical-components") {
    await exerciseOpticalComponents(host, contract, font, present, environment);
    return { region };
  }
  if (region === "composite-values") {
    await exerciseGlassValues(host, contract, font, present, environment);
    return { region };
  }
  if (region === "composite-containers") {
    await exerciseGlassContainers(host, contract, font, present, environment);
    return { region };
  }
  if (region === "composite-selection") {
    await exerciseGlassSelection(host, contract, font, present, environment);
    return { region };
  }
  if (region === "composite-overlays") {
    await exerciseGlassOverlays(host, contract, font, present, environment);
    return { region };
  }
  if (region === "material-sharing") {
    await exerciseGlassMaterialSharing(
      host,
      contract,
      font,
      present,
      environment,
    );
    return { region };
  }
  if (region === "focus-scale") {
    for (const { specimen, name, probe } of [
      {
        specimen: switchScaleReference(contract),
        name: "switch-focus-scale",
        probe: checkSwitchFocusScale,
      },
      {
        specimen: switchLightReference(contract),
        name: "switch-light",
        probe: checkSwitchRimLight,
      },
    ]) {
      const session = await SpecimenSession.open({
        host,
        contract,
        font,
        specimen,
        themes: {},
        world: `glass/${name}`,
      });
      try {
        check(
          (await session.assetSettled("ipp-glass/shader")).status === "loaded",
          "Switch focus shader failed to load",
        );
        const captured = await session.capture(present);
        await save(name, captured.image);
        check(
          captured.leftover.length === 0,
          "Switch focus capture left interaction active",
        );
        await environment.record({
          [name]: probe(captured.image),
          routing: captured.states.map(({ name, routing }) => ({
            name,
            routing,
          })),
        });
      } finally {
        await session.close();
      }
    }
    return { region };
  }
  const family =
    region === "reference"
      ? "reference"
      : region === "values"
        ? "values"
        : "actions";
  {
    const session = await SpecimenSession.open({
      host,
      contract,
      font,
      specimen: glassSpecimen(contract, family),
      themes: {},
      world: `glass/${family}`,
    });
    try {
      check(
        (await session.assetSettled("ipp-glass/shader")).status === "loaded",
        "Glass shader failed to load",
      );
      if (region === "editing") {
        const actions = await session.interact(present, (interaction) =>
          glassActions(interaction, async () => {
            await session.update({
              specimen: glassSpecimen(contract, family, {
                primary: "#CF9CFF",
                secondary: "#785CFF",
              }),
            });
          }),
        );
        await environment.record({ actions });
        return { region };
      }
      const capture = await session.capture(present);
      await save(`${family}-cyan`, capture.image);
      check(
        capture.leftover.length === 0,
        "Glass state capture left interaction active",
      );
      for (const state of capture.states) {
        check(
          !state.routing.some((entry) =>
            /:(miss|blocked)|\(.*Error/.test(entry),
          ),
          `${state.name} input missed: ${state.routing}`,
        );
        await save(state.name, crop(capture.image, state.cell));
      }
      if (family === "reference") {
        const probes = probeGlassOpticalFinish(
          capture.image,
          [40, 51.6, 427.12, 118.4],
        );
        await environment.record({ reference: "liquid-glass", probes });
        for (const probe of probes)
          check(probe.passed, `${probe.name}: ${probe.detail}`);
        return { region };
      }
      if (family === "actions") {
        const button = (index: number) =>
          crop(
            capture.image,
            actionSpecimenBounds("button", index).map((n) => n * 2) as [
              number,
              number,
              number,
              number,
            ],
          );
        const probes = probeGlassButtonStates(
          {
            idle: button(0),
            hover: button(1),
            pressed: button(2),
            disabled: button(8),
            selectedDisabled: button(9),
          },
          [0, 0, 360, 80],
        );
        await environment.record({ family, probes });
        for (const probe of probes)
          check(probe.passed, `${probe.name}: ${probe.detail}`);
        await session.update({
          specimen: glassSpecimen(contract, family, {
            primary: "#CF9CFF",
            secondary: "#785CFF",
          }),
        });
        const violet = await session.capture(present, [
          "button-hover",
          "button-pressed",
        ]);
        for (const state of violet.states)
          await save(`${state.name}-violet`, crop(violet.image, state.cell));
        await session.update({
          specimen: glassSpecimen(
            contract,
            family,
            {},
            { intensity: 1, x: 0.1, y: 0.1 },
          ),
        });
        const leftCapture = await session.capture(present, ["button-hover"]);
        const left = crop(leftCapture.image, leftCapture.states[0]!.cell);
        await save("actions-light-left", left);
        await session.update({
          specimen: glassSpecimen(
            contract,
            family,
            {},
            { intensity: 1, x: 0.9, y: 0.9 },
          ),
        });
        const rightCapture = await session.capture(present, ["button-hover"]);
        const right = crop(rightCapture.image, rightCapture.states[0]!.cell);
        await save("actions-light-right", right);
        const lightDifference = difference(left, right);
        check(
          lightDifference > 0.05,
          `Glass moving light had no visible response: ${lightDifference}`,
        );
        await environment.record({ lightDifference });
      } else {
        checkGlassSwitchPlacement(capture.image);
        for (const [id, value, vertical] of [
          ["horizontal", 0.5, false],
          ["vertical", 0.5, true],
          ["range", 0.2, false],
          ["range", 0.8, false],
          ["vertical-range", 0.2, true],
          ["vertical-range", 0.8, true],
        ] as const)
          checkGlassSliderThumb(capture.image, id, value, vertical);
        await session.update({
          specimen: glassSpecimen(contract, family, { background: "#E55A88" }),
        });
        const contrast = await session.capture(present);
        await save("values-opacity-contrast", contrast.image);
        await environment.record({
          opaqueBodyDifference: checkGlassTransmission(
            capture.image,
            contrast.image,
          ),
        });
        await session.update({
          specimen: glassSpecimen(
            contract,
            family,
            {},
            { intensity: 1, x: 0.1, y: 0.1 },
          ),
        });
        const left = await session.capture(present);
        await save("values-light-left", left.image);
        await session.update({
          specimen: glassSpecimen(
            contract,
            family,
            {},
            { intensity: 1, x: 0.9, y: 0.9 },
          ),
        });
        const right = await session.capture(present);
        await save("values-light-right", right.image);
        await environment.record({
          valueLightResponse: checkGlassValueLightResponse(
            left.image,
            right.image,
          ),
        });
        await session.update({ specimen: glassSpecimen(contract, family) });
        await session.interact(present, (interaction) =>
          glassValuesScenario(
            valuesDriver(interaction, async (name, frame) =>
              save(name, image(frame)),
            ),
          ),
        );
      }
      await environment.record({
        family,
        states: capture.states.map(({ name, routing }) => ({ name, routing })),
      });
    } finally {
      await session.close();
    }
  }
  return { region };
}

function difference(a: RgbaImage, b: RgbaImage): number {
  let total = 0;
  for (let i = 0; i < a.pixels.length; i += 4)
    for (let c = 0; c < 3; c++)
      total += Math.abs(a.pixels[i + c]! - b.pixels[i + c]!);
  return total / (a.width * a.height * 3);
}
