import type { PublicationReference, WorldReference } from "./types.js";

export interface GuiTarget {
  readonly world: WorldReference;
  readonly entity: bigint;
  readonly component: number;
  readonly incarnation: bigint;
}

/** Logical focus of one World, read through the `guiFocus` inspection collection. */
export interface GuiFocusRecord {
  readonly target: GuiTarget;
  /** Whether focus is indicated, as after keyboard traversal. */
  readonly visible: boolean;
}

/**
 * One live pointer's feedback on one control, read through the `guiPointers`
 * inspection collection; ordered by target entity, then pointer.
 */
export interface GuiPointerRecord {
  readonly target: GuiTarget;
  /** Pointer number, scoped by its GUI input session and context. */
  readonly pointer: bigint;
  /** This pointer's flags on the control, not the control-wide aggregate. */
  readonly state: {
    readonly hovered: boolean;
    readonly pressed: boolean;
    readonly captured: boolean;
  };
}

export type GuiAction =
  | { kind: "scrollTo"; offset: readonly [number, number] }
  | { kind: "scrollBy"; delta: readonly [number, number] }
  | { kind: "scrollToIndex"; index: number; offset?: number }
  | { kind: "submit" }
  | { kind: "press" }
  | { kind: "toggle" }
  | { kind: "scalar"; value: number }
  | { kind: "text"; value: string }
  | { kind: "focus" }
  | { kind: "blur" };

/**
 * A momentary control effect. Committed values are ordinary component fields
 * observed through lifecycle value watches, not effects.
 */
export interface GuiCommittedEffect {
  id: { world: WorldReference; ordinal: bigint } | null;
  target: GuiTarget;
  source: "semantic" | { kind: "routed"; publication: PublicationReference };
  tick: bigint;
  ancestry: readonly bigint[];
  effect:
    | { kind: "pressed" }
    | { kind: "focusChanged"; focused: boolean; changed: boolean }
    | { kind: "submitted"; text: string }
    | {
        kind: "interactionChanged";
        pointer: bigint;
        state: { hovered: boolean; pressed: boolean; captured: boolean };
        changed: boolean;
      };
}

export interface GuiSubscriptionId {
  readonly output: bigint;
  readonly generation: bigint;
}

/** An ordered ACK marker. No clock, ordinal, replay or numeric comparison guarantee. */
export interface GuiSubscriptionCut {
  readonly world: WorldReference;
  readonly subscription: GuiSubscriptionId;
  readonly session: bigint;
  readonly request: bigint;
  readonly kind: "subscribed" | "unsubscribed";
}

export type GuiObservedEffect = Readonly<
  GuiCommittedEffect & {
    id: { readonly world: WorldReference; readonly ordinal: bigint };
  }
>;
export interface GuiObservationOptions {
  classes?: "application" | "feedback" | "all";
}

export type GuiSubscriptionClosure =
  | { kind: "unsubscribed"; cut: GuiSubscriptionCut }
  | { kind: "closed"; reason: Error };

export interface GuiEffectSubscription {
  readonly id: GuiSubscriptionId;
  readonly world: WorldReference;
  readonly start: GuiSubscriptionCut;
  readonly closed: Promise<GuiSubscriptionClosure>;
  unsubscribe(): Promise<GuiSubscriptionCut>;
}

export type GuiObservationRequest =
  | {
      kind: "subscribe";
      world: WorldReference;
      classes: "application" | "feedback" | "all";
    }
  | {
      kind: "unsubscribe";
      world: WorldReference;
      subscription: GuiSubscriptionId;
    };

export type GuiObservationRecord =
  | {
      kind: "control";
      world: WorldReference;
      subscription: GuiSubscriptionId;
      result:
        | "subscribed"
        | "unsubscribed"
        | "cancelled"
        | "staleWorld"
        | "staleSubscription"
        | "alreadySubscribed";
    }
  | {
      kind: "effect";
      subscription: GuiSubscriptionId;
      effect: GuiObservedEffect;
    };
