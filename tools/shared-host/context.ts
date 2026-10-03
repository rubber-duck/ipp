/** Running a client module against the shared Host: its context and its output files. */
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { basename, extname, join } from "node:path";
import type {
  ClientContext,
  ClientResult,
  SharedHostClient,
} from "./client.js";
import { connect, withPresentationLock, type HostState } from "./host.js";
import { loadModule, shareHostContract } from "./modules.js";
import { encodePng } from "./png.js";
import { image, present, settled } from "./presentation.js";

/** A connected client module and the bundle identity it was loaded from. */
export interface OpenClient {
  readonly context: ClientContext;
  client: SharedHostClient;
  identity: string;
  readonly state: unknown;
}

/** Default name of a module run: its file name without extension. */
export function moduleName(path: string): string {
  return basename(path, extname(path));
}

export async function loadClient(
  workspace: string,
  path: string,
  previous?: string,
): Promise<{ client: SharedHostClient; identity: string } | null> {
  const loaded = await loadModule(workspace, path, previous);
  if (!loaded) return null;
  const client = loaded.module.default as SharedHostClient | undefined;
  if (!client?.open || !client.capture || !client.close)
    throw new Error(
      `${path} has no defineClient({ open, capture, close }) default export`,
    );
  return { client, identity: loaded.identity };
}

/** Connect to the Host, load the module and open it. */
export async function openClient(options: {
  readonly workspace: string;
  readonly directory: string;
  readonly host: HostState;
  readonly module: string;
  readonly name: string;
  readonly args: readonly string[];
}): Promise<OpenClient> {
  const { workspace, directory, host } = options;
  const connection = await connect(host);
  // Modules read the Host's own contract, so share it before loading any.
  shareHostContract(connection.contract);
  let loaded: Awaited<ReturnType<typeof loadClient>>;
  try {
    loaded = (await loadClient(workspace, options.module))!;
  } catch (error) {
    await connection.client.close().catch(() => {});
    throw error;
  }
  const exclusive = <T>(section: () => Promise<T>) =>
    withPresentationLock(
      directory,
      `${options.name} (pid ${process.pid})`,
      section,
    );
  const context: ClientContext = {
    host: connection.client,
    contract: connection.contract,
    workspace,
    assetRoot: join(host.worktree, "target"),
    assetPrefixes: host.ioRead?.map((source) => source.prefix) ?? [],
    name: options.name,
    font: async () => new Uint8Array(await readFile(host.font)),
    load: (path, previous) => loadModule(workspace, path, previous),
    present: (binding, section) =>
      present(connection.client, exclusive, binding, section),
    capture: (binding) =>
      present(connection.client, exclusive, binding, async (view) =>
        image(await settled(connection.client, view, [binding.output])),
      ),
  };
  try {
    const state = await loaded.client.open(context, options.args);
    return { context, client: loaded.client, identity: loaded.identity, state };
  } catch (error) {
    await connection.client.close().catch(() => {});
    throw error;
  }
}

export async function closeClient(open: OpenClient): Promise<void> {
  try {
    await open.client.close(open.state, open.context);
  } finally {
    await open.context.host.close();
  }
}

/** Write a result's images as `<directory>/<key>.png`. */
export async function writeResult(
  result: ClientResult,
  directory: string,
): Promise<string[]> {
  const files: string[] = [];
  const images = Object.entries(result.images ?? {});
  if (images.length) await mkdir(directory, { recursive: true });
  for (const [key, picture] of images) {
    if (!/^[\w.-]+$/.test(key))
      throw new Error(`Image key ${key} is not a plain file name`);
    const path = join(directory, `${key}.png`);
    await writeFile(path, encodePng(picture));
    files.push(path);
  }
  return files;
}
