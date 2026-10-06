/**
 * The runtime's default GUI look through a real Host: every skin-lab
 * specimen is declared with its theme modules, which leave controls on their
 * default look (the switch and amber variants name the built-in looks), and
 * captured in each of its interaction states with real physical input or
 * semantic actions. The composite of every state's cell is checked by the
 * region probes. The environment supplies the Host connection, the shared
 * font and where images and evidence go; nothing here depends on the process
 * or transport arrangement.
 */
import type {
  Client,
  HostClientBase,
  PresentationView,
  RootBinding,
} from "@ipp/client";
import {
  SpecimenSession,
  type Present,
} from "../../skin-lab/specimen-session.js";
import type { SkinSpecimen } from "../../skin-lab/specimen.js";
import type { GuiKitContract } from "@ipp/react/gui-kit";
import type { SkinThemes, ThemeContract } from "../../skin-lab/theme.js";
import button from "../../skin-lab/specimens/a01-button.js";
import items from "../../skin-lab/specimens/a01-button-items.js";
import checkbox from "../../skin-lab/specimens/a02-checkbox.js";
import switches from "../../skin-lab/specimens/a03-switch.js";
import slider from "../../skin-lab/specimens/a04-slider.js";
import textInput from "../../skin-lab/specimens/a05-text-input.js";
import scrollView from "../../skin-lab/specimens/a06-scroll-view.js";
import virtualList from "../../skin-lab/specimens/a07-virtual-list.js";
import rotaryKnob from "../../skin-lab/specimens/d01-rotary-knob.js";
import numericStepper from "../../skin-lab/specimens/d02-numeric-stepper.js";
import verticalSlider from "../../skin-lab/specimens/e01-vertical-slider.js";
import bipolarSlider from "../../skin-lab/specimens/e02-bipolar-slider.js";
import { themes as buttonThemes } from "../../skin-lab/themes/button.js";
import { themes as checkboxThemes } from "../../skin-lab/themes/checkbox.js";
import { themes as itemThemes } from "../../skin-lab/themes/items.js";
import { themes as scrollThemes } from "../../skin-lab/themes/scroll.js";
import { themes as sliderThemes } from "../../skin-lab/themes/slider.js";
import { themes as textInputThemes } from "../../skin-lab/themes/text-input.js";
import { overlapOrder } from "./overlap.js";
import { panelPaints } from "./paints.js";
import { probe, type ProbeResult } from "../default-skin-oracle.js";

export { nativePresentationTransport } from "../../../../packages/ipp-client/src/native-presentation.js";
export { workerTransport } from "../../../../packages/ipp-client/src/worker.js";

/** Theme modules by the names specimens give them. */
const THEMES: Readonly<Record<string, SkinThemes>> = {
  button: buttonThemes,
  items: itemThemes,
  checkbox: checkboxThemes,
  slider: sliderThemes,
  "text-input": textInputThemes,
  scroll: scrollThemes,
};

const SPECIMENS: readonly (readonly [string, SkinSpecimen])[] = [
  ["a01-button", button],
  ["a01-button-items", items],
  ["a02-checkbox", checkbox],
  ["a03-switch", switches],
  ["a04-slider", slider],
  ["a05-text-input", textInput],
  ["a06-scroll-view", scrollView],
  ["a07-virtual-list", virtualList],
  ["d01-rotary-knob", rotaryKnob],
  ["d02-numeric-stepper", numericStepper],
  ["e01-vertical-slider", verticalSlider],
  ["e02-bipolar-slider", bipolarSlider],
];

/** One specimen's composite capture as RGBA bytes in base64. */
export interface DefaultSkinImage {
  readonly name: string;
  readonly width: number;
  readonly height: number;
  readonly rgba: string;
}

export interface DefaultSkinEnvironment {
  capture(image: DefaultSkinImage): Promise<void>;
  record(value: object): Promise<void>;
}

function base64(bytes: Uint8Array): string {
  let text = "";
  for (let start = 0; start < bytes.length; start += 0x8000)
    text += String.fromCharCode(...bytes.subarray(start, start + 0x8000));
  return btoa(text);
}

/**
 * Declare, capture and probe every specimen in its own temporary World, one
 * after another on the Host's surface, and fail with every unmet probe.
 */
export async function defaultSkin(
  host: HostClientBase<Client>,
  contract: ThemeContract & GuiKitContract,
  font: Uint8Array<ArrayBuffer>,
  environment: DefaultSkinEnvironment,
): Promise<Record<string, number>> {
  const present: Present = async <T>(
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
  const failures: string[] = [];
  const passed: Record<string, number> = {};
  for (const [name, specimen] of SPECIMENS) {
    const themes = specimen.theme ? THEMES[specimen.theme] : {};
    if (!themes)
      throw new Error(`${name} names unknown theme module ${specimen.theme}`);
    const session = await SpecimenSession.open({
      host,
      contract,
      font,
      specimen,
      themes,
      world: `gui-default-skin/${name}`,
    });
    try {
      const capture = await session.capture(present);
      await environment.capture({
        name,
        width: capture.image.width,
        height: capture.image.height,
        rgba: base64(capture.image.pixels),
      });
      const results: ProbeResult[] = probe(name, capture.image);
      await environment.record({
        specimen: name,
        routing: Object.fromEntries(
          capture.states.map((state) => [state.name, state.routing]),
        ),
        leftover: capture.leftover,
        results,
      });
      // No pinned input may miss, be blocked or fail, and nothing may
      // outlive the release of a state.
      for (const state of capture.states)
        for (const entry of state.routing) {
          const step = /^(\w+):(\w+)(\(.*\))?$/.exec(entry);
          if (step && (step[2] === "miss" || step[2] === "blocked" || step[3]))
            failures.push(`${name}/${state.name}: input ${entry}`);
        }
      if (capture.leftover.length)
        failures.push(
          `${name}: feedback left after release ${capture.leftover.join(" ")}`,
        );
      for (const result of results)
        if (!result.passed)
          failures.push(`${name}: ${result.name} ${result.detail}`);
      passed[name] = results.filter((result) => result.passed).length;
    } finally {
      await session.close();
    }
  }
  // Custom paints on panels: patterns, frames, glow, the fallback of a paint
  // that does not compile and a property write.
  const paints = await panelPaints(
    host,
    contract,
    font,
    present,
    async ({ name, image }) =>
      environment.capture({
        name,
        width: image.width,
        height: image.height,
        rgba: base64(image.pixels),
      }),
  );
  await environment.record({
    specimen: "p01-panel-paints",
    assets: paints.assets,
    results: paints.results,
  });
  for (const result of paints.results)
    if (!result.passed)
      failures.push(`p01-panel-paints: ${result.name} ${result.detail}`);
  passed["p01-panel-paints"] = paints.results.filter(
    (result) => result.passed,
  ).length;
  // Text under shapes painted after it: a dialog, a toast, stacked panels and
  // a raised layer, against the same scene drawn one layer at a time.
  const overlap = await overlapOrder(
    host,
    contract,
    font,
    present,
    async ({ name, image }) =>
      environment.capture({
        name,
        width: image.width,
        height: image.height,
        rgba: base64(image.pixels),
      }),
  );
  await environment.record({ specimen: "overlap", results: overlap });
  for (const result of overlap)
    if (!result.passed)
      failures.push(`overlap: ${result.name} ${result.detail}`);
  passed.overlap = overlap.filter((result) => result.passed).length;
  if (failures.length)
    throw new Error(`Default skin probes failed:\n${failures.join("\n")}`);
  return passed;
}
