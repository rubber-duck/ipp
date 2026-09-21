/** Shared browser state and client inspection helpers for Canvas fixture cases. */
import type {
  Client,
  ComponentDescriptor,
  EntitySnapshot,
  FieldValue,
  Inspection,
} from "@ipp/client";
import type { CanvasRuntimeConfiguration } from "@ipp/react/web";

const OPERATION_TIMEOUT_MS = 10_000;

export interface CanvasRuntimeInput {
  readonly assetCacheBytes?: number;
  readonly resourceUrls?: NonNullable<
    CanvasRuntimeConfiguration["resourceUrls"]
  >;
  readonly generatedModuleUrl: string;
  readonly workerScriptUrl: string;
  readonly wasmUrl: string;
  readonly timeoutMs?: number;
}

export interface TransferObservation {
  readonly calls: number;
  readonly duplicateTransfers: number;
  readonly distinctCanvases: number;
  readonly dimensionWritesAfterTransfer: number;
}

const transferredCanvases = new WeakSet<HTMLCanvasElement>();
let transferCalls = 0;
let duplicateTransfers = 0;
let distinctTransferredCanvases = 0;
let dimensionWritesAfterTransfer = 0;
let restoreTransferObserver: (() => void) | undefined;

export function transferObservation(): TransferObservation {
  return {
    calls: transferCalls,
    duplicateTransfers,
    distinctCanvases: distinctTransferredCanvases,
    dimensionWritesAfterTransfer,
  };
}

export function fields(
  descriptor: ComponentDescriptor,
  values: Readonly<Record<string, number | bigint | string>>,
): { readonly offset: number; readonly value: FieldValue }[] {
  return Object.entries(values).map(([name, value]) => {
    const field = descriptor.fields[name];
    if (!field) throw new Error(`${descriptor.id} has no field ${name}`);
    return {
      offset: field.offset,
      value:
        field.kind === 5
          ? { kind: "string", value: String(value) }
          : field.kind === 4
            ? { kind: "u64", value: BigInt(value) }
            : field.kind === 3
              ? { kind: "u32", value: Number(value) }
              : { kind: "f32", value: Number(value) },
    };
  });
}

export async function waitForResource(
  client: Client,
  kind: number,
  source: string,
): Promise<Inspection> {
  const deadline = performance.now() + OPERATION_TIMEOUT_MS;
  while (true) {
    const inspection = await client.inspect();
    const resource = inspection.resources.find(
      (candidate) => candidate.kind === kind && candidate.source === source,
    );
    if (resource?.status === "loaded") return inspection;
    if (resource?.status === "failed") {
      throw new Error(
        `${kind} source ${source} failed: ${resource.error ?? "unknown error"}`,
      );
    }
    if (performance.now() >= deadline) {
      throw new Error(`Timed out waiting for ${kind} source ${source}`);
    }
    await animationBarrier();
  }
}

export function requireComponent(
  client: Client,
  name: string,
): ComponentDescriptor {
  const descriptor = client.components[name];
  if (!descriptor) throw new Error(`runtime does not expose ${name}`);
  return descriptor;
}

export function entity(
  inspection: Inspection,
  symbolicId: string,
): EntitySnapshot | undefined {
  return inspection.entities.find(
    ({ metadata }) => metadata.symbolicId === symbolicId,
  );
}

export function numericField(
  entitySnapshot: EntitySnapshot | undefined,
  layer: "base" | "effective",
  component: number,
  field: string,
): number | null {
  const value = entitySnapshot?.[layer].find(
    (snapshot) => snapshot.component === component,
  )?.fields[field];
  return value === undefined ? null : Number(value);
}

export function installTransferObserver(): void {
  if (restoreTransferObserver) return;
  const prototype = HTMLCanvasElement.prototype;
  const nativeTransfer = prototype.transferControlToOffscreen;
  if (typeof nativeTransfer !== "function") {
    throw new Error("browser does not expose OffscreenCanvas transfer");
  }
  transferCalls = 0;
  duplicateTransfers = 0;
  distinctTransferredCanvases = 0;
  dimensionWritesAfterTransfer = 0;
  const dimensionDescriptors = ["width", "height"].map((property) => {
    const descriptor = Object.getOwnPropertyDescriptor(prototype, property);
    if (!descriptor?.get || !descriptor.set) {
      throw new Error(`browser canvas ${property} property is not observable`);
    }
    Object.defineProperty(prototype, property, {
      ...descriptor,
      get: descriptor.get,
      set(this: HTMLCanvasElement, value: number): void {
        if (transferredCanvases.has(this)) dimensionWritesAfterTransfer += 1;
        descriptor.set?.call(this, value);
      },
    });
    return [property, descriptor] as const;
  });
  Object.defineProperty(prototype, "transferControlToOffscreen", {
    configurable: true,
    writable: true,
    value(this: HTMLCanvasElement): OffscreenCanvas {
      transferCalls += 1;
      if (transferredCanvases.has(this)) duplicateTransfers += 1;
      else {
        transferredCanvases.add(this);
        distinctTransferredCanvases += 1;
      }
      return nativeTransfer.call(this);
    },
  });
  restoreTransferObserver = () => {
    Object.defineProperty(prototype, "transferControlToOffscreen", {
      configurable: true,
      writable: true,
      value: nativeTransfer,
    });
    for (const [property, descriptor] of dimensionDescriptors) {
      Object.defineProperty(prototype, property, descriptor);
    }
  };
}

export async function waitUntil(
  predicate: () => boolean,
  label: string,
  timeoutMs = OPERATION_TIMEOUT_MS,
): Promise<void> {
  const deadline = performance.now() + timeoutMs;
  while (!predicate()) {
    if (performance.now() >= deadline)
      throw new Error(`Timed out waiting for ${label}`);
    await animationBarrier();
  }
}

export async function animationBarrier(): Promise<void> {
  await new Promise<void>((resolvePromise) =>
    requestAnimationFrame(() => resolvePromise()),
  );
}

export function releaseTransferObserver(): void {
  restoreTransferObserver?.();
  restoreTransferObserver = undefined;
}
