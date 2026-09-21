import type {
  Client,
  MessageTransport,
  BatchOutcome,
  Command,
  EntityMetadata,
  EntityRef,
  Inspection,
  Response,
} from "@ipp/client";

export interface GeneratedModule {
  readonly IppClient: {
    connectWorker(
      workerUrl: string | URL,
      wasmUrl: string | URL,
      options?: { readonly timeoutMs?: number },
    ): Promise<Client>;
    connectTransport(
      transport: MessageTransport,
      options?: { readonly timeoutMs?: number },
    ): Promise<Client>;
  };
  readonly Entity: {
    create(alias: number, metadata?: Partial<EntityMetadata>): Command;
    alias(alias: number): EntityRef;
    handle(id: bigint): EntityRef;
    delete(entity: EntityRef): Command;
  };
  readonly Scalar: {
    readonly id: number;
    insert(entity: EntityRef, values?: { readonly value?: number }): Command;
    setValue(entity: EntityRef, value: number): Command;
  };
  decodeResponse(bytes: Uint8Array, expectedSession: bigint): Response;
}
export type GeneratedClient = Client;
export type EntitySnapshot = Inspection["entities"][number];

export interface ReactRuntimeConfiguration {
  readonly generatedModuleUrl: string;
  readonly workerScriptUrl: string;
  readonly wasmUrl: string;
  readonly timeoutMs: number;
}

export interface ScalarObservation {
  readonly entityExists: boolean;
  readonly base: number | null;
  readonly effective: number | null;
}

export async function connect(
  configuration: ReactRuntimeConfiguration,
): Promise<{
  readonly contract: GeneratedModule;
  readonly client: GeneratedClient;
}> {
  const contract = await loadContract(configuration.generatedModuleUrl);
  const client = await contract.IppClient.connectWorker(
    configuration.workerScriptUrl,
    configuration.wasmUrl,
    { timeoutMs: configuration.timeoutMs },
  );
  if (!client.capabilities.stateOverlays) {
    await client.close();
    throw new Error("React fixture requires the overlays capability");
  }
  await client.waitForFrame();
  return { contract, client };
}

export async function loadContract(url: string): Promise<GeneratedModule> {
  const contract = (await import(url)) as GeneratedModule;
  if (
    typeof contract.IppClient !== "function" ||
    typeof contract.Scalar !== "object" ||
    contract.Scalar === null
  ) {
    throw new Error("generated module is not an IPP scalar client");
  }
  return contract;
}

export function removeScalar(
  contract: GeneratedModule,
  entity: bigint,
): Command {
  return {
    kind: "removeComponent",
    entity: contract.Entity.handle(entity),
    component: contract.Scalar.id,
  };
}

export async function requireSuccess(
  pending: Promise<BatchOutcome>,
): Promise<Extract<BatchOutcome, { readonly ok: true }>> {
  const outcome = await pending;
  if (!outcome.ok) {
    throw new Error(
      `batch operation ${outcome.error.operation} rejected: ${outcome.error.reason}`,
    );
  }
  return outcome;
}

export function requireAlias(
  outcome: {
    readonly aliases: readonly {
      readonly alias: number;
      readonly id: bigint;
    }[];
  },
  alias: number,
): bigint {
  const resolved = outcome.aliases.find(
    (candidate) => candidate.alias === alias,
  );
  if (resolved === undefined) throw new Error(`missing entity alias ${alias}`);
  return resolved.id;
}

export async function observeScalar(
  client: GeneratedClient,
  scalarId: number,
  symbolicId: string,
): Promise<ScalarObservation> {
  return observeScalarInInspection(
    await client.inspect(),
    scalarId,
    symbolicId,
  );
}

export function observeScalarInInspection(
  inspection: Inspection,
  scalarId: number,
  symbolicId: string,
): ScalarObservation {
  const entity = findEntity(inspection, symbolicId);
  if (entity === undefined) {
    return { entityExists: false, base: null, effective: null };
  }
  return {
    entityExists: true,
    base: scalarValue(entity, scalarId, "base"),
    effective: scalarValue(entity, scalarId, "effective"),
  };
}

export function findEntity(
  inspection: Inspection,
  symbolicId: string,
): EntitySnapshot | undefined {
  return inspection.entities.find(
    (candidate) => candidate.metadata.symbolicId === symbolicId,
  );
}

export function scalarValue(
  entity: EntitySnapshot,
  scalarId: number,
  layer: "base" | "effective",
): number | null {
  const component = entity[layer].find(
    (candidate) => candidate.component === scalarId,
  );
  if (component === undefined) return null;
  const value = component.fields.value;
  if (typeof value !== "number") {
    throw new Error(`Scalar ${layer} value is not numeric`);
  }
  return value;
}

export async function rejectedMessage(pending: Promise<void>): Promise<string> {
  try {
    await pending;
  } catch (error) {
    return error instanceof Error ? error.message : String(error);
  }
  throw new Error("expected React commit to reject");
}

export async function settleRoots(
  roots: readonly { unmount(): Promise<void> }[],
): Promise<void> {
  await Promise.all(roots.map((root) => root.unmount().catch(() => undefined)));
}

export async function isPending(promise: Promise<void>): Promise<boolean> {
  const result = await Promise.race([
    promise.then(
      () => "settled" as const,
      () => "settled" as const,
    ),
    new Promise<"pending">((resolve) => {
      setTimeout(() => resolve("pending"), 0);
    }),
  ]);
  return result === "pending";
}
