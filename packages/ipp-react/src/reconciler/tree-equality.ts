import type { AnimationDescription } from "../animation/declarations.js";
import { animationSignature } from "../animation/description.js";
import type { AssetDescription } from "../assets/declarations.js";
import { attachmentIdentity } from "../composition/attachment-identity.js";
import type { DataSourceDescription } from "../data/declarations.js";
import type { GuiControlListeners } from "../gui/callbacks.js";
import type { ReactEntityReference } from "./entity-references.js";
import { fieldIdentity, type DeclarationFieldValue } from "./field-values.js";
import type {
  ReactComponentDescription,
  ReactEntityDescription,
  ReactEntityLinkDescription,
  ReactWorldDynamicValue,
  ReactWorldElementProps,
  ReactWorldFieldValue,
} from "./tree.js";

/**
 * The asset and animation declarations as compared between descriptions.
 * Preserves NaN, infinities and -0 so encoder-rejected authored values cannot
 * compare equal to a later corrected declaration.
 */
export function resourceSignature(
  assets: readonly AssetDescription[],
  animations: readonly AnimationDescription[],
  dataSources: readonly DataSourceDescription[],
): string {
  return JSON.stringify([
    dataSources.map((source) => [source.identity, source.signature]),
    assets.map(({ bytes, signature, ...asset }) => asset),
    animations.map(({ mailbox, onPlaybackEvent, ...description }) =>
      animationSignature(description),
    ),
  ]);
}

/** An array copied on its first change. */
export class Copied<T> {
  copied = false;

  constructor(public values: readonly T[]) {}

  set(index: number, value: T): void {
    if (!this.copied) {
      this.values = [...this.values];
      this.copied = true;
    }
    (this.values as T[])[index] = value;
  }
}

/**
 * Whether two descriptions of one component declaration have the same
 * listeners and ref present, which decide what a root observes and checks.
 */
export function sameListenerShape(
  left: ReactComponentDescription,
  right: ReactComponentDescription,
): boolean {
  if (!left.controlRef !== !right.controlRef) return false;
  if (!left.controlListeners || !right.controlListeners)
    return left.controlListeners === right.controlListeners;
  const keys = Object.keys(right.controlListeners);
  return (
    keys.length === Object.keys(left.controlListeners).length &&
    keys.every((key) => Object.hasOwn(left.controlListeners!, key))
  );
}

/** Declaration inputs derived from one component's props. */
export interface ComponentShape {
  readonly component: number;
  readonly fields: ReadonlyMap<number, ReactWorldFieldValue>;
  readonly properties:
    | Readonly<Record<string, ReactWorldDynamicValue>>
    | undefined;
  readonly assetIds: readonly string[];
  readonly entityReferences: boolean;
  resolved?: {
    readonly key: string;
    readonly fields: ReadonlyMap<number, ReactWorldFieldValue>;
  };
}

/**
 * Whether a props update leaves every declared value unchanged. Callbacks and
 * callback refs are listeners rather than declared values; byte arrays always
 * compare by content because callers may reuse and mutate them. Plain arrays
 * and objects, such as vectors and binding lists that renders pass as fresh
 * literals, compare by value.
 */
export function sameDeclarationProps(
  previous: ReactWorldElementProps,
  next: ReactWorldElementProps,
): boolean {
  const keys = Object.keys(next);
  if (keys.length !== Object.keys(previous).length) return false;
  for (const key of keys) {
    if (key === "children") continue;
    if (!Object.hasOwn(previous, key)) return false;
    const before = previous[key];
    const after = next[key];
    if (Object.is(before, after)) {
      if (after instanceof Uint8Array) return false;
      continue;
    }
    if (typeof before === "function" && typeof after === "function") continue;
    if (!samePlainData(before, after, 0)) return false;
  }
  return true;
}

/** Whether two plain arrays or objects hold equal primitive values. */
function samePlainData(left: unknown, right: unknown, depth: number): boolean {
  if (Object.is(left, right)) return !(right instanceof Uint8Array);
  if (
    depth > 4 ||
    typeof left !== "object" ||
    typeof right !== "object" ||
    left === null ||
    right === null
  )
    return false;
  if (Array.isArray(left)) {
    if (!Array.isArray(right) || left.length !== right.length) return false;
    for (let index = 0; index < left.length; index++)
      if (!samePlainData(left[index], right[index], depth + 1)) return false;
    return true;
  }
  if (
    Object.getPrototypeOf(left) !== Object.prototype ||
    Object.getPrototypeOf(right) !== Object.prototype
  )
    return false;
  const keys = Object.keys(right);
  if (keys.length !== Object.keys(left).length) return false;
  for (const key of keys)
    if (
      !Object.hasOwn(left, key) ||
      !samePlainData(
        (left as Record<string, unknown>)[key],
        (right as Record<string, unknown>)[key],
        depth + 1,
      )
    )
      return false;
  return true;
}

export function sameListeners(
  left: GuiControlListeners | undefined,
  right: GuiControlListeners | undefined,
): boolean {
  if (left === right) return true;
  if (!left || !right) return false;
  const keys = Object.keys(right) as (keyof GuiControlListeners)[];
  return (
    keys.length === Object.keys(left).length &&
    keys.every((key) => left[key] === right[key])
  );
}

function sameBytes(left: Uint8Array, right: Uint8Array): boolean {
  if (left.length !== right.length) return false;
  for (let index = 0; index < left.length; index++)
    if (left[index] !== right[index]) return false;
  return true;
}

function sameFieldValue(
  left: ReactWorldFieldValue,
  right: ReactWorldFieldValue,
): boolean {
  if (left.kind !== right.kind) return false;
  if (
    (left.kind === "bytes" || left.kind === "rows") &&
    (right.kind === "bytes" || right.kind === "rows")
  )
    return sameBytes(left.value, right.value);
  if (
    left.kind === "asset" ||
    left.kind === "row-asset" ||
    left.kind === "entity-reference"
  )
    return Object.is(left.value, (right as typeof left).value);
  return fieldIdentity(left) === fieldIdentity(right as DeclarationFieldValue);
}

export function sameShape(
  left: ComponentShape,
  right: ComponentShape,
): boolean {
  if (
    left.component !== right.component ||
    left.fields.size !== right.fields.size
  )
    return false;
  for (const [offset, value] of right.fields) {
    const previous = left.fields.get(offset);
    if (!previous || !sameFieldValue(previous, value)) return false;
  }
  if (!left.properties || !right.properties)
    return left.properties === right.properties;
  return (
    attachmentIdentity(left.properties) === attachmentIdentity(right.properties)
  );
}

/** Symbolic entity fields resolved against this description's entities. */
export function resolvedFields(
  shape: ComponentShape,
  reference: (value: string | bigint) => ReactEntityReference,
): ReadonlyMap<number, ReactWorldFieldValue> {
  const targets = new Map<number, ReactEntityReference>();
  for (const [offset, value] of shape.fields)
    if (value.kind === "entity-reference" && typeof value.value === "string")
      targets.set(offset, reference(value.value));
  const key = [...targets]
    .map(([offset, target]) =>
      typeof target === "bigint"
        ? `${offset}:${target}n`
        : `${offset}:${target.entity}`,
    )
    .join();
  if (shape.resolved?.key === key) return shape.resolved.fields;
  const fields = new Map(shape.fields);
  for (const [offset, target] of targets)
    fields.set(offset, { kind: "entity-reference", value: target });
  shape.resolved = { key, fields };
  return fields;
}

export function sameEach<T>(
  left: readonly T[],
  right: readonly T[],
  same: (left: T, right: T) => boolean,
): boolean {
  if (left.length !== right.length) return false;
  for (let index = 0; index < left.length; index++)
    if (!same(left[index]!, right[index]!)) return false;
  return true;
}

export function sameEntity(
  left: ReactEntityDescription,
  right: ReactEntityDescription,
): boolean {
  return (
    left.identity === right.identity &&
    left.symbolicId === right.symbolicId &&
    left.kind === right.kind &&
    left.parent === right.parent
  );
}

export function sameComponent(
  left: ReactComponentDescription,
  right: ReactComponentDescription,
): boolean {
  return (
    left.identity === right.identity &&
    left.entity === right.entity &&
    left.component === right.component &&
    left.fields === right.fields &&
    left.properties === right.properties
  );
}

function sameReference(
  left: ReactEntityReference | null,
  right: ReactEntityReference | null,
): boolean {
  if (left === null || right === null || typeof left === "bigint")
    return left === right;
  return typeof right === "object" && left.entity === right.entity;
}

export function sameLink(
  left: ReactEntityLinkDescription,
  right: ReactEntityLinkDescription,
): boolean {
  return (
    left.identity === right.identity &&
    left.entity === right.entity &&
    sameReference(left.parent, right.parent) &&
    sameReference(left.before, right.before)
  );
}
