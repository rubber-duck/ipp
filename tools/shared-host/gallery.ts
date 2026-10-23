import {
  DemoApplication,
  DEMO_SYSTEMS,
} from "../../examples/demos/application.js";
import {
  DEMO_FONT_RESOURCES,
  demoFont,
} from "../../examples/demos/shared/fonts.js";
/** Shared gallery scenes mounted through the persistent development Host. */
import { readFile } from "node:fs/promises";
import { resolve, sep } from "node:path";
import { parseArgs } from "node:util";
import type {
  AssetWorldClient,
  Client,
  ClientAssetSource,
  PresentationFrameOptions,
  PresentationViewport,
  RootBinding,
  GuiPhysicalInput,
  WorldReference,
} from "@ipp/client";
import { CanvasWorldSession } from "@ipp/react/canvas";
import type {
  GalleryAssets,
  GalleryOptions,
  GalleryResource,
  GallerySceneDefinition,
} from "../../examples/demos/shared/scene.js";
import {
  defineClient,
  type ClientContext,
  type LoadedModule,
} from "./client.js";
import { image } from "./presentation.js";

export const GALLERY_ASSET_PREFIX = "ipp-gallery://assets/";
export const GALLERY_REGISTRY = "examples/demos/scene-registry.ts";

/** Local reads and immutable, session-owned resources for a scene mount. */
class NativeGalleryAssets implements GalleryAssets {
  private readonly resources = new Map<string, ClientAssetSource>();

  constructor(
    private readonly root: string,
    private readonly client?: AssetWorldClient,
  ) {}

  private path(path: string): string {
    const relative = path.startsWith(GALLERY_ASSET_PREFIX)
      ? path.slice(GALLERY_ASSET_PREFIX.length)
      : path.replace(/^\/?target\//, "");
    const targetPath = relative.replace(
      /^gallery-platformer-assets\//,
      "gallery-platformer-native-assets/",
    );
    const file = resolve(
      this.root,
      targetPath.replace(
        /^demos-cockpit-assets\//,
        "demos-cockpit-native-assets/",
      ),
    );
    if (file !== this.root && !file.startsWith(this.root + sep))
      throw new Error(`Gallery asset escapes its target directory: ${path}`);
    return file;
  }

  async prepare(resources: readonly GalleryResource[], signal?: AbortSignal) {
    if (!this.client)
      throw new Error("Resource preparation requires an open World");
    for (const resource of resources) {
      signal?.throwIfAborted();
      if (this.resources.has(resource.path)) continue;
      const bytes = await this.readBytes(resource.path, signal);
      const source = await this.client.createAsset(resource.kind, bytes.buffer);
      this.resources.set(resource.path, source);
    }
  }

  url(path: string): string {
    if (/^[a-z][\w+.-]*:/i.test(path)) return path;
    const source = this.resources.get(path);
    if (source) return source.source;
    // The Host owner configures this read-only target directory with --gallery.
    this.path(path);
    return GALLERY_ASSET_PREFIX + path.replace(/^\/?target\//, "");
  }

  async readBytes(
    path: string,
    signal?: AbortSignal,
  ): Promise<Uint8Array<ArrayBuffer>> {
    signal?.throwIfAborted();
    const bytes = await readFile(this.path(path), signal ? { signal } : {});
    signal?.throwIfAborted();
    return new Uint8Array(bytes);
  }

  async readJson<T>(path: string, signal?: AbortSignal): Promise<T> {
    return JSON.parse(
      new TextDecoder().decode(await this.readBytes(path, signal)),
    ) as T;
  }

  async close() {
    const failures: unknown[] = [];
    for (const [path, source] of this.resources) {
      try {
        await this.client!.releaseAsset(source);
        this.resources.delete(path);
      } catch (error) {
        failures.push(error);
      }
    }
    if (failures.length)
      throw new AggregateError(failures, "Gallery assets remain owned");
  }
}

/** Scene controllers observe actual frames through the Host-wide presentation lock. */
class NativeCanvasSession extends CanvasWorldSession {
  private binding: RootBinding | undefined;
  private presented: import("@ipp/client").PresentationView | null = null;
  private selected: import("@ipp/client").PresentationView | null = null;
  private resolveBinding!: (binding: RootBinding) => void;
  private rejectBinding!: (error: Error) => void;
  private readonly pendingBinding: Promise<RootBinding>;

  constructor(
    private readonly context: ClientContext,
    client: Client,
  ) {
    super({ host: context.host, client });
    this.pendingBinding = new Promise((resolve, reject) => {
      this.resolveBinding = resolve;
      this.rejectBinding = reject;
    });
    void this.pendingBinding.catch(() => {});
  }

  bind(binding: RootBinding) {
    this.binding = binding;
    this.resolveBinding(binding);
  }

  override get view() {
    return this.presented;
  }

  override get viewport(): PresentationViewport | null {
    return this.binding?.viewport ?? null;
  }

  /** Keep observers on the exact selected view while the shared capture lock is held. */
  async withPresentation<T>(
    operation: (view: import("@ipp/client").PresentationView) => Promise<T>,
  ): Promise<T> {
    if (this.selected) return operation(this.selected);
    const binding = this.binding ?? (await this.pendingBinding);
    return this.context.present(binding, async (view) => {
      this.presented = view;
      this.selected = view;
      try {
        return await operation(view);
      } finally {
        this.selected = null;
      }
    });
  }

  override async frame(options: PresentationFrameOptions = {}) {
    await this.flush();
    return this.withPresentation((view) =>
      this.host.presentation.frame(view, options),
    );
  }

  override async capture(options: PresentationFrameOptions = {}) {
    await this.flush();
    return this.withPresentation((view) =>
      this.host.presentation.capture(view, options),
    );
  }

  async input(
    inputs: readonly GuiPhysicalInput[],
    application: DemoApplication,
  ) {
    return this.withPresentation(async (view) => {
      await this.host.presentation.frame(view, {
        afterOutputs: application.frameOutputs,
      });
      const input = await this.host.input.open(view);
      const outcomes = [];
      try {
        for (const command of inputs) {
          const outcome = await input.send(command);
          outcomes.push(outcome);
          await application.handlePhysicalInput(command, outcome);
          await this.flush();
          await this.host.presentation.frame(view, {
            afterOutputs: application.frameOutputs,
          });
        }
      } finally {
        await input.close();
      }
      return outcomes;
    });
  }

  override async close() {
    this.rejectBinding(new Error("Gallery Canvas closed before presentation"));
    await super.close();
    if (this.binding) {
      await this.host.clearRootOutput(this.binding);
      this.binding = undefined;
    }
  }
}

interface MountedScene {
  readonly definition: GallerySceneDefinition;
  readonly scene: DemoApplication;
  readonly canvas: NativeCanvasSession;
  readonly client: Client;
  readonly worlds: readonly WorldReference[];
  readonly assets: NativeGalleryAssets;
  readonly lifetime: AbortController;
}

function definition(loaded: LoadedModule, id: string): GallerySceneDefinition {
  const registry = loaded.module.gallerySceneDefinition;
  const candidate =
    typeof registry === "function"
      ? registry(id)
      : (loaded.module.default ??
        Object.values(loaded.module).find(
          (value) =>
            value &&
            typeof value === "object" &&
            (value as { id?: unknown }).id === id,
        ));
  const scene = candidate as GallerySceneDefinition | undefined;
  if (
    !scene ||
    scene.id !== id ||
    typeof scene.mount !== "function" ||
    typeof scene.world !== "function"
  )
    throw new Error(`Module does not export a gallery scene ${id}`);
  return scene;
}

async function cleanup(mounted: MountedScene) {
  mounted.lifetime.abort(new Error("Gallery scene disposed"));
  await mounted.scene.dispose();
  const failures: unknown[] = [];
  for (const close of [
    () => mounted.canvas.close(),
    () => mounted.assets.close(),
    () => mounted.client.close(),
    ...mounted.worlds.map(
      (world) => () => mounted.canvas.host.destroyWorld(world),
    ),
  ]) {
    try {
      await close();
    } catch (error) {
      failures.push(error);
    }
  }
  if (failures.length)
    throw new AggregateError(failures, "Gallery scene cleanup failed");
}

export class NativeGallerySession {
  private mounted: MountedScene | undefined;
  private loaded: LoadedModule | undefined;
  private identity = "";
  private generation = 0;
  private tail: Promise<unknown> = Promise.resolve();

  constructor(
    private readonly context: ClientContext,
    readonly id: string,
    private readonly module: string,
    private readonly assetRoot: string,
    private viewport: PresentationViewport,
    private options: GalleryOptions,
  ) {}

  private ordered<T>(operation: () => Promise<T>): Promise<T> {
    const pending = this.tail.then(operation);
    this.tail = pending.catch(() => {});
    return pending;
  }

  async open() {
    if (
      (this.id === "platformer" || this.id === "cockpit") &&
      !this.context.assetPrefixes?.includes(`https://${this.id}.ipp.invalid/`)
    )
      throw new Error(
        `${this.id} requires its saved asset namespace. Ask the Host owner to restart with: node tools/shared-host/shared-host.mjs host start --gallery`,
      );
    const loaded = (await this.context.load(this.module))!;
    this.loaded = loaded;
    this.mounted = await this.mount(definition(loaded, this.id));
    this.identity = loaded.identity;
  }

  private async mount(scene: GallerySceneDefinition): Promise<MountedScene> {
    const lifetime = new AbortController();
    const worlds: WorldReference[] = [];
    let client: Client | undefined,
      canvas: NativeCanvasSession | undefined,
      assets: NativeGalleryAssets | undefined,
      application: DemoApplication | undefined;
    try {
      const name = `demos/${this.context.name}/${++this.generation}`;
      const root = await this.context.host.createWorld({
        selectedSystems: DEMO_SYSTEMS,
        temporary: true,
        symbolicId: name,
        canvas: {
          extent: [
            this.viewport.width / this.viewport.devicePixelRatio,
            this.viewport.height / this.viewport.devicePixelRatio,
          ],
          unitsPerMetre: 1,
        },
      });
      worlds.push(root.reference);
      client = await this.context.host.openWorld(root.reference);
      canvas = new NativeCanvasSession(this.context, client);
      assets = new NativeGalleryAssets(
        this.assetRoot,
        client as AssetWorldClient,
      );
      await assets.prepare(DEMO_FONT_RESOURCES, lifetime.signal);
      const output = (await import("@ipp/client")).canvasOutput(root.reference);
      canvas.bind(await this.context.host.setRootOutput(output, this.viewport));
      const Application = (this.loaded?.module.DemoApplication ??
        DemoApplication) as typeof DemoApplication;
      application = new Application(
        {
          canvas,
          assets,
          contract: this.context.contract,
          signal: lifetime.signal,
          assetsFor: (child) =>
            new NativeGalleryAssets(this.assetRoot, child as AssetWorldClient),
          sceneDefinition: (id) =>
            this.loaded ? definition(this.loaded, id) : scene,
          sceneDefinitions: (this.loaded?.module.GALLERY_SCENE_DEFINITIONS as
            | readonly GallerySceneDefinition[]
            | undefined) ?? [scene],
          renderScaleChanged: async (scale) => {
            canvas!.bind(
              await this.context.host.setRootOutput(output, {
                ...this.viewport,
                width: Math.round(this.viewport.width * scale),
                height: Math.round(this.viewport.height * scale),
                devicePixelRatio: this.viewport.devicePixelRatio * scale,
              }),
            );
            await application!.resize(canvas!.viewport!);
          },
        },
        {
          scene: scene.id,
          options: this.options,
          font: demoFont(this.options.font).key,
          ...(typeof this.options.renderScale === "number"
            ? { renderScale: this.options.renderScale }
            : {}),
          ...(this.options.skin === "glass" || this.options.skin === "cybertron"
            ? { skin: this.options.skin }
            : {}),
        },
      );
      await application.resize(this.viewport);
      await application.ready;
      if (
        typeof this.options.renderScale === "number" &&
        this.options.renderScale !== 1
      )
        await application.action("renderScale", this.options.renderScale);
      await canvas.flush();
      this.options = application.options;
      return {
        definition: scene,
        scene: application,
        canvas,
        client,
        worlds,
        assets,
        lifetime,
      };
    } catch (error) {
      lifetime.abort(error);
      const failures: unknown[] = [error];
      for (const close of [
        () => application?.dispose(),
        () => canvas?.close(),
        () => assets?.close(),
        () => client?.close(),
        ...worlds.map((world) => () => this.context.host.destroyWorld(world)),
      ]) {
        try {
          await close();
        } catch (failure) {
          failures.push(failure);
        }
      }
      throw failures.length === 1
        ? error
        : new AggregateError(failures, "Demo application mounting failed");
    }
  }

  present<T>(
    operation: (view: import("@ipp/client").PresentationView) => Promise<T>,
  ) {
    return this.requireMounted().canvas.withPresentation(operation);
  }

  /** Resize the physical root and authored layout together, retaining the scene. */
  resize(viewport: PresentationViewport) {
    return this.ordered(async () => {
      const mounted = this.requireMounted();
      const binding = await this.context.host.setRootOutput(
        mounted.scene.output,
        viewport,
      );
      mounted.canvas.bind(binding);
      this.viewport = viewport;
      await mounted.scene.resize(viewport);
      await mounted.canvas.flush();
    });
  }

  capture() {
    return this.ordered(async () => {
      const mounted = this.requireMounted();
      await mounted.scene.ready;
      const captured = await mounted.canvas.capture({
        afterOutputs: mounted.scene.frameOutputs,
      });
      return {
        images: { [this.id]: image(captured) },
        report: {
          scene: mounted.scene.sceneId,
          generation: this.generation,
          sequence: captured.sequence,
          viewport: this.viewport,
        },
      };
    });
  }

  command(command: string, args: readonly string[]) {
    return this.ordered(async () => {
      if (command === "reload") {
        // Bundle and validate before releasing the current scene or any handles.
        const next = await this.context.load(
          this.module,
          args.includes("--if-changed") ? this.identity : undefined,
        );
        if (!next)
          return { report: { reloaded: false, generation: this.generation } };
        const previous = this.mounted;
        const selected = definition(next, previous?.scene.sceneId ?? this.id);
        if (previous) {
          this.options = previous.scene.options;
          await cleanup(previous);
        }
        this.mounted = undefined;
        this.loaded = next;
        this.mounted = await this.mount(selected);
        this.identity = next.identity;
        return {
          report: {
            reloaded: true,
            generation: this.generation,
            options: this.options,
          },
        };
      }
      const mounted = this.requireMounted();
      if (command === "options") {
        const patch: unknown = JSON.parse(args.join(" "));
        if (!patch || typeof patch !== "object" || Array.isArray(patch))
          throw new Error("Options must be a JSON object");
        await mounted.scene.update(patch as GalleryOptions);
        this.options = mounted.scene.options;
        return { report: { scene: this.id, options: this.options } };
      }
      if (command === "action") {
        const [name, ...payload] = args;
        if (!name) throw new Error("Action needs its scene action name");
        if (name === "physicalInput") {
          const inputs = JSON.parse(payload.join(" ")) as GuiPhysicalInput[];
          for (const input of inputs)
            if ("pointer" in input) input.pointer = BigInt(input.pointer);
          return {
            report: {
              outcomes: await mounted.canvas.input(inputs, mounted.scene),
            },
          };
        }
        return {
          report: await mounted.scene.action(
            name,
            payload.length ? JSON.parse(payload.join(" ")) : undefined,
          ),
        };
      }
      if (command === "inspect")
        return {
          report: {
            scene: this.id,
            generation: this.generation,
            options: mounted.scene.options,
            actions: mounted.definition.actions,
            state: await mounted.scene.inspect(),
            application: await mounted.scene.inspectApplication(),
          },
        };
      throw new Error(`Unknown gallery command ${command}`);
    });
  }

  get application(): DemoApplication {
    return this.requireMounted().scene;
  }

  private requireMounted(): MountedScene {
    if (!this.mounted)
      throw new Error("No mounted gallery scene; reload to recover");
    return this.mounted;
  }

  close() {
    return this.ordered(async () => {
      if (this.mounted) {
        await cleanup(this.mounted);
        this.mounted = undefined;
      }
    });
  }
}

export default defineClient<NativeGallerySession>({
  reloadOnCapture: false,
  async open(context, args) {
    const { values, positionals } = parseArgs({
      args: [...args],
      allowPositionals: true,
      options: {
        width: { type: "string" },
        height: { type: "string" },
        options: { type: "string" },
        module: { type: "string" },
        "asset-root": { type: "string" },
      },
    });
    const width = Number(values.width ?? 960),
      height = Number(values.height ?? 640);
    if (
      ![width, height].every(
        (value) => Number.isInteger(value) && value > 0 && value <= 2048,
      )
    )
      throw new Error(
        "Gallery width and height must be integers from 1 to 2048",
      );
    const options = JSON.parse(values.options ?? "{}") as GalleryOptions;
    if (!options || typeof options !== "object" || Array.isArray(options))
      throw new Error("Options must be a JSON object");
    const session = new NativeGallerySession(
      context,
      positionals[0] ?? "shapes",
      values.module ?? GALLERY_REGISTRY,
      resolve(values["asset-root"] ?? context.assetRoot),
      { width, height, devicePixelRatio: 1 },
      options,
    );
    await session.open();
    return session;
  },
  capture: (state) => state.capture(),
  command: (state, _context, command, args) => state.command(command, args),
  close: (state) => state.close(),
});
