import {
  CanvasWorld,
  Children,
  Entity,
  FlatSurface,
  Transform,
  type CanvasWorldHandle,
} from "@ipp/react";
import {
  Behavior,
  Box,
  Button,
  Checkbox,
  Drawing,
  Font,
  Layout,
  Skin,
  Style,
  Text,
  Theme,
} from "@ipp/react/gui";
import type {
  GuiInputRoutingOutcome,
  GuiPhysicalContext,
  PresentationView,
  WorldReference,
} from "@ipp/client";
import {
  close as closeSurfaceFixture,
  initialize as initializeSurfaceFixture,
  surfaceFixture,
} from "../../../surfaces/pages/surface.js";

export * from "../../../surfaces/pages/surface.js";

type Color = readonly [number, number, number, number];

/** Straight linear RGBA of the panel's backing box. */
const PANEL_COLOR: Color = [0.02, 0.03, 0.05, 1];

/** The panel's physical Surface and logical Canvas extent at density 1. */
const PANEL_EXTENT = [3.8, 2.4] as const;

/** Systems of the attached Canvas World that owns the panel's GUI entities. */
const PANEL_SYSTEMS = [
  "ipp.animation",
  "ipp.gui",
  "ipp.gui-layout",
  "ipp.canvas",
  "ipp.asset-dependencies",
  "ipp.lifecycle-publisher",
] as const;

/** Row alignment shared by every managed child: top-left in its slot. */
const TOP_LEFT = { align_x: -1, align_y: -1 } as const;

/** One background part row of the generated GUI paint contract. */
interface PanelPartRow {
  part: number;
  color?: Color;
  corner_radius?: readonly [number, number];
  corner_cut?: readonly [number, number, number, number];
  border_width?: number;
  border_color?: Color;
  fill_mode?: number;
  gradient_start?: readonly [number, number];
  gradient_end?: readonly [number, number];
  gradient_color0?: Color;
  gradient_color1?: Color;
  glow_color?: Color;
  glow_intensity?: number;
  glow_radius?: number;
  glow_falloff?: number;
}

interface PanelPartTable {
  readonly nextSlot: number;
  readonly rows: ReadonlyMap<number, PanelPartRow>;
}

/** Target-generated part-row encoders; layouts belong to the running build. */
interface PanelContract {
  guiPaintPartIndex(key: { part: "background" }): number;
  GuiTheme: { encodeParts(table: PanelPartTable): Uint8Array<ArrayBuffer> };
  GuiSkin: { encodeParts(table: PanelPartTable): Uint8Array<ArrayBuffer> };
}

interface PanelConfig {
  variant: "mixed" | "mixed-without-glow" | "filled" | "sparse" | "empty";
  control?: boolean;
  /** Shape size, border and corner radius in logical units (metres at the default density). */
  shape: {
    width: number;
    height: number;
    borderWidth: number;
    cornerRadius: number;
  };
  angle?: number;
  /** Mixed-variant label overrides; defaults render "Gui" in green. */
  label?: { text?: string; color?: Color };
}

/** Batches and commands one panel render committed to its Canvas World. */
interface CanvasEdits {
  requests: number;
  /** Commands; React writes several changed fields of one component in one. */
  edits: number;
  /** Distinct entity components (or entities) the commands addressed. */
  components: number;
  /** Field values the commands wrote in place. */
  fields: number;
  /** Kinds of the commands that were not in-place field writes. */
  structural: string[];
}

/** One command's target and what it wrote, as the panel render committed it. */
interface CanvasCommand {
  target: string;
  /** Field values written in place: a field write, or an adopting component write. */
  fields: number;
  /** The command's kind when it is not an in-place field write. */
  structural?: string;
}

/** Batches and every command committed to one Canvas World. */
interface CanvasBatches {
  requests: number;
  commands: CanvasCommand[];
}

/** A Canvas World session the fixture opened to observe committed state. */
interface ObservedCanvas {
  readonly world: WorldReference;
  readonly session: ReturnType<typeof surfaceFixture.host.openWorld>;
}

let contract: PanelContract | undefined;
let panelConfig: PanelConfig | undefined;
let panelDensity = 1;
let panelCanvas: ObservedCanvas | undefined;
/**
 * Host name of the panel's Canvas World. Unmount releases the first presented
 * World's panel without destroying its Canvas World, which keeps its name.
 */
let panelWorldName = "retained-gui-panel-content";
let input: { view: PresentationView; context: GuiPhysicalContext } | undefined;

/** Batches and command targets per Canvas World, recorded at the generated client's batch entry. */
const canvasBatches = new Map<string, CanvasBatches>();

/** The entity and component one command addresses, and what it writes. */
function canvasCommand(command: object): CanvasCommand {
  const { kind, entity, component, fields, adopt } = command as {
    kind: string;
    entity?: { kind: string; id?: bigint; alias?: number; symbol?: string };
    component?: number;
    fields?: readonly unknown[];
    adopt?: boolean;
  };
  const name = entity?.id ?? entity?.alias ?? entity?.symbol;
  const target = `${entity?.kind}:${String(name)}:${String(component)}`;
  if (kind === "setField") return { target, fields: 1 };
  // An adopting component write sets only the listed fields of an existing
  // component, in place.
  if (kind === "insertComponent" && adopt)
    return { target, fields: fields?.length ?? 0 };
  return { target, fields: 0, structural: kind };
}

const worldKey = (world: WorldReference) => `${world.id}:${world.incarnation}`;

/**
 * Count every batch that sessions opened through this Host commit, keyed by
 * World. React opens each attached Canvas World's authoring session through
 * `openWorld`, so a panel render's writes are observed where the generated
 * client sends them.
 */
function countCanvasBatches() {
  const host = surfaceFixture.host;
  const open = host.openWorld.bind(host);
  host.openWorld = async (world) => {
    const session = await open(world);
    const batch = session.batch.bind(session);
    session.batch = (commands, ...options) => {
      const key = worldKey(world);
      const batches = canvasBatches.get(key) ?? { requests: 0, commands: [] };
      batches.requests += 1;
      batches.commands.push(...commands.map(canvasCommand));
      canvasBatches.set(key, batches);
      return batch(commands, ...options);
    };
    return session;
  };
}

/** Connect as the Surface fixture does, then prepare GUI panel authoring. */
export async function initialize(
  config: Parameters<typeof initializeSurfaceFixture>[0],
) {
  const initialized = await initializeSurfaceFixture(config);
  contract = (await import(config.generatedModuleUrl)) as PanelContract;
  countCanvasBatches();
  return initialized;
}

function observeCanvas(handle: CanvasWorldHandle) {
  if (panelCanvas && worldKey(panelCanvas.world) === worldKey(handle.world))
    return;
  const observed: ObservedCanvas = {
    world: handle.world,
    session: surfaceFixture.host.openWorld(handle.world),
  };
  panelCanvas = observed;
  void observed.session.catch(() => {});
  const release = async () => {
    if (panelCanvas === observed) panelCanvas = undefined;
    const session = await observed.session;
    if (!session.closure) await session.close();
  };
  void handle.closed.then(release, release).catch(() => {});
}

/**
 * A GUI panel on the same camera, viewport and DPR as the terminal workload,
 * authored as ordinary entities in the Canvas World attached to its Surface.
 * `mixed` combines a gradient shape with glow, atlas glyphs and a curve drawing;
 * `filled` and `sparse` differ only in the shape's interior fill. `control`
 * places a checkbox before the shape, under {@link CONTROL_POINT}, for pointer
 * interaction.
 */
export async function guiPanel(config: PanelConfig) {
  const [width, height] = [640, 480];
  await surfaceFixture.resizePresentation(width, height);
  panelConfig = config;
  // Count the batches and commands this render commits to the panel's Canvas
  // World, observed at the generated client's batch entry point.
  const recorded = () =>
    panelCanvas ? canvasBatches.get(worldKey(panelCanvas.world)) : undefined;
  const before = {
    requests: recorded()?.requests ?? 0,
    commands: recorded()?.commands.length ?? 0,
  };
  await renderPanel();
  if (!panelCanvas) throw new Error("The panel's Canvas World is not attached");
  const after = recorded() ?? { requests: 0, commands: [] };
  const commands = after.commands.slice(before.commands);
  const canvasEdits: CanvasEdits = {
    requests: after.requests - before.requests,
    edits: commands.length,
    components: new Set(commands.map((command) => command.target)).size,
    fields: commands.reduce((sum, command) => sum + command.fields, 0),
    structural: commands.flatMap((command) =>
      command.structural ? [command.structural] : [],
    ),
  };
  // The orthographic fixture camera spans ORTHO_HEIGHT metres vertically.
  return { pixelsPerMetre: height / surfaceFixture.cameraHeight, canvasEdits };
}

async function renderPanel() {
  const config = panelConfig;
  if (!config || !contract) throw new Error("No GUI panel is configured");
  const { variant, shape } = config;
  const background = contract.guiPaintPartIndex({ part: "background" });
  // A rounded rectangle: the row states away the default button look's
  // corner cuts, which it would otherwise sit on.
  const edge: PanelPartRow = {
    part: background,
    corner_radius: [shape.cornerRadius, shape.cornerRadius],
    corner_cut: [0, 0, 0, 0],
    border_width: shape.borderWidth,
    border_color: [1, 1, 1, 1],
  };
  const mixed = variant.startsWith("mixed");
  const theme = contract.GuiTheme.encodeParts({
    nextSlot: 1,
    rows: new Map([
      [
        0,
        mixed
          ? {
              ...edge,
              fill_mode: 1,
              gradient_start: [0, 0],
              gradient_end: [0, shape.height],
              gradient_color0: [1, 0.08, 0.04, 1],
              gradient_color1: [1, 0.8, 0.08, 1],
              ...(variant === "mixed"
                ? {
                    glow_color: [1, 0.35, 0.05, 1],
                    glow_intensity: 0.8,
                    glow_radius: 0.18,
                    glow_falloff: 2,
                  }
                : {}),
            }
          : edge,
      ],
    ]),
  });
  // The shape's own fill overrides the shared theme's appearance.
  const fill: Color =
    variant === "sparse" ? [0, 0, 0, 0] : [0.85, 0.25, 0.08, 1];
  const skin = contract.GuiSkin.encodeParts({
    nextSlot: 1,
    rows: new Map([[0, { part: background, color: fill }]]),
  });
  // The checkbox keeps its default look with round corners: cached and
  // direct images antialias the inner corner of a square stroke differently,
  // at one pixel by more than the cache comparison's tolerance.
  const rounded = contract.GuiSkin.encodeParts({
    nextSlot: 1,
    rows: new Map([
      [
        0,
        {
          part: background,
          corner_cut: [0, 0, 0, 0],
          corner_radius: [0.1, 0.1],
        },
      ],
    ]),
  });
  const label = config.label;
  await surfaceFixture.presentCached(() => (
    <>
      <Entity key="gui" id="retained-gui-panel">
        <Transform ry={config.angle ?? 0} />
        <FlatSurface width={PANEL_EXTENT[0]} height={PANEL_EXTENT[1]} />
        {surfaceFixture.cacheDeclaration("retained-gui-panel")}
      </Entity>
      <CanvasWorld
        key="gui-content"
        presentation={{ anchor: "retained-gui-panel" }}
        create={{
          symbolicId: panelWorldName,
          selectedSystems: PANEL_SYSTEMS,
        }}
        extent={PANEL_EXTENT}
        unitsPerMetre={panelDensity}
        onReady={observeCanvas}
      >
        <Entity id="panel-theme">
          <Theme parts={theme} />
        </Entity>
        <Entity id="canvas">
          <Layout
            kind={3}
            width={PANEL_EXTENT[0]}
            height={PANEL_EXTENT[1]}
            {...TOP_LEFT}
          />
          {/* The panel's text scale, which its controls' default looks follow. */}
          <Font source={surfaceFixture.assets.font.source} font_size={0.36} />
          <Children>
            <Entity id="backing">
              <Layout
                width={PANEL_EXTENT[0]}
                height={PANEL_EXTENT[1]}
                {...TOP_LEFT}
              />
              <Style
                red={PANEL_COLOR[0]}
                green={PANEL_COLOR[1]}
                blue={PANEL_COLOR[2]}
                alpha={PANEL_COLOR[3]}
              />
              <Box width={PANEL_EXTENT[0]} height={PANEL_EXTENT[1]} />
            </Entity>
            <Entity id="content">
              <Layout
                kind={4}
                padding_top={0.6}
                padding_left={0.3}
                {...TOP_LEFT}
              />
              <Children>
                <Entity id="row">
                  <Layout kind={1} {...TOP_LEFT} />
                  <Children>
                    {config.control ? (
                      <Entity key="control" id="control">
                        <Layout width={0.5} height={1.2} {...TOP_LEFT} />
                        <Skin parts={rounded} />
                        <Checkbox />
                      </Entity>
                    ) : null}
                    {variant === "empty" ? (
                      <Entity key="shape-slot" id="shape-slot">
                        <Layout
                          width={shape.width}
                          height={shape.height}
                          {...TOP_LEFT}
                        />
                      </Entity>
                    ) : (
                      <Entity key="shape" id="shape">
                        <Layout
                          width={shape.width}
                          height={shape.height}
                          {...TOP_LEFT}
                        />
                        {/* A decorative themed shape: painted, never an input target. */}
                        <Behavior enabled={false} />
                        <Skin theme="panel-theme" parts={skin} />
                        <Button label="" />
                      </Entity>
                    )}
                    {mixed ? (
                      <Entity key="icon" id="icon">
                        {/* Scale the 32 x 24 unit icon to 0.8 x 0.6 metres. */}
                        <Layout
                          width={0.8}
                          height={0.6}
                          margin_top={0.3}
                          margin_left={0.15}
                          {...TOP_LEFT}
                        />
                        <Style scale_x={0.025} scale_y={0.025} />
                        <Drawing source={surfaceFixture.assets.icon.source} />
                      </Entity>
                    ) : null}
                    {mixed ? (
                      <Entity key="label" id="label">
                        <Layout
                          margin_top={0.3}
                          margin_left={0.15}
                          {...TOP_LEFT}
                        />
                        <Style
                          red={label?.color?.[0] ?? 0.2}
                          green={label?.color?.[1] ?? 1}
                          blue={label?.color?.[2] ?? 0.35}
                          alpha={label?.color?.[3] ?? 1}
                        />
                        <Text
                          text={label?.text ?? "Gui"}
                          source={surfaceFixture.assets.font.source}
                          font_size={0.36}
                        />
                      </Entity>
                    ) : null}
                  </Children>
                </Entity>
              </Children>
            </Entity>
          </Children>
        </Entity>
      </CanvasWorld>
    </>
  ));
  // Settle every pending declaration before its batches are counted.
  await surfaceFixture.presentation.flush();
}

/**
 * Set the panel CanvasWorld's units-per-metre density prop and return the
 * Canvas System state read back from the panel World once the update, which
 * carries no reply, has applied. Authored layout lengths stay logical, so the
 * panel paints scaled about the Surface's top-left corner.
 */
export async function guiPanelDensity(units: number) {
  panelDensity = units;
  await renderPanel();
  const observed = panelCanvas;
  if (!observed) throw new Error("The panel's Canvas World is not attached");
  const session = await observed.session;
  const deadline = performance.now() + 10_000;
  for (;;) {
    const state = (await session.inspectPage({ collection: "canvas" })).canvas
      ?.state;
    if (!state) throw new Error("The panel World has no Canvas System state");
    if (
      state.unitsPerMetre === Math.fround(units) ||
      performance.now() > deadline
    )
      return { unitsPerMetre: state.unitsPerMetre, extent: [...state.extent] };
    await session.waitForFrame();
  }
}

/**
 * Evaluated Canvas-local logical `[x, y, width, height]` of the panel's themed
 * shape control, read from its CanvasBounds in the attached Canvas World.
 */
export async function guiShapeBounds() {
  const observed = panelCanvas;
  if (!observed) throw new Error("The panel's Canvas World is not attached");
  const session = await observed.session;
  const boundsComponent = session.components.CanvasBounds?.id;
  const shape = (await session.inspect()).entities.find(
    (entity) => entity.metadata.symbolicId === "shape",
  );
  if (!shape) throw new Error("The panel has no shape control");
  const bounds = shape.components.find(
    (value) => value.component === boundsComponent,
  )?.fields;
  if (!bounds) return null;
  return ["x", "y", "width", "height"].map((field) => Number(bounds[field]));
}

export async function presentSecondWorld(panel: PanelConfig) {
  await closeInput();
  panelCanvas = undefined;
  panelDensity = 1;
  // The first World keeps its panel and Canvas World resident, so the second
  // World's panel creates its Canvas World under its own name.
  panelWorldName = "retained-gui-panel-content-second";
  await surfaceFixture.replacePresentedWorld();
  return guiPanel(panel);
}

/**
 * Normalized viewport point over the `control` checkbox of an unrotated
 * {@link guiPanel}: the 3.8 x 2.4 m panel spans 16..624 x 48..432 pixels of the
 * 640 x 480 view at 160 px/m, and the checkbox follows the padding over panel
 * metres x 0.3..0.8 and y 0.6..1.8.
 */
const CONTROL_POINT = [
  (16 + 0.55 * 160) / 640,
  (48 + 1.2 * 160) / 480,
] as const;

async function closeInput() {
  const current = input;
  input = undefined;
  await current?.context.close();
}

/**
 * Hover the `control` checkbox of the presented GUI panel through the Host's
 * physical input routing for the presented view, or move the pointer off every
 * panel.
 */
export async function hoverPanel(
  active: boolean,
): Promise<GuiInputRoutingOutcome> {
  const view = surfaceFixture.presentation.view;
  if (!view) throw new Error("No presented view receives GUI input");
  if (input?.view !== view) {
    await closeInput();
    input = { view, context: await surfaceFixture.host.input.open(view) };
  }
  const outcome = await input.context.send({
    kind: "pointerMove",
    pointer: 1n,
    point: active ? CONTROL_POINT : [0.01, 0.01],
  });
  if (active && outcome.disposition !== "routed")
    throw new Error(
      `Pointer over the checkbox was not routed: ${JSON.stringify(outcome)}`,
    );
  return outcome;
}

export async function close() {
  await closeInput();
  panelCanvas = undefined;
  panelConfig = undefined;
  panelDensity = 1;
  panelWorldName = "retained-gui-panel-content";
  canvasBatches.clear();
  await closeSurfaceFixture();
}
