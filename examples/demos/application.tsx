import type { ReactNode } from "react";
import { DEMO_RENDER_SCALES } from "./shared/display-settings.js";
import {
  DemoDiagnostics,
  type GalleryDiagnosticsMonitor,
} from "./frame-rate.js";
import { FrameRatePanel, type FrameRateContract } from "./frame-rate-panel.js";
import { ChartSceneInput } from "./shared/scene-input.js";
import { cameraAttachment } from "./shared/camera.js";
import { createLightingInteraction } from "./scenes/lighting/interaction.js";
import type { LightingWorldObjects } from "./scenes/lighting/model.js";
import type { WorldAnimationDemo } from "./scenes/lighting/playback.js";
/** One engine-rendered application shared by every platform adapter. */
import {
  Children,
  Entity,
  FlatSurface,
  AttachedWorld,
  type AttachedWorldHandle,
  type ReactWorldRoot,
} from "@ipp/react";
import { Style, Layout } from "@ipp/react/gui";
import {
  GuiKit,
  Button,
  Checkbox,
  Dropdown,
  Column,
  Toolbar,
  Panel,
  Form,
  Field,
  ScrollArea,
  Popover,
  TextLine,
  type GuiKitContract,
} from "@ipp/react/gui-kit";
import { demoSkins } from "./skin.js";
import { CanvasWorldSession, type IppCanvasHandle } from "@ipp/react/canvas";
import {
  canvasOutput,
  type Client,
  type PresentationFrameOptions,
  type PresentationViewport,
  type WorldReference,
  type WorldGraphLoadResult,
  type WorldLoadOptions,
  type CameraOutputReference,
  type CameraAttachmentReference,
  type PickingWorldClient,
  type GuiAction,
  type OutputReference,
  type CameraViewMotion,
  type RootBinding,
  type GeometryPickResultEvent,
  type GuiPhysicalInput,
  type GuiInputRoutingOutcome,
} from "@ipp/client";
import {
  gallerySceneDefinition,
  GALLERY_SCENE_DEFINITIONS,
} from "./scene-registry.js";
import type {
  GalleryAssets,
  GalleryOptions,
  GallerySceneContext,
  GallerySceneDefinition,
  GallerySceneMount,
  GalleryApplicationControl,
  GalleryApplicationSettings,
} from "./shared/scene.js";
import type { GallerySkin } from "./shared/skin-preference.js";
import {
  DEFAULT_DEMO_FONT,
  DEMO_FONTS,
  DEMO_ICON_FONT,
  demoFont,
  type GalleryFont,
} from "./shared/fonts.js";

export const DEMO_SYSTEMS = [
  "ipp.world-attachment",
  "ipp.lifecycle-publisher",
  "ipp.animation",
  "ipp.asset-dependencies",
  "ipp.hierarchy",
  "ipp.look-at",
  "ipp.final-propagation",
  "ipp.geometry",
  "ipp.gui",
  "ipp.gui-layout",
  "ipp.canvas",
  "ipp.surface",
  "ipp.data-bindings",
  "ipp.plot",
] as const;
const DEMO_FONT_SIZE = 14;
const DEMO_CHROME_MARGIN = 16;

interface OwnedAssets extends GalleryAssets {
  close?(): Promise<void>;
}
export interface DemoApplicationContext extends GallerySceneContext {
  readonly sceneDefinition?: (id: string) => GallerySceneDefinition;
  readonly sceneDefinitions?: readonly GallerySceneDefinition[];
  readonly assetsFor?: (client: Client) => OwnedAssets;
  /** Change physical density while keeping logical GUI coordinates unchanged. */
  readonly renderScaleChanged?: (scale: number) => void | Promise<void>;
}

class SceneCanvas extends CanvasWorldSession {
  private connected!: () => void;
  private rejectConnection!: (error: unknown) => void;
  private readonly attached = new Promise<void>((resolve, reject) => {
    this.connected = resolve;
    this.rejectConnection = reject;
  });
  handle: AttachedWorldHandle | undefined;
  output: OutputReference | undefined;

  constructor(
    client: Client,
    private readonly parent: IppCanvasHandle,
    private readonly signal: AbortSignal,
    report: (error: Error) => void,
  ) {
    super({ client, host: parent.host, onError: report });
    void this.attached.catch(() => {});
    signal.addEventListener("abort", () => this.cancel(signal.reason), {
      once: true,
    });
  }

  cancel(error: unknown) {
    if (!this.handle) this.rejectConnection(error);
  }

  bind(handle: AttachedWorldHandle) {
    this.handle = handle;
    this.connected();
  }
  override get view() {
    return this.parent.view;
  }
  override get viewport() {
    return this.parent.viewport;
  }
  override async frame(options: PresentationFrameOptions = {}) {
    this.signal.throwIfAborted();
    await this.attached;
    this.signal.throwIfAborted();
    await this.flush();
    return this.parent.frame({
      ...options,
      afterOutputs: [
        ...(options.afterOutputs ?? []),
        ...(this.output ? [this.output] : []),
      ],
    });
  }
  override async capture(options: PresentationFrameOptions = {}) {
    this.signal.throwIfAborted();
    await this.attached;
    this.signal.throwIfAborted();
    await this.flush();
    return this.parent.capture({
      ...options,
      afterOutputs: [
        ...(options.afterOutputs ?? []),
        ...(this.output ? [this.output] : []),
      ],
    });
  }
}

interface OwnedScene {
  readonly generation: number;
  readonly definition: GallerySceneDefinition;
  readonly mount: GallerySceneMount;
  readonly canvas: SceneCanvas;
  readonly client: Client;
  readonly worlds: readonly WorldReference[];
  readonly assets: OwnedAssets;
  readonly ownsAssets: boolean;
  readonly prepared: () => void;
  readonly input?: import("./shared/scene.js").GallerySceneInput | undefined;
  readonly lifetime: AbortController;
  unsubscribe?: (() => void) | undefined;
}

/** A compact heading and form share the scroll area's padded content. */
function DemoControlScroll({
  title,
  description,
  children,
}: {
  readonly title: string;
  readonly description: string;
  readonly children: ReactNode;
}) {
  return (
    <ScrollArea id="demo-controls-scroll">
      <Column id="demo-controls-heading" gap="compact">
        <TextLine id="demo-scene-title" text={title} tone="accent" />
        <TextLine
          id="demo-scene-description"
          text={description}
          tone="neutral"
          size="small"
        />
      </Column>
      <Form id="demo-controls-fields">{children}</Form>
    </ScrollArea>
  );
}

/** World creation, candidate preparation, attachment replacement and retirement have one owner. */
export class DemoApplication implements GallerySceneMount {
  readonly output;
  readonly ready: Promise<void>;
  private readonly root: ReactWorldRoot;
  private active: OwnedScene | undefined;
  private preparing: OwnedScene | undefined;
  private generation = 0;
  private tail: Promise<unknown> = Promise.resolve();
  private renderTail: Promise<void> = Promise.resolve();
  private closing = false;
  private disposal: Promise<void> | undefined;
  private readonly retiring = new Set<OwnedScene>();
  private readonly retirements = new Map<
    OwnedScene,
    (() => Promise<unknown> | unknown)[]
  >();
  private rootCleanup: (() => Promise<unknown>)[] | undefined;
  private readonly failedCandidates = new Set<
    (() => Promise<unknown> | unknown)[]
  >();
  private readonly listeners = new Set<() => void>();
  private skin: GallerySkin = "glass";
  private font: GalleryFont = DEFAULT_DEMO_FONT;
  private readonly skins;
  private scale = 1;
  private controlsOpen = false;
  private frameRate: GalleryDiagnosticsMonitor | undefined;
  private readonly diagnostics: DemoDiagnostics;
  private settingsSnapshot: GalleryApplicationSettings | undefined;
  private readonly applicationControl: GalleryApplicationControl = {
    snapshot: () => {
      if (
        this.settingsSnapshot?.skin !== this.skin ||
        this.settingsSnapshot?.font !== this.font ||
        this.settingsSnapshot?.renderScale !== this.scale
      )
        this.settingsSnapshot = {
          skin: this.skin,
          font: this.font,
          renderScale: this.scale,
        };
      return this.settingsSnapshot;
    },
    subscribe: (listener) => this.subscribe(listener),
    update: (patch) => this.update(patch),
  };
  private failure: string | undefined;
  private pending: string | undefined;
  private viewport: PresentationViewport;
  private readonly retained = new Map<string, GalleryOptions>();

  constructor(
    private readonly context: DemoApplicationContext,
    initial: {
      scene?: string;
      options?: GalleryOptions;
      skin?: GallerySkin;
      font?: GalleryFont;
      renderScale?: number;
    } = {},
  ) {
    const world = context.canvas.client.worldReference;
    if (!world)
      throw new Error("The demo application requires an explicit Canvas World");
    this.output = canvasOutput(world);
    this.root = context.canvas.createRoot();
    const contract = context.contract as unknown as GuiKitContract;
    this.skins = demoSkins(contract);
    this.viewport = context.canvas.viewport ?? {
      width: 960,
      height: 640,
      devicePixelRatio: 1,
    };
    this.skin = initial.skin ?? "glass";
    this.font = demoFont(initial.font).key;
    this.scale = initial.renderScale ?? 1;
    this.diagnostics = new DemoDiagnostics(context.canvas, this.report);
    const id = initial.scene ?? "shapes";
    if (initial.options) this.retained.set(id, initial.options);
    this.ready = this.navigate(id);
    void this.ready.catch(() => {});
  }

  get options(): GalleryOptions {
    return {
      ...this.active?.mount.options,
      scene: this.active?.definition.id,
      skin: this.skin,
      font: this.font,
      renderScale: this.scale,
      performance: this.frameRate !== undefined,
    };
  }
  get controller() {
    return this.active?.mount.controller;
  }
  get scene() {
    return this.active?.mount;
  }
  get sceneCanvas() {
    return this.active?.canvas;
  }
  get sceneClient() {
    return this.active?.client;
  }
  get sceneId() {
    return this.active?.definition.id;
  }
  get cameraPath(): readonly CameraAttachmentReference[] {
    const handle = this.active?.canvas.handle;
    return handle ? [cameraAttachment(handle.attachment)] : [];
  }
  get navigationMode(): "orbit" | "charts" | "following" | "fixed" | "yaw" {
    if (this.active?.definition.cameraNavigation === false) return "fixed";
    if (this.active?.definition.cameraNavigation === "yaw") return "yaw";
    return this.active?.definition.id === "charts"
      ? "charts"
      : this.active?.definition.id === "platformer"
        ? "following"
        : "orbit";
  }
  get pickingEnabled() {
    return this.active?.definition.id === "lighting";
  }
  get frameOutputs() {
    return [this.output, ...(this.active ? [this.active.mount.output] : [])];
  }
  get sceneOutput() {
    return this.active?.mount.output;
  }
  /** Wait for lifecycle operations and acknowledged shell declarations. */
  async flush() {
    await this.tail;
    await this.renderTail;
    await this.context.canvas.flush();
  }
  subscribe(listener: () => void) {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  private ordered<T>(work: () => Promise<T>): Promise<T> {
    const pending = this.tail.then(work);
    this.tail = pending.catch(() => {});
    return pending;
  }
  private changed() {
    for (const listener of this.listeners) listener();
  }
  private report = (error: unknown) => {
    if (this.closing || this.context.signal.aborted) return;
    this.failure = error instanceof Error ? error.message : String(error);
    this.context.onError?.(
      error instanceof Error ? error : new Error(String(error)),
    );
    void this.render();
    this.changed();
  };

  private render(): Promise<void> {
    const pending = this.renderTail.then(() =>
      this.root.render(this.declarations()),
    );
    this.renderTail = pending.catch(() => {});
    return pending;
  }

  /** Cockpit navigation occupies its own strip above the scene's camera output. */
  private sceneViewport(id: string): PresentationViewport {
    if (id !== "cockpit") return this.viewport;
    const { tokens } = this.skins[this.skin];
    const scale = DEMO_FONT_SIZE / tokens.em;
    const inset = (tokens.spacing?.inset ?? tokens.inset) * scale;
    const gap = (tokens.spacing?.label ?? tokens.inset / 2) * scale;
    const narrow = this.viewport.width / this.viewport.devicePixelRatio < 400;
    const rowHeight = Math.max(tokens.controlHeight, tokens.row) * scale;
    const toolbarHeight =
      2 * inset + rowHeight * (narrow ? 2 : 1) + (narrow ? gap : 0);
    return {
      ...this.viewport,
      height: Math.max(
        1,
        this.viewport.height -
          (2 * DEMO_CHROME_MARGIN + toolbarHeight) *
            this.viewport.devicePixelRatio,
      ),
    };
  }

  private declarations() {
    const contract = this.context.contract as unknown as GuiKitContract;
    const skin = this.skins[this.skin];
    const active = this.active;
    const Controls = active?.definition.controls;
    const width = this.viewport.width / this.viewport.devicePixelRatio,
      height = this.viewport.height / this.viewport.devicePixelRatio;
    const panelWidth = Math.min(340, width - 32);
    const narrow = width < 400;
    const toolbarWidth = Math.min(width - 32, 450);
    const run = (work: Promise<unknown>) => {
      return work.then(() => {}, this.report);
    };
    const scenePicker = (
      <Dropdown
        id="demo-scene-picker"
        label="Scene"
        options={(
          this.context.sceneDefinitions ?? GALLERY_SCENE_DEFINITIONS
        ).map((scene) => ({
          key: scene.id,
          label: scene.shortLabel,
        }))}
        value={this.active?.definition.id ?? this.pending ?? "shapes"}
        onChange={(id) => run(this.navigate(id))}
        width={-1}
        flex={1}
      />
    );
    const toolbarActions = (
      <>
        <Popover
          id="demo-display"
          label="Display"
          title="DISPLAY"
          contentWidth={248}
          {...(narrow ? { width: 0, flex: 1 } : {})}
        >
          <Form id="demo-display-content">
            <Field id="demo-skin-field" label="Skin">
              <Dropdown
                id="demo-skin-picker"
                label="Skin"
                options={[
                  { key: "glass", label: "Glass" },
                  { key: "cybertron", label: "Cybertron" },
                ]}
                value={this.skin}
                onChange={(value) => run(this.action("skin", value))}
              />
            </Field>
            <Field id="demo-font-field" label="Font">
              <Dropdown
                id="demo-font-picker"
                label="Font"
                options={DEMO_FONTS}
                value={this.font}
                onChange={(value) => run(this.action("font", value))}
              />
            </Field>
            <Field id="demo-scale" label="Render scale">
              <Dropdown
                id="demo-render-scale"
                label="Render scale"
                options={DEMO_RENDER_SCALES.map((value) => ({
                  key: String(value),
                  label: `${value}× pixels`,
                }))}
                value={String(this.scale)}
                onChange={(value) =>
                  run(this.action("renderScale", Number(value)))
                }
              />
            </Field>
            <Toolbar id="demo-reset-row">
              <Button
                id="demo-reset-view"
                label="Reset view"
                variant="secondary"
                onPress={() => run(this.action("resetCamera"))}
                width={0}
                flex={1}
              />
            </Toolbar>
            <Checkbox
              id="demo-performance-toggle"
              checked={this.frameRate !== undefined}
              onCheckedChange={(enabled) =>
                run(this.action("performance", enabled))
              }
            >
              FPS history
            </Checkbox>
          </Form>
        </Popover>
        <Button
          id="demo-controls-toggle"
          label={this.controlsOpen ? "Close" : "Controls"}
          variant="secondary"
          selected={this.controlsOpen}
          onPress={() => {
            this.controlsOpen = !this.controlsOpen;
            void this.render();
            this.changed();
          }}
          {...(narrow ? { width: 0, flex: 1 } : {})}
        />
      </>
    );
    return (
      <>
        {[this.active, this.preparing]
          .filter((scene): scene is OwnedScene => !!scene)
          .map((scene) => {
            const sceneViewport = this.sceneViewport(scene.definition.id);
            const sceneHeight =
              sceneViewport.height / sceneViewport.devicePixelRatio;
            return (
              <Entity
                key={scene.generation}
                id={`demo-scene-${scene.generation}`}
              >
                <Style
                  x={0}
                  y={height - sceneHeight}
                  layer={scene === this.active ? 1 : 0}
                />
                <FlatSurface width={width} height={sceneHeight} />
                <AttachedWorld
                  anchor={`demo-scene-${scene.generation}`}
                  child={{ borrow: scene.worlds[0]! }}
                  attachment={{
                    mode: "surface-camera",
                    output: scene.mount.output as CameraOutputReference,
                  }}
                  onReady={(handle) => scene.canvas.bind(handle)}
                />
              </Entity>
            );
          })}
        <GuiKit
          id="demo-skin"
          contract={contract}
          skin={skin}
          font={this.context.assets.url(demoFont(this.font).path)}
          iconFont={this.context.assets.url(DEMO_ICON_FONT)}
          fontSize={DEMO_FONT_SIZE}
        >
          <Entity id="demo-toolbar">
            <Style x={DEMO_CHROME_MARGIN} y={DEMO_CHROME_MARGIN} layer={100} />
            <Children>
              <Column
                id="demo-chrome"
                width={width - 32}
                height={height - 32}
                fitHeight={false}
                gap={12}
              >
                <Panel
                  id="demo-toolbar-panel"
                  width={toolbarWidth}
                  padding="inset"
                  fitHeight
                >
                  <Column id="demo-toolbar-lines" gap="label">
                    <Toolbar id="demo-toolbar-row">
                      {scenePicker}
                      {!narrow && toolbarActions}
                    </Toolbar>
                    {narrow && (
                      <Toolbar id="demo-toolbar-actions">
                        {toolbarActions}
                      </Toolbar>
                    )}
                  </Column>
                </Panel>
                {this.frameRate && (
                  <FrameRatePanel
                    source={this.frameRate.source}
                    contract={
                      this.context.contract as unknown as FrameRateContract
                    }
                    width={panelWidth}
                  />
                )}
                {this.controlsOpen && this.active && (
                  <Panel
                    id="demo-controls-panel"
                    width={panelWidth}
                    maxHeight={560}
                  >
                    <DemoControlScroll
                      title={this.active.definition.label}
                      description={this.active.definition.description}
                    >
                      {Controls && (
                        <Controls
                          mount={this.active.mount}
                          report={(error) => {
                            if (
                              this.active === active &&
                              !this.closing &&
                              !active?.lifetime.signal.aborted
                            )
                              this.report(error);
                          }}
                        />
                      )}
                    </DemoControlScroll>
                  </Panel>
                )}
              </Column>
            </Children>
          </Entity>
          {(this.failure || this.pending) && (
            <Entity id="demo-status">
              <Style x={16} y={height - 40} layer={200} />
              <Layout kind={0} width={Math.max(100, width - 32)} height={30} />
              <Children>
                <TextLine
                  id="demo-status-text"
                  text={this.failure ?? `Preparing ${this.pending}…`}
                  tone={this.failure ? "error" : "accent"}
                />
              </Children>
            </Entity>
          )}
        </GuiKit>
      </>
    );
  }

  private async prepare(
    definition: GallerySceneDefinition,
  ): Promise<OwnedScene> {
    const lifetime = new AbortController();
    const signal = AbortSignal.any([this.context.signal, lifetime.signal]);
    const timer = setTimeout(
      () =>
        lifetime.abort(
          new Error("Demo scene preparation timed out after 60 seconds"),
        ),
      60_000,
    );
    const worlds: WorldReference[] = [];
    let client: Client | undefined,
      canvas: SceneCanvas | undefined,
      mount: GallerySceneMount | undefined,
      assets: OwnedAssets | undefined;
    try {
      const source = definition.world(this.context.assets);
      const parent = this.context.canvas.client.worldReference!;
      const symbolicId = `demo/${parent.incarnation}/${parent.id}/${definition.id}/${++this.generation}`;
      if (source.create)
        worlds.push(
          (
            await this.context.canvas.host.createWorld({
              ...source.create,
              temporary: true,
              symbolicId,
            })
          ).reference,
        );
      else {
        const host = this.context.canvas
          .host as typeof this.context.canvas.host & {
          loadWorld(
            bytes: Uint8Array,
            options: WorldLoadOptions,
          ): Promise<WorldGraphLoadResult>;
        };
        const graph = await host.loadWorld(
          await this.context.assets.readBytes(source.load.url, signal),
          {
            ...source.load.options,
            symbolicId,
            signal,
            worldNames: (graph) =>
              new Map(
                graph.nodes.map((node) => [
                  node.id,
                  node.id === graph.root
                    ? symbolicId
                    : `${symbolicId}/${node.id}`,
                ]),
              ),
          },
        );
        worlds.push(
          graph.root,
          ...[...graph.created.values()].filter(
            (world) => world.id !== graph.root.id,
          ),
        );
      }
      client = await this.context.canvas.host.openWorld(worlds[0]!);
      const generation = this.generation;
      const report = (error: Error) => {
        if (
          !signal.aborted &&
          (this.active?.generation === generation ||
            this.preparing?.generation === generation ||
            (this.generation === generation && this.pending === definition.id))
        )
          this.report(error);
      };
      canvas = new SceneCanvas(client, this.context.canvas, signal, report);
      assets = this.context.assetsFor?.(client) ?? this.context.assets;
      await assets.prepare(definition.resources ?? [], signal);
      await definition.initialize?.(client, signal, canvas.host, assets);
      mount = await definition.mount(
        {
          canvas,
          assets,
          contract: this.context.contract,
          signal,
          onError: report,
          diagnostics: this.diagnostics,
          application: this.applicationControl,
          presentationClient: this.context.canvas.client as PickingWorldClient,
          cameraPath: () =>
            canvas!.handle ? [cameraAttachment(canvas!.handle.attachment)] : [],
          cameraView: () => {
            const binding = this.context.canvas.view?.binding;
            if (!binding || !canvas!.handle)
              throw new Error("The scene camera is not presented");
            return {
              kind: "camera",
              root: { kind: "bound", binding },
              path: [cameraAttachment(canvas!.handle.attachment)],
            };
          },
        },
        {
          ...definition.defaultOptions,
          ...this.retained.get(definition.id),
          skin: this.skin,
          font: this.font,
        },
      );
      canvas.output = mount.output;
      return {
        generation: this.generation,
        definition,
        mount,
        canvas,
        client,
        worlds,
        assets,
        ownsAssets: !!this.context.assetsFor,
        prepared: () => clearTimeout(timer),
        input:
          mount.sceneInput ??
          (definition.id === "charts"
            ? new ChartSceneInput(mount, signal, () => this.viewport)
            : undefined),
        lifetime,
      };
    } catch (error) {
      clearTimeout(timer);
      lifetime.abort(error);
      canvas?.cancel(error);
      const loadError = error as {
        name?: string;
        pendingCleanup?: Map<unknown, WorldReference>;
      };
      if (
        loadError?.name === "WorldGraphLoadError" &&
        loadError.pendingCleanup instanceof Map
      )
        worlds.push(...loadError.pendingCleanup.values());
      const cleanup = [
        () => mount?.dispose(),
        () => canvas?.close(),
        () => (this.context.assetsFor ? assets?.close?.() : undefined),
        () => client?.close(),
        ...worlds.map(
          (world) => () => this.context.canvas.host.destroyWorld(world),
        ),
      ];
      this.failedCandidates.add(cleanup);
      try {
        await this.drainCleanup(cleanup);
        this.failedCandidates.delete(cleanup);
      } catch (failure) {
        throw new AggregateError(
          [error, failure],
          "Demo candidate cleanup remains pending",
        );
      }
      throw error;
    }
  }

  navigate(id: string): Promise<void> {
    return this.ordered(async () => {
      if (this.closing) throw new Error("The demo application is closed");
      const definition = (
        this.context.sceneDefinition ?? gallerySceneDefinition
      )(id);
      if (this.active?.definition === definition) {
        await this.active.mount.action("resetCamera");
        return;
      }
      this.pending = definition.id;
      this.failure = undefined;
      const previous = this.active;
      let candidate: OwnedScene | undefined;
      let published = false;
      try {
        await this.render();
        this.changed();
        candidate = await this.prepare(definition);
        this.preparing = candidate;
        await this.render();
        await candidate.mount.ready;
        candidate.lifetime.signal.throwIfAborted();
        candidate.prepared();
        await candidate.mount.resize?.(
          this.sceneViewport(candidate.definition.id),
        );
        await this.context.canvas.frame({
          afterOutputs: [this.output, candidate.mount.output],
        });
        this.active = candidate;
        this.preparing = undefined;
        candidate.unsubscribe = candidate.mount.subscribe?.(() => {
          if (
            !this.closing &&
            this.active === candidate &&
            !candidate!.lifetime.signal.aborted
          ) {
            void this.render();
            this.changed();
          }
        });
        this.pending = undefined;
        await this.render();
        await this.context.canvas.frame({
          afterOutputs: [this.output, candidate.mount.output],
        });
        published = true;
        if (previous) {
          this.retiring.add(previous);
          this.retained.set(previous.definition.id, previous.mount.options);
          await this.retire(previous);
        }
        this.changed();
      } catch (error) {
        const failures: unknown[] = [error];
        this.preparing = undefined;
        this.pending = undefined;
        if (!published) {
          this.active = previous;
          candidate?.unsubscribe?.();
          if (candidate) this.retiring.add(candidate);
          try {
            await this.render();
            if (previous)
              await this.context.canvas.frame({
                afterOutputs: [this.output, previous.mount.output],
              });
          } catch (failure) {
            failures.push(failure);
          }
          if (candidate && failures.length === 1) {
            try {
              await this.retire(candidate);
            } catch (failure) {
              failures.push(failure);
            }
          }
        }
        const failure =
          failures.length === 1
            ? error
            : new AggregateError(
                failures,
                "Demo replacement recovery remains pending",
              );
        this.report(failure);
        throw failure;
      }
    });
  }

  private async drainCleanup(steps: (() => Promise<unknown> | unknown)[]) {
    while (steps.length) {
      await steps[0]!();
      steps.shift();
    }
  }

  private async retire(scene: OwnedScene) {
    this.retiring.add(scene);
    scene.unsubscribe?.();
    scene.input?.cancel();
    scene.prepared();
    scene.lifetime.abort(new Error("Demo scene retired"));
    scene.canvas.cancel(scene.lifetime.signal.reason);
    let cleanup = this.retirements.get(scene);
    if (!cleanup) {
      cleanup = [
        () => scene.mount.dispose(),
        () => scene.canvas.close(),
        () => (scene.ownsAssets ? scene.assets.close?.() : undefined),
        () => scene.client.close(),
        ...scene.worlds.map(
          (world) => () => this.context.canvas.host.destroyWorld(world),
        ),
      ];
      this.retirements.set(scene, cleanup);
    }
    await this.drainCleanup(cleanup);
    this.retirements.delete(scene);
    this.retiring.delete(scene);
  }

  async retryCleanup() {
    const failures: unknown[] = [];
    if (this.rootCleanup) {
      try {
        await this.drainCleanup(this.rootCleanup);
        this.rootCleanup = undefined;
      } catch (error) {
        failures.push(error);
      }
    }
    for (const scene of [...this.retiring]) {
      try {
        await this.retire(scene);
      } catch (error) {
        failures.push(error);
      }
    }
    for (const steps of [...this.failedCandidates]) {
      try {
        await this.drainCleanup(steps);
        this.failedCandidates.delete(steps);
      } catch (error) {
        failures.push(error);
      }
    }
    if (failures.length)
      throw new AggregateError(failures, "Demo cleanup remains pending");
  }

  async resize(viewport: PresentationViewport) {
    this.viewport = viewport;
    if (this.active)
      await this.active.mount.resize?.(
        this.sceneViewport(this.active.definition.id),
      );
    await this.render();
  }
  async update(patch: GalleryOptions): Promise<void> {
    const { scene, skin, font, renderScale, performance, ...fields } = patch;
    if (scene !== undefined) await this.navigate(String(scene));
    if (skin !== undefined) await this.action("skin", skin);
    if (font !== undefined) await this.action("font", font);
    if (renderScale !== undefined)
      await this.action("renderScale", renderScale);
    if (performance !== undefined)
      await this.action("performance", performance);
    if (Object.keys(fields).length)
      await this.sceneOperation((scene) => scene.mount.update(fields));
  }

  private async sceneOperation<T>(
    operation: (scene: OwnedScene) => Promise<T>,
  ) {
    // Admit against one lifetime in lifecycle order, without holding navigation
    // or capture behind a scene-owned asynchronous resource preparation.
    const admitted = await this.ordered(async () => {
      const scene = this.active;
      if (this.closing || !scene || scene.lifetime.signal.aborted)
        throw new Error("No active demo scene");
      return { scene, completion: operation(scene) };
    });
    const result = await admitted.completion;
    if (
      !this.closing &&
      this.active === admitted.scene &&
      !admitted.scene.lifetime.signal.aborted
    ) {
      await this.render();
      this.changed();
    }
    return result;
  }

  action(name: string, args?: unknown): Promise<unknown> {
    if (name === "performance")
      return this.ordered(async () => {
        if (args === true && !this.frameRate) {
          const history = await this.diagnostics.acquire();
          this.frameRate = history;
          await this.render();
        } else if (args === false && this.frameRate) {
          const history = this.frameRate;
          this.frameRate = undefined;
          await this.render();
          await history.close();
        }
        this.changed();
        return this.options;
      });
    if (name === "gui")
      return this.actuateGui(
        args as { id: string; component: string; action: GuiAction },
      );
    if (name === "retryCleanup") return this.ordered(() => this.retryCleanup());
    if (name === "scene") return this.navigate(String(args));
    if (name !== "skin" && name !== "font" && name !== "renderScale")
      return this.sceneOperation((scene) => scene.mount.action(name, args));
    return this.ordered(async () => {
      if (name === "skin") {
        if (args !== "glass" && args !== "cybertron")
          throw new Error("Unknown demo skin");
        this.skin = args;
        await this.active?.mount.update({ skin: args });
        if (this.active?.definition.id === "cockpit")
          await this.active.mount.resize?.(this.sceneViewport("cockpit"));
      } else if (name === "font") {
        const font = demoFont(args).key;
        await this.active?.mount.update({ font });
        this.font = font;
      } else if (name === "renderScale") {
        const value = Number(args);
        if (!DEMO_RENDER_SCALES.some((scale) => scale === value))
          throw new RangeError("Unsupported render scale");
        await this.context.renderScaleChanged?.(value);
        this.scale = value;
      }
      await this.render();
      this.changed();
      return this.options;
    });
  }
  get capturingSceneInput() {
    return this.active?.input?.capturing ?? false;
  }
  async handlePhysicalInput(
    input: GuiPhysicalInput,
    outcome: GuiInputRoutingOutcome,
  ) {
    await this.active?.input?.handle(input, outcome);
  }

  async navigateCamera(motion: CameraViewMotion, binding: RootBinding) {
    const scene = this.active;
    if (!scene || scene.lifetime.signal.aborted)
      throw new Error("No active scene camera");
    if (scene.definition.cameraNavigation === false) return;
    if (scene.definition.cameraNavigation === "yaw") {
      if (
        (motion.kind !== "rotate" && motion.kind !== "turn") ||
        motion.yaw === 0
      )
        return;
      motion = { kind: "turn", yaw: motion.yaw, pitch: 0 };
    }
    if (scene.definition.actions.includes("navigate")) {
      await scene.mount.action("navigate", motion);
      return;
    }
    await (this.context.canvas.client as PickingWorldClient).navigateCamera({
      binding,
      path: this.cameraPath,
      motion,
    });
  }

  async picked(result: GeometryPickResultEvent, signal: AbortSignal) {
    const scene = this.active;
    if (!scene || !this.pickingEnabled) return;
    const lifetime = AbortSignal.any([signal, scene.lifetime.signal]);
    const interaction = createLightingInteraction(
      scene.canvas,
      {
        objects: scene.mount.options.objects as LightingWorldObjects,
        select: (id) => {
          if (!lifetime.aborted)
            void scene.mount.action("select", id).catch((error) => {
              if (!lifetime.aborted) this.report(error);
            });
        },
        update: (id, patch) => {
          if (!lifetime.aborted)
            void scene.mount
              .action("updateObject", { id, patch })
              .catch((error) => {
                if (!lifetime.aborted) this.report(error);
              });
        },
      },
      (scene.mount.controller as WorldAnimationDemo | undefined)?.session,
      () => {},
    );
    return interaction(result, lifetime);
  }

  async actuateGui({
    id,
    component,
    action,
  }: {
    id: string;
    component: string;
    action: GuiAction;
  }) {
    const client = this.context.canvas.client;
    const inspection = await client.inspect();
    const entity = inspection.entities.find(
      (entity) => entity.metadata.symbolicId === id,
    );
    const type = client.components[component];
    const target = entity?.components.find(
      (value) => value.component === type?.id,
    );
    if (!entity || !type || !target)
      throw new Error(`No live ${component} control at ${id}`);
    const watch = await client.watchLifecycle(
      [
        {
          target: { kind: "component", entity: entity.id, component: type.id },
          kinds: 8,
        },
      ],
      () => {},
    );
    try {
      const lifetime = watch.baselines[0]?.lifetime;
      if (lifetime?.kind !== "component" || lifetime.incarnation === null)
        throw new Error("The GUI target retired before its action");
      const result = await client.batch([
        {
          kind: "guiAction",
          entity: { kind: "handle", id: entity.id },
          component: type.id,
          incarnation: lifetime.incarnation,
          action,
        },
      ]);
      if (!result.ok) throw new Error(result.error.reason);
      await this.context.canvas.flush();
      return result;
    } finally {
      await watch.remove();
    }
  }

  async inspect() {
    return this.active?.mount.inspect() ?? this.context.canvas.client.inspect();
  }
  async inspectApplication() {
    return {
      options: this.options,
      sceneWorlds: this.active?.worlds ?? [],
      cameraPath: this.cameraPath,
      preparing: this.pending,
      pendingCleanup: [...this.retiring].map((scene) => ({
        scene: scene.definition.id,
        worlds: scene.worlds,
      })),
      failedCandidates: this.failedCandidates.size,
      pendingRootCleanup: !!this.rootCleanup?.length,
      error: this.failure,
      frameRate: this.frameRate
        ? await this.context.canvas.host.datasets.read(this.frameRate.source)
        : undefined,
      shell: await this.context.canvas.client.inspect(),
    };
  }
  dispose(): Promise<void> {
    if (this.disposal) return this.disposal;
    const first = !this.closing;
    this.closing = true;
    this.diagnostics.pause();
    this.disposal = this.ordered(async () => {
      if (!first) {
        await this.retryCleanup();
        return;
      }
      const scenes = [
        ...new Set(
          [this.active, this.preparing, ...this.retiring].filter(
            (scene): scene is OwnedScene => !!scene,
          ),
        ),
      ];
      this.active = undefined;
      this.preparing = undefined;
      const failures: unknown[] = [];
      this.rootCleanup = [
        () => this.root.render(null),
        () => this.root.flush(),
        () => this.root.unmount(),
        async () => {
          await this.diagnostics.close();
          this.frameRate = undefined;
        },
      ];
      try {
        await this.drainCleanup(this.rootCleanup);
        this.rootCleanup = undefined;
      } catch (error) {
        failures.push(error);
      }
      for (const scene of scenes) {
        try {
          await this.retire(scene);
        } catch (error) {
          failures.push(error);
        }
      }
      try {
        await this.retryCleanup();
      } catch (error) {
        failures.push(error);
      }
      this.listeners.clear();
      if (failures.length)
        throw new AggregateError(
          failures,
          "Demo application cleanup remains pending",
        );
    });
    // A failed close retains its cleanup records and allows a later retry.
    void this.disposal.catch(() => {
      this.disposal = undefined;
    });
    return this.disposal;
  }
}
