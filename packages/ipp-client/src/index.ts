/** Target-independent public support consumed by generated clients and integrations. */
export {
  ClientBase,
  RequestNotSentError,
  RequestRejectedError,
  validateOptions,
} from "./client.js";
export type {
  Client,
  ClientClosure,
  AnimationWorldClient,
  AssetWorldClient,
  CameraWorldClient,
  CanvasWorldClient,
  PickingWorldClient,
  RenderWorldClient,
  GuiWorldClient,
  ConnectOptions,
  LogLevel,
  SpatialWorldClient,
  WorkerConnectOptions,
  WorldConnectOptions,
  WorkerWorldConnectOptions,
  ResourceUrlMapping,
} from "./client.js";
export {
  BatchIdentities,
  type CommandBatchWriter,
  type CommandPageLimits,
  planCommandPages,
} from "./command-pages.js";
export { FieldKind } from "./types.js";
export {
  canvasOutput,
  outputProducer,
  sameOutputReference,
} from "./references.js";
export type * from "./types.js";
export { PortTransport } from "./transport.js";
export type {
  BatchIdentitySource,
  MessageTransport,
  TransportEvents,
} from "./transport.js";
export { createWorkerHost, workerTransport } from "./worker.js";
export type { WorkerHost, WorkerEndpoint } from "./worker.js";
export { browserRuntime } from "./browser.js";

export {
  HOST_MESSAGE_BYTES,
  HostContractMismatchError,
  acceptHostAnnouncement,
  contractIdentity,
  hostContractRequest,
  hostHello,
  isHostContractReply,
  readHostAnnouncement,
  readHostContract,
  readHostContractReply,
} from "./host-contract.js";
export type { ContractIdentity, HostAnnouncement } from "./host-contract.js";
export { webSocketTransport } from "./transport.js";
export { HostClientBase } from "./host-client.js";
export { WorldSelectionRequiredError } from "./host-protocol.js";
export { HostPhysicalInput, GuiPhysicalContext } from "./host-input.js";
export type {
  GuiPhysicalInput,
  GuiPickingBlocker,
  GuiPhysicalContextOptions,
  GuiPhysicalKey,
  GuiInputRoutingOutcome,
  GuiInputCancellation,
  GuiTextFence,
  GuiNativeTextState,
  GuiNativeEdit,
} from "./host-input.js";
export {
  HostPresentation,
  PresentationError,
  CaptureTransferError,
} from "./host-presentation.js";
export type {
  RootBinding,
  PresentationSurface,
  PresentationView,
  PresentedFrame,
  PresentedSource,
  PresentedCapture,
  PresentationViewport,
  PresentationFrameOptions,
  PresentationFailure,
} from "./host-presentation.js";
export type * from "./host-protocol.js";
export {
  WorldPersistenceHostClient,
  WorldGraphLoadError,
} from "./world-persistence-client.js";
export type {
  WorldLoadOptions,
  WorldTransferOptions,
  WorldGraphDescriptor,
  WorldGraphLoadResult,
} from "./world-persistence-client.js";

export * from "./dynamic-properties.js";

export { clientAssetSource } from "./asset-sources.js";

export * from "./surface-types.js";
export * from "./gui-types.js";
export {
  LifecycleWatchStartError,
  isLifecycleWatchRemoveError,
} from "./lifecycle-watches.js";
