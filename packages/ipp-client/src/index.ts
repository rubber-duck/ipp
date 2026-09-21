/** Target-independent public support consumed by generated clients and integrations. */
export {
  ClientBase,
  RequestNotSentError,
  RequestRejectedError,
  validateOptions,
} from "./client.js";
export type {
  Client,
  AnimationWorldClient,
  AssetWorldClient,
  CameraWorldClient,
  PickingWorldClient,
  RenderWorldClient,
  SurfaceWorldClient,
  GuiWorldClient,
  ConnectOptions,
  LogLevel,
  SpatialWorldClient,
  WorkerConnectOptions,
  ResourceUrlMapping,
} from "./client.js";
export { MAX_CAPTURE_DIMENSION } from "./presentation.js";
export { applyCommandPages, CommandEncodingError } from "./command-pages.js";
export { FieldKind } from "./types.js";
export type * from "./types.js";
export { PortTransport } from "./transport.js";
export type { MessageTransport, TransportEvents } from "./transport.js";
export { workerTransport } from "./worker.js";
export { browserRuntime } from "./browser.js";

export { HostClientBase } from "./host-client.js";
export type * from "./host-protocol.js";
export { WorldPersistenceHostClient } from "./world-persistence-client.js";
export type {
  WorldLoadOptions,
  WorldTransferOptions,
} from "./world-persistence-client.js";

export * from "./dynamic-properties.js";

export { clientAssetSource } from "./asset-sources.js";

export * from "./surface-types.js";
export * from "./gui-types.js";
