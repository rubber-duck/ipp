import type {
  BatchOutcome,
  Client,
  GuiAction,
  GuiObservedEffect,
  GuiTarget,
  GuiWorldClient,
} from "@ipp/client";
import { check } from "../harness/page/checks.js";

/** Submit one semantic action as its own batch of one `guiAction` command. */
export function guiAction(
  client: Pick<Client, "batch">,
  target: Pick<GuiTarget, "entity" | "component" | "incarnation">,
  action: GuiAction,
): Promise<BatchOutcome> {
  return client.batch([
    {
      kind: "guiAction",
      entity: { kind: "handle", id: target.entity },
      component: target.component,
      incarnation: target.incarnation,
      action,
    },
  ]);
}

/** Check that the action's batch applied. */
export function accepted(outcome: BatchOutcome, message = "GUI action"): void {
  check(
    outcome.ok,
    `${message} was refused: ${outcome.ok ? "" : outcome.error.reason}`,
  );
}

/** Check that the action's batch stopped at its operation with `reason`. */
export function refused(outcome: BatchOutcome, reason: string): void {
  check(
    !outcome.ok &&
      outcome.error.scope === "operation" &&
      outcome.error.reason === reason,
    `Expected GUI action refusal ${reason}, got ${
      outcome.ok ? "success" : outcome.error.reason
    }`,
  );
}

/** The momentary effects of one World, as an ordinary subscriber receives them. */
export interface GuiEffectLog {
  readonly effects: readonly GuiObservedEffect[];
  /** The first unclaimed effect matching `predicate`, awaited if needed. */
  next(
    predicate: (effect: GuiObservedEffect) => boolean,
    message: string,
    timeoutMs?: number,
  ): Promise<GuiObservedEffect>;
  close(): Promise<void>;
}

export async function effectLog(client: GuiWorldClient): Promise<GuiEffectLog> {
  const effects: GuiObservedEffect[] = [];
  const claimed = new Set<GuiObservedEffect>();
  let wake: (() => void) | undefined;
  const subscription = await client.subscribeGuiEffects(
    (effect) => {
      effects.push(effect);
      wake?.();
    },
    { classes: "all" },
  );
  return {
    effects,
    async next(predicate, message, timeoutMs = 10_000) {
      const deadline = Date.now() + timeoutMs;
      for (;;) {
        const found = effects.find(
          (effect) => !claimed.has(effect) && predicate(effect),
        );
        if (found) {
          claimed.add(found);
          return found;
        }
        const remaining = deadline - Date.now();
        check(remaining > 0, message);
        await new Promise<void>((resolve) => {
          const timer = setTimeout(resolve, remaining);
          wake = () => {
            clearTimeout(timer);
            resolve();
          };
        });
        wake = undefined;
      }
    },
    async close() {
      await subscription.unsubscribe().catch(() => {});
    },
  };
}
