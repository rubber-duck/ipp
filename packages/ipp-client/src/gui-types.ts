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
  /**
   * The focused part of the control, such as a range slider's upper thumb 1;
   * 0 for a control with one part.
   */
  readonly part: number;
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

/**
 * One group's active item, read through the `guiActiveItems` inspection
 * collection; ordered and paged by group entity.
 */
export interface GuiActiveItemRecord {
  /** The entity holding the `GuiGroup`. */
  readonly group: bigint;
  /** The active item's control lifetime. */
  readonly target: GuiTarget;
}

/**
 * The World's GUI presentation preferences, read through the `guiPreferences`
 * inspection collection and changed with `GuiPreferencesUpdateCommand`.
 */
export interface GuiPreferencesRecord {
  /** Every skin transition is immediate. */
  readonly reducedMotion: boolean;
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
  /**
   * Set a colour control's hue, saturation, value and alpha, each in 0..1;
   * a colour outside that range is refused.
   */
  | {
      kind: "color";
      value: readonly [number, number, number, number];
    }
  /**
   * Focus a part of the control: a range slider's lower thumb 0 or upper
   * thumb 1, a colour control's field 0, hue rail 1 or alpha rail 2;
   * omitted, part 0, the whole of a control with one part.
   */
  | { kind: "focus"; part?: number }
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
    /**
     * Focus on the control after the change, and its focused part, or the
     * part it left: 0 for a control with one part.
     */
    | {
        kind: "focusChanged";
        focused: boolean;
        changed: boolean;
        part: number;
      }
    | { kind: "submitted"; text: string }
    /**
     * Text a numeric text input refused to commit because it does not parse;
     * its number is unchanged.
     */
    | { kind: "rejected"; text: string }
    /**
     * A numeric text input's pending edit ended without being committed or
     * refused, such as by Escape, with the discarded text; its number is
     * unchanged.
     */
    | { kind: "discarded"; text: string }
    /**
     * A secondary press, the Menu key or Shift+F10 on the control, at a
     * logical point of its canvas: the press point, or the bottom-left corner
     * of its visible box for a key. The client decides what opens.
     */
    | { kind: "contextRequested"; point: readonly [number, number] }
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
