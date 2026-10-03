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
  WorldGraphLoadResult,
  WorldGraphLoadError,
  WorldLoadOptions,
  WorldReference,
} from "@ipp/client";
import { CanvasWorldSession } from "@ipp/react/canvas";
import type {
  GalleryAssets,
  GalleryOptions,
  GalleryResource,
  GallerySceneDefinition,
  GallerySceneMount,
} from "../../examples/world-gallery/shared/scene.js";
import {
  defineClient,
  type ClientContext,
  type LoadedModule,
} from "./client.js";
import { image } from "./presentation.js";

export const GALLERY_ASSET_PREFIX = "ipp-gallery://assets/";
export const GALLERY_REGISTRY = "examples/world-gallery/scene-registry.ts";

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
    const file = resolve(this.root, targetPath);
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

  override get viewport(): PresentationViewport | null {
    return this.binding?.viewport ?? null;
  }

  override async frame(options: PresentationFrameOptions = {}) {
    await this.flush();
    const binding = await this.pendingBinding;
    return this.context.present(binding, (view) =>
      this.host.presentation.frame(view, options),
    );
  }

  override async capture(options: PresentationFrameOptions = {}) {
    await this.flush();
    const binding = await this.pendingBinding;
    return this.context.present(binding, (view) =>
      this.host.presentation.capture(view, options),
    );
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
  readonly scene: GallerySceneMount;
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
  const failures: unknown[] = [];
  for (const close of [
    () => mounted.scene.dispose(),
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
  private identity = "";
  private generation = 0;
  private tail: Promise<unknown> = Promise.resolve();

  constructor(
    private readonly context: ClientContext,
    readonly id: string,
    private readonly module: string,
    private readonly assetRoot: string,
    private readonly viewport: PresentationViewport,
    private options: GalleryOptions,
  ) {}

  private ordered<T>(operation: () => Promise<T>): Promise<T> {
    const pending = this.tail.then(operation);
    this.tail = pending.catch(() => {});
    return pending;
  }

  async open() {
    if (
      this.id === "platformer" &&
      !this.context.assetPrefixes?.includes("https://platformer.ipp.invalid/")
    )
      throw new Error(
        "Platformer requires its saved asset namespace. Ask the Host owner to restart with: node tools/shared-host/shared-host.mjs host start --gallery",
      );
    const loaded = (await this.context.load(this.module))!;
    this.mounted = await this.mount(definition(loaded, this.id));
    this.identity = loaded.identity;
  }

  private async mount(scene: GallerySceneDefinition): Promise<MountedScene> {
    const lifetime = new AbortController();
    const timer = setTimeout(
      () =>
        lifetime.abort(
          new Error("Gallery readiness timed out after 60 seconds"),
        ),
      60_000,
    );
    const worldSource = scene.world(new NativeGalleryAssets(this.assetRoot));
    const worlds: WorldReference[] = [];
    let client: Client | undefined;
    let canvas: NativeCanvasSession | undefined;
    let assets: NativeGalleryAssets | undefined;
    let mounted: GallerySceneMount | undefined;
    try {
      const name = `gallery/${this.context.name}/${++this.generation}`;
      if (worldSource.create) {
        const world = await this.context.host.createWorld({
          ...worldSource.create,
          temporary: true,
          symbolicId: name,
        });
        worlds.push(world.reference);
      } else {
        const host = this.context.host as typeof this.context.host & {
          loadWorld(
            bytes: Uint8Array,
            options: WorldLoadOptions,
          ): Promise<WorldGraphLoadResult>;
        };
        if (!host.loadWorld)
          throw new Error("This Host has no saved World support");
        const bytes = await new NativeGalleryAssets(this.assetRoot).readBytes(
          worldSource.load.url,
          lifetime.signal,
        );
        const graph = await host.loadWorld(bytes, {
          ...worldSource.load.options,
          symbolicId: name,
          signal: lifetime.signal,
          worldNames: (graph) =>
            new Map(
              graph.nodes.map((node) => [
                node.id,
                node.id === graph.root ? name : `${name}/${node.id}`,
              ]),
            ),
        });
        worlds.push(
          graph.root,
          ...[...graph.created.values()].filter(
            (world) => world.id !== graph.root.id,
          ),
        );
      }
      lifetime.signal.throwIfAborted();
      client = await this.context.host.openWorld(worlds[0]!);
      canvas = new NativeCanvasSession(this.context, client);
      assets = new NativeGalleryAssets(
        this.assetRoot,
        client as AssetWorldClient,
      );
      await assets.prepare(scene.resources ?? [], lifetime.signal);
      await scene.initialize?.(
        client,
        lifetime.signal,
        this.context.host,
        assets,
      );
      mounted = await scene.mount(
        {
          canvas,
          assets,
          contract: this.context.contract,
          signal: lifetime.signal,
        },
        { ...scene.defaultOptions, ...this.options },
      );
      canvas.bind(
        await this.context.host.setRootOutput(mounted.output, this.viewport),
      );
      await mounted.resize?.(this.viewport);
      lifetime.signal.throwIfAborted();
      await Promise.race([
        mounted.ready,
        new Promise<never>((_, reject) => {
          lifetime.signal.addEventListener(
            "abort",
            () => reject(lifetime.signal.reason),
            { once: true },
          );
        }),
      ]);
      lifetime.signal.throwIfAborted();
      await canvas.flush();
      this.options = mounted.options;
      return {
        definition: scene,
        scene: mounted,
        canvas,
        client,
        worlds,
        assets,
        lifetime,
      };
    } catch (error) {
      lifetime.abort(error);
      // Generated Host clients load their own support classes; compare the public
      // error identity across that module boundary and retain exact cleanup handles.
      const loadError = error as Partial<WorldGraphLoadError> | undefined;
      if (
        loadError?.name === "WorldGraphLoadError" &&
        loadError.pendingCleanup instanceof Map
      )
        worlds.push(...loadError.pendingCleanup.values());
      const failures: unknown[] = [error];
      for (const close of [
        () => mounted?.dispose(),
        () => canvas?.close(),
        () => assets?.close(),
        () => client?.close(),
        ...worlds.map((world) => () => this.context.host.destroyWorld(world)),
      ]) {
        try {
          await close();
        } catch (cleanup) {
          failures.push(cleanup);
        }
      }
      throw failures.length === 1
        ? error
        : new AggregateError(failures, "Gallery mounting failed");
    } finally {
      clearTimeout(timer);
    }
  }

  capture() {
    return this.ordered(async () => {
      const mounted = this.requireMounted();
      await mounted.scene.ready;
      const captured = await mounted.canvas.capture({
        afterOutputs: [mounted.scene.output],
      });
      return {
        images: { [this.id]: image(captured) },
        report: {
          scene: this.id,
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
        const selected = definition(next, this.id);
        const previous = this.mounted;
        if (previous) {
          this.options = previous.scene.options;
          await cleanup(previous);
        }
        this.mounted = undefined;
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
          },
        };
      throw new Error(`Unknown gallery command ${command}`);
    });
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
