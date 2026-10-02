/**
 * The skin lab as a shared-Host client module: opens named specimens, reloads
 * their specimen and theme modules before every capture, and returns each
 * capture with its reference side-by-side. See README.md.
 */
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { parseArgs } from "node:util";
import {
  defineClient,
  type ClientContext,
} from "../../tools/shared-host/client.js";
import type { RgbaImage } from "../../tools/shared-host/images.js";
import { decodePng } from "../../tools/shared-host/png.js";
import { captureImages } from "./results.js";
import { SpecimenSession } from "./specimen-session.js";
import type { SkinSpecimen } from "./specimen.js";
import type { GuiKitContract } from "@ipp/react/gui-kit";
import type { SkinThemes, ThemeContract } from "./theme.js";

const OPTIONS = {
  states: { type: "string" },
  zoom: { type: "string" },
  "zoom-factor": { type: "string" },
  raw: { type: "boolean" },
  references: { type: "string" },
} as const;

interface Loaded<T> {
  readonly value: T;
  readonly identity: string;
}

interface LabSpecimen {
  readonly name: string;
  specimen: Loaded<SkinSpecimen>;
  themes: Loaded<SkinThemes>;
  readonly session: SpecimenSession;
}

function parse(args: readonly string[]) {
  return parseArgs({
    args: [...args],
    options: OPTIONS,
    allowPositionals: true,
  });
}

function list(value: string | undefined): string[] | undefined {
  return value
    ?.split(",")
    .map((entry) => entry.trim())
    .filter(Boolean);
}

async function loadSpecimen(
  context: ClientContext,
  name: string,
  previous?: string,
): Promise<Loaded<SkinSpecimen> | null> {
  const path = join(
    context.workspace,
    "tests/skin-lab/specimens",
    `${name}.tsx`,
  );
  if (!existsSync(path)) throw new Error(`No specimen module ${path}`);
  const loaded = await context.load(path, previous);
  if (!loaded) return null;
  const value = loaded.module.default as SkinSpecimen | undefined;
  if (!value?.states || !value.render)
    throw new Error(`Specimen ${name} has no defineSpecimen default export`);
  return { value, identity: loaded.identity };
}

async function loadThemes(
  context: ClientContext,
  theme: string | undefined,
  previous?: string,
): Promise<Loaded<SkinThemes> | null> {
  if (!theme) return previous === "" ? null : { value: {}, identity: "" };
  const loaded = await context.load(
    join(context.workspace, "tests/skin-lab/themes", `${theme}.ts`),
    previous,
  );
  if (!loaded) return null;
  const value = loaded.module.themes as SkinThemes | undefined;
  if (!value || typeof value !== "object")
    throw new Error(`Theme module ${theme} has no themes export`);
  return { value, identity: loaded.identity };
}

/** The ignored reference crops beside the primary checkout. */
function referenceDirectory(workspace: string, option?: string): string {
  const selected = option ?? process.env.IPP_SKIN_REFERENCES;
  if (selected) return resolve(selected);
  const common = spawnSync(
    "git",
    ["rev-parse", "--path-format=absolute", "--git-common-dir"],
    { cwd: workspace, encoding: "utf8" },
  );
  const root = common.status === 0 ? dirname(common.stdout.trim()) : workspace;
  return join(root, ".resources/skin-reference");
}

/** Reload edited modules: themes as row writes, a specimen by re-declaring. */
async function reload(context: ClientContext, lab: LabSpecimen) {
  const specimen = await loadSpecimen(context, lab.name, lab.specimen.identity);
  const theme = (specimen ?? lab.specimen).value.theme;
  const themes = await loadThemes(
    context,
    theme,
    theme === lab.specimen.value.theme ? lab.themes.identity : undefined,
  );
  const writes =
    specimen || themes
      ? await lab.session.update({
          ...(specimen ? { specimen: specimen.value } : {}),
          ...(themes ? { themes: themes.value } : {}),
        })
      : {};
  // Only an applied module counts as loaded, so a module that failed to
  // apply fails every capture until it changes.
  if (specimen) lab.specimen = specimen;
  if (themes) lab.themes = themes;
  return { specimen: !!specimen, theme: !!themes, writes };
}

export default defineClient<LabSpecimen[]>({
  async open(context, args) {
    const names = parse(args).positionals;
    if (!names.length) throw new Error("Name at least one specimen");
    const font = await context.font();
    const opened: LabSpecimen[] = [];
    try {
      for (const name of names) {
        const specimen = (await loadSpecimen(context, name))!;
        const themes = (await loadThemes(context, specimen.value.theme))!;
        const session = await SpecimenSession.open({
          host: context.host,
          contract: context.contract as unknown as ThemeContract &
            GuiKitContract,
          font,
          specimen: specimen.value,
          themes: themes.value,
          world: `skin-lab/${context.name}/${name}`,
        });
        opened.push({ name, specimen, themes, session });
      }
    } catch (error) {
      for (const lab of opened) await lab.session.close().catch(() => {});
      throw error;
    }
    return opened;
  },

  async capture(state, context, args) {
    const { values, positionals } = parse(args);
    const selected = positionals.length
      ? positionals.map((name) => {
          const lab = state.find((entry) => entry.name === name);
          if (!lab) throw new Error(`Specimen ${name} is not open here`);
          return lab;
        })
      : state;
    const references = referenceDirectory(context.workspace, values.references);
    const images: Record<string, RgbaImage> = {};
    const summary: string[] = [];
    const report: Record<string, unknown> = {};
    for (const lab of selected) {
      const started = performance.now();
      const reloaded = await reload(context, lab);
      const reloadMs = performance.now() - started;
      const capture = await lab.session.capture(
        context.present,
        list(values.states),
      );
      const reference = lab.specimen.value.reference;
      const referencePath = reference && join(references, reference.image);
      Object.assign(
        images,
        captureImages(lab.name, lab.specimen.value, capture, {
          ...(referencePath && existsSync(referencePath)
            ? { reference: decodePng(await readFile(referencePath)) }
            : {}),
          ...(values.zoom ? { zoom: list(values.zoom)! } : {}),
          ...(values["zoom-factor"]
            ? { zoomFactor: Number(values["zoom-factor"]) }
            : {}),
          ...(values.raw ? { raw: true } : {}),
        }),
      );
      const routing = capture.states
        .filter((entry) => entry.routing.length)
        .map((entry) => `  ${entry.name}: ${entry.routing.join(" ")}`);
      const timings = { reloadMs, ...capture.timings };
      summary.push(
        `${lab.name}: reloaded specimen ${reloaded.specimen}, theme ${reloaded.theme}${Object.keys(reloaded.writes).length ? `, wrote ${JSON.stringify(reloaded.writes)}` : ""}`,
        `  timings ms: ${JSON.stringify(Object.fromEntries(Object.entries(timings).map(([key, value]) => [key, Math.round(value)])))}`,
        ...routing,
        ...(capture.leftover.length
          ? [`  left after release: ${capture.leftover.join(" ")}`]
          : []),
        ...(referencePath && !existsSync(referencePath)
          ? [`  no reference crop at ${referencePath}`]
          : []),
      );
      report[lab.name] = {
        reloaded,
        timings,
        routing: Object.fromEntries(
          capture.states.map((entry) => [entry.name, entry.routing]),
        ),
        leftover: capture.leftover,
      };
    }
    return { images, summary, report };
  },

  async close(state) {
    for (const lab of state) await lab.session.close().catch(() => {});
  },
});
