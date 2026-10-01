import type { Ref } from "react";
import type {
  BatchOutcome,
  ComponentFieldValue,
  GuiAction,
  GuiTarget,
} from "@ipp/client";

/**
 * One exact incarnation of a declared control component. A new incarnation
 * publishes a new handle; calls on a handle that is no longer live throw.
 */
export interface GuiControlHandle {
  readonly target: GuiTarget;
  /** The control component's current fields, by field name. */
  read(): Promise<Readonly<Record<string, ComponentFieldValue>>>;
  /**
   * Write `value` to `field` only while it still holds `expected`. Resolves
   * false, without effect, when the field holds another value.
   */
  compareAndSet(
    field: string,
    expected: boolean | number | bigint | string,
    value: boolean | number | bigint | string,
  ): Promise<boolean>;
  /**
   * Apply a semantic action with user-equivalent validation, as one batch.
   * Resolves with the batch outcome; a refused action fails it with
   * `StaleTarget`, `Unavailable`, `UnsupportedAction` or `InvalidValue`.
   */
  action(action: GuiAction): Promise<BatchOutcome>;
}

export type GuiControlRef = Ref<GuiControlHandle>;

export function validateControlRef(
  value: unknown,
): asserts value is GuiControlRef | undefined {
  if (value == null || typeof value === "function") return;
  if (typeof value === "object" && "current" in value) return;
  throw new Error("Control ref must be a ref callback or object");
}
