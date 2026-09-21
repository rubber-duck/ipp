export interface EntityId {
  readonly bits: bigint;
  readonly index: number;
  readonly generation: number;
}

export type EntityReference =
  | { readonly kind: "entity"; readonly entity: EntityId }
  | { readonly kind: "alias"; readonly alias: string };

export type SceneOperation =
  | {
      readonly kind: "create";
      readonly alias: string;
      readonly symbolicId?: string;
      readonly classes?: readonly string[];
    }
  | { readonly kind: "delete"; readonly entity: EntityReference }
  | {
      readonly kind: "updateMetadata";
      readonly entity: EntityReference;
      readonly symbolicId?: string;
      readonly classes?: readonly string[];
    }
  | {
      readonly kind: "insertScalar";
      readonly entity: EntityReference;
      readonly value: number;
    }
  | {
      readonly kind: "setScalar";
      readonly entity: EntityReference;
      readonly value: number;
    }
  | {
      readonly kind: "insertLinearDriver";
      readonly entity: EntityReference;
      readonly source: EntityReference;
      readonly scale: number;
      readonly bias: number;
    };

export interface BatchSuccess {
  readonly status: "committed";
  readonly batchId: bigint;
  readonly commitTick: bigint;
  readonly aliases: Readonly<Record<string, EntityId>>;
}

export interface BatchFailure {
  readonly aliases: Readonly<Record<string, EntityId>>;
  readonly status: "rejected";
  readonly batchId: bigint;
  readonly operationIndex: number | null;
  readonly scope: "operation" | "commit";
  readonly code: string;
  readonly reason: string;
}

export type BatchOutcome = BatchSuccess | BatchFailure;

export interface FrameObservation {
  readonly tick: bigint;
  readonly time: number;
}

export interface ConcurrentRpcCorrelation {
  readonly batches: readonly {
    readonly requestedBatchId: bigint;
    readonly returnedBatchId: bigint;
    readonly commitTick: bigint;
    readonly aliasCount: number;
  }[];
  readonly inspections: readonly {
    readonly tick: bigint;
    readonly time: number;
    readonly sawMarker: boolean;
  }[];
}

export interface EntityObservation {
  readonly entity: EntityId;
  readonly symbolicId: string | null;
  readonly classes: readonly string[];
  readonly scalar: {
    readonly base: number;
    readonly effective: number;
  } | null;
  readonly linearDriver: {
    readonly source: EntityId;
    readonly scale: number;
    readonly bias: number;
  } | null;
}

export interface HarnessDriver {
  submit(
    batchId: bigint,
    operations: readonly SceneOperation[],
    options: { readonly signal: AbortSignal },
  ): Promise<BatchOutcome>;
  waitForFrame(
    afterTick: bigint | undefined,
    options: { readonly signal: AbortSignal },
  ): Promise<FrameObservation>;
  publicStepAvailable(options: {
    readonly signal: AbortSignal;
  }): Promise<boolean>;
  correlateConcurrentRequests(
    marker: EntityId,
    batchIds: readonly bigint[],
    options: { readonly signal: AbortSignal },
  ): Promise<ConcurrentRpcCorrelation>;
  inspect(
    entity: EntityId,
    options: { readonly signal: AbortSignal },
  ): Promise<EntityObservation | null>;
  findBySymbolicId(
    symbolicId: string,
    options: { readonly signal: AbortSignal },
  ): Promise<EntityObservation | null>;
  close(): Promise<void>;
}

export interface ProtocolRejection {
  readonly rejected: boolean;
  readonly stage: "handshake" | "request";
  readonly code: string;
  readonly detail: string;
}

export type MalformedCase =
  | "no-bootstrap"
  | "oversized-message"
  | "trailing-bytes"
  | "unknown-tag"
  | "removed-step-tag";

export interface DriverConnectOptions {
  readonly signal: AbortSignal;
  readonly record: (kind: string, value: unknown) => Promise<void>;
}

export interface HarnessDriverFactory {
  connect(url: string, options: DriverConnectOptions): Promise<HarnessDriver>;
  rejectMismatchedSchema(
    url: string,
    options: DriverConnectOptions,
  ): Promise<ProtocolRejection>;
  rejectStaleSession(
    url: string,
    options: DriverConnectOptions,
  ): Promise<ProtocolRejection>;
  rejectMalformed(
    url: string,
    malformedCase: MalformedCase,
    options: DriverConnectOptions,
  ): Promise<ProtocolRejection>;
}

export function entity(entity: EntityId): EntityReference {
  return { kind: "entity", entity };
}

export function alias(value: string): EntityReference {
  return { kind: "alias", alias: value };
}
