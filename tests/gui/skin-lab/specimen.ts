/**
 * Skin specimens: ordinary GUI declarations captured in named interaction
 * states. A specimen only declares entities and names its states; the
 * environment that captures it (the skin lab, later a maintained scenario)
 * owns connection, presentation, input ingress and image files, so nothing
 * here depends on Node, a port or a process.
 */
import type { ReactNode } from "react";
import type { GuiAction, GuiPhysicalKey } from "@ipp/client";
import type { AssetReference } from "@ipp/react";
import type { GuiControlRef } from "@ipp/react/gui";

/** A logical canvas point. One unit is one pixel of the unscaled reference sheet. */
export type Point = readonly [number, number];

/** A logical canvas rectangle: left, top, width, height. */
export type Rect = readonly [number, number, number, number];

/** Captures are made at this device scale, matching the 2x reference crops. */
export const CAPTURE_SCALE = 2;

/**
 * One step that pins a state before its capture. Pointer steps use real
 * physical input ingress on the presented canvas with one mouse pointer;
 * `action` applies a semantic `GuiAction` to a control named through
 * {@link SpecimenContext.control}.
 */
export type PinStep =
  /** Move the pointer over a point: hover. */
  | { readonly kind: "hover"; readonly at: Point }
  /** Move to a point and press without releasing: pressed and captured. */
  | { readonly kind: "press"; readonly at: Point }
  /** Press at `from` and move to `to` without releasing: a held drag. */
  | { readonly kind: "drag"; readonly from: Point; readonly to: Point }
  /** Press and release at a point: activates, and focuses a text input. */
  | { readonly kind: "click"; readonly at: Point }
  | { readonly kind: "key"; readonly key: GuiPhysicalKey }
  /**
   * Wait for settled paint before the next step, so that writes a client
   * makes in response to earlier steps, such as opening an overlay on a
   * press, have applied.
   */
  | { readonly kind: "settle" }
  /**
   * Let `seconds` of the specimen World's Host clock pass, such as a
   * tooltip's delay, before the next step or the capture.
   */
  | { readonly kind: "wait"; readonly seconds: number }
  | {
      readonly kind: "action";
      readonly control: string;
      readonly action: GuiAction;
    }
  /**
   * Select UTF-8 byte offsets of the text input focused by an earlier
   * `click`, through its native text state; equal offsets place the caret.
   */
  | {
      readonly kind: "selectText";
      readonly start: number;
      readonly end: number;
    }
  /**
   * Replace the selection of the text input focused by an earlier `click`
   * with `text`, through its native text state, as typing does.
   */
  | { readonly kind: "typeText"; readonly text: string };

/** A named state shown in one cell of the specimen. */
export interface SpecimenState {
  readonly name: string;
  /** The cell whose pixels show this state, including any glow around it. */
  readonly cell: Rect;
  /**
   * Steps applied before this state's own capture. Without steps the cell
   * is taken from the shared capture of the declared, unpinned specimen.
   */
  readonly pin?: readonly PinStep[];
  /**
   * Steps that undo values the pin changed, applied after its capture.
   * Pointer, keyboard and focus feedback is always released.
   */
  readonly restore?: readonly PinStep[];
}

/** Where the specimen sits in a reference crop. */
export interface SpecimenReference {
  /** File name of a reference crop. */
  readonly image: string;
  /** The crop pixel that corresponds to the canvas origin. */
  readonly origin: Point;
  /** Crop pixels per logical unit; the per-row crops are 2x. */
  readonly scale?: number;
}

export interface SpecimenContext {
  /** The shared GUI font, for `Font` and `Text` sources. */
  readonly font: AssetReference;
  /**
   * A `Skin` referencing a named theme of the specimen's theme module, or
   * nothing while that theme has no rows: the runtime default look.
   */
  skin(theme: string): ReactNode;
  /** A ref naming a control for `action` pin steps. */
  control(name: string): GuiControlRef;
}

export interface SkinSpecimen {
  /** Canvas extent in logical units; captured at {@link CAPTURE_SCALE}. */
  readonly extent: readonly [number, number];
  /**
   * Theme module name under `themes/`. It is loaded separately from the
   * specimen, so a live session applies its edits as ordinary row writes
   * to the existing theme entities.
   */
  readonly theme?: string;
  readonly reference?: SpecimenReference;
  readonly states: readonly SpecimenState[];
  /**
   * Ids of declared assets the specimen shows failing, such as a paint that
   * does not compile; their failure does not fail the declaration.
   */
  readonly failingAssets?: readonly string[];
  render(lab: SpecimenContext): ReactNode;
}

export function defineSpecimen(specimen: SkinSpecimen): SkinSpecimen {
  const names = new Set<string>();
  for (const state of specimen.states) {
    if (names.has(state.name))
      throw new Error(`Duplicate specimen state ${state.name}`);
    names.add(state.name);
    const [x, y, width, height] = state.cell;
    if (
      width <= 0 ||
      height <= 0 ||
      x < 0 ||
      y < 0 ||
      x + width > specimen.extent[0] ||
      y + height > specimen.extent[1]
    )
      throw new Error(`State ${state.name} cell lies outside the canvas`);
  }
  return specimen;
}

/** Linear RGBA from an sRGB `#rrggbb` or `#rrggbbaa` colour. */
export function srgb(hex: string): [number, number, number, number] {
  const match = /^#([0-9a-f]{6})([0-9a-f]{2})?$/i.exec(hex);
  if (!match) throw new Error(`Expected #rrggbb or #rrggbbaa, got ${hex}`);
  const channel = (index: number) => {
    const value = Number.parseInt(match[1]!.slice(index, index + 2), 16) / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  };
  const alpha = match[2] ? Number.parseInt(match[2], 16) / 255 : 1;
  return [channel(0), channel(2), channel(4), alpha];
}
