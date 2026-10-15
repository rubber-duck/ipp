import type { LifecycleTargetWatch } from "@ipp/client";
import type { ReactFieldWatchLease } from "./field-watches.js";
/** Services belong to the receiving declaration container, never a global runtime. */
import { useContext } from "react";
import type {
  ComponentDescriptor,
  RowsInput,
  RowsLayoutDescriptor,
  WorldReference,
} from "@ipp/client";
import { AttachmentContext } from "../composition/attached-world.js";
import type { GuiKitContract } from "../gui-kit/kit.js";
import type { ReactCommitStatus } from "../reconciler/commits.js";
import type { ReactWorldContainer } from "../reconciler/host-config.js";
import type { ReactWorldClient } from "../reconciler/world-client.js";
import type { ReactAssetState } from "../assets/registry.js";
import type { DataWindow, ReactDataSourceState } from "../data/declarations.js";

export interface ReactWorldContract extends GuiKitContract {
  readonly SCHEMA_HASH: bigint;
  readonly components: GuiKitContract["components"] &
    Readonly<Record<string, ComponentDescriptor>>;
  encodeRowsTable<Row extends object>(
    layout: RowsLayoutDescriptor,
    table: RowsInput<Row>,
  ): Uint8Array<ArrayBuffer>;
  encodeDataWindows?: (
    windows: readonly DataWindow[],
  ) => Uint8Array<ArrayBuffer>;
}

export interface ReactAssetCodec<T> {
  readonly kind: number;
  readonly encode: (data: T) => Uint8Array<ArrayBuffer>;
}

/** Explicitly target-matched application encoders; generic inputs are erased here. */
export interface ReactWorldCodecs {
  readonly schemaHash: bigint;
  readonly mesh?: ReactAssetCodec<never>;
}

export interface ReactWorldServiceOptions {
  readonly contract?: ReactWorldContract;
  readonly codecs?: ReactWorldCodecs;
}

export interface ReactWorldScope<
  Contract extends ReactWorldContract = ReactWorldContract,
> {
  readonly client: ReactWorldClient;
  readonly world: WorldReference | undefined;
  readonly namespace: string;
  readonly contract: Contract | undefined;
  readonly codecs: ReactWorldCodecs | undefined;
  getCommitStatus(): ReactCommitStatus;
  subscribeCommits(listener: () => void): () => void;
  resolveEntity(reference: string | bigint): bigint;
  getAsset(id: string): ReactAssetState | undefined;
  subscribeAssets(listener: () => void): () => void;
  getDataSource(name: string): ReactDataSourceState | undefined;
  subscribeDataSources(listener: () => void): () => void;
}

const scopeContainers = new WeakMap<ReactWorldScope, ReactWorldContainer>();

/** Internal hook resources participate in their receiving root cleanup. */
export function trackFieldWatch(
  scope: ReactWorldScope,
  admission: Promise<LifecycleTargetWatch>,
): ReactFieldWatchLease {
  const container = scopeContainers.get(scope);
  if (!container)
    throw new Error("Field watch requires a registered receiving World");
  return container.commits.fieldWatches.track(admission);
}

const scopes = new WeakMap<ReactWorldContainer, ReactWorldScope>();
const namespaces = new WeakMap<ReactWorldContainer, string>();

export function worldNamespace(container: ReactWorldContainer): string {
  let value = namespaces.get(container);
  if (!value) {
    value = globalThis.crypto.randomUUID();
    namespaces.set(container, value);
  }
  return value;
}

export function validateWorldServices(
  client: ReactWorldClient,
  options: ReactWorldServiceOptions,
): void {
  if (options.contract && options.contract.SCHEMA_HASH !== client.schemaHash)
    throw new Error(
      "React World contract does not match the receiving session schema",
    );
  if (options.codecs && options.codecs.schemaHash !== client.schemaHash)
    throw new Error(
      "React World codecs do not match the receiving session schema",
    );
}

/** Root and attached child setup validate services against the actual session. */
export function registerWorldScope(
  container: ReactWorldContainer,
  client: ReactWorldClient,
  options: ReactWorldServiceOptions = {},
): ReactWorldScope {
  validateWorldServices(client, options);
  const scope: ReactWorldScope = Object.freeze({
    client,
    world: client.worldReference,
    namespace: worldNamespace(container),
    contract: options.contract,
    codecs: options.codecs,
    getCommitStatus: () => container.commits.getStatus(),
    subscribeCommits: (listener: () => void) =>
      container.commits.subscribeStatus(listener),
    resolveEntity: (reference: string | bigint) =>
      container.commits.resolveEntity(reference),
    getAsset: (id: string) => container.commits.assets.get(id),
    subscribeAssets: (listener: () => void) =>
      container.commits.assets.subscribeSnapshot(listener),
    getDataSource: (name: string) => container.commits.dataSources.get(name),
    subscribeDataSources: (listener: () => void) =>
      container.commits.dataSources.subscribeSnapshot(listener),
  });
  scopes.set(container, scope);
  scopeContainers.set(scope, container);
  return scope;
}

/** Optional for declarations whose explicit props need no ambient services. */
export function useOptionalWorld<
  Contract extends ReactWorldContract = ReactWorldContract,
>(): ReactWorldScope<Contract> | undefined {
  const container = useContext(AttachmentContext);
  return (container ? scopes.get(container) : undefined) as
    | ReactWorldScope<Contract>
    | undefined;
}

export function useWorld<
  Contract extends ReactWorldContract = ReactWorldContract,
>(): ReactWorldScope<Contract> {
  const scope = useOptionalWorld<Contract>();
  if (!scope)
    throw new Error(
      "useWorld requires a receiving React World; create declarations with createRoot or AttachedWorld",
    );
  return scope;
}
