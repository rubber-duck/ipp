/**
 * ADVANCED: an expander holding the dashboard's presentation settings. ACCENT
 * re-themes the primary actions and the projector in place; EXPLODE LAYERS
 * separates the canvas layers along the Surface normal; REDUCED MOTION is the
 * panel World's GUI preference, which also snaps the explosion, toasts and
 * progress indicators. The expander stands on the page under NODE STATUS;
 * collapsing it removes its rows and leaves the space empty.
 */
import { Entity } from "@ipp/react";
import type { ReactNode } from "react";
import { Behavior, Checkbox, Skin } from "@ipp/react/gui";
import {
  Expander,
  Row as KitRow,
  SegmentedControl,
  TextLine,
} from "@ipp/react/gui-kit";
import { SWITCH_THEME } from "./monitor.js";
import { BoxLayout, LEAF, PanelBody, TOKENS } from "./presentation.js";
import type { Accent, GuiScene } from "./scene.js";
import { useStoreValue } from "./store.js";

const ROW_GAP = 4;

/** The expander and its three rows. */
export const ADVANCED_HEIGHT =
  TOKENS.controlHeight +
  TOKENS.inset / 2 +
  3 * TOKENS.controlHeight +
  2 * ROW_GAP;

/** Symbolic IDs of the settings controls. */
export const ADVANCED_CONTROLS = {
  header: "gui-advanced-header",
  accent: "gui-accent",
  explode: "gui-explode",
  motion: "gui-reduced-motion",
} as const;

export function Advanced({ scene }: { readonly scene: GuiScene }) {
  const open = useStoreValue(scene.state, (state) => state.advancedOpen);
  const accent = useStoreValue(scene.state, (state) => state.accent);
  return (
    <PanelBody id="gui-advanced">
      <Expander
        id={ADVANCED_CONTROLS.header}
        label="ADVANCED"
        summary="3 OPTIONS"
        expanded={open}
        onExpandedChange={scene.setAdvancedOpen}
      >
        <Setting id="gui-accent-row" label="ACCENT" first>
          <SegmentedControl
            id={ADVANCED_CONTROLS.accent}
            options={[
              { value: "cyan", label: "CYAN" },
              { value: "amber", label: "AMBER" },
            ]}
            value={accent}
            onChange={(value) => scene.setAccent(value as Accent)}
            layout={{ width: 160 }}
          />
        </Setting>
        <Setting id="gui-explode-row" label="EXPLODE LAYERS">
          <Entity id={ADVANCED_CONTROLS.explode}>
            <BoxLayout
              kind={LEAF}
              width={72}
              height={TOKENS.smallHeight}
              alignY={0}
            />
            <Skin theme={SWITCH_THEME} />
            <Behavior semantic_label="EXPLODE LAYERS" />
            <Checkbox
              label=""
              checked={false}
              ref={scene.explodeControl}
              onToggle={(event) => scene.setExploded(event.value)}
            />
          </Entity>
        </Setting>
        <Setting id="gui-motion-row" label="REDUCED MOTION">
          <Entity id={ADVANCED_CONTROLS.motion}>
            <BoxLayout
              kind={LEAF}
              width={TOKENS.smallHeight}
              height={TOKENS.smallHeight}
              alignY={0}
            />
            <Behavior semantic_label="REDUCED MOTION" />
            <Checkbox
              label=""
              checked={false}
              onToggle={(event) => scene.setReducedMotion(event.value)}
            />
          </Entity>
        </Setting>
      </Expander>
    </PanelBody>
  );
}

/** One setting: its label at the inset and its control at the row's end. */
function Setting({
  id,
  label,
  first = false,
  children,
}: {
  readonly id: string;
  readonly label: string;
  readonly first?: boolean;
  readonly children: ReactNode;
}) {
  return (
    <KitRow
      id={id}
      height={TOKENS.controlHeight}
      layout={{
        padding_left: TOKENS.inset,
        padding_right: TOKENS.inset,
        margin_top: first ? TOKENS.inset / 2 : ROW_GAP,
      }}
    >
      <TextLine
        id={`${id}/label`}
        text={label}
        tone="text"
        layout={{ flex: 1 }}
      />
      {children}
    </KitRow>
  );
}
