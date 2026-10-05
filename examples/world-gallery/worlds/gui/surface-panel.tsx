/** The Surface explains and controls its own presentation. Each tab retains
 * its controls while hidden, keeping runtime values and machine handles live. */
import { Children, Entity } from "@ipp/react";
import type { SurfaceFacing } from "@ipp/client";
import { Behavior, Checkbox, Skin, Style, Theme } from "@ipp/react/gui";
import {
  Dropdown,
  Knob,
  Row as KitRow,
  SegmentedControl,
  Separator,
  TextLine,
  Tooltip,
} from "@ipp/react/gui-kit";
import type { ReactNode } from "react";
import {
  BoxLayout,
  COLUMN,
  CONTRACT,
  LEAF,
  Row,
  Section,
  Stack,
  TOKENS,
} from "./presentation.js";
import {
  LAYER_STEP_MAX,
  LAYER_STEP_MIN,
  type Accent,
  type GuiScene,
  type GuiSurfaceCacheMode,
  type GuiSurfaceShape,
  type PresentationTab,
} from "./scene.js";
import { useStoreValue } from "./store.js";

export const SURFACE_PANEL_WIDTH = 640;
export const SURFACE_PANEL_HEIGHT = 236;
export const PRESENTATION_TABS = "gui-presentation-tabs";
const BODY_WIDTH =
  SURFACE_PANEL_WIDTH - 2 * TOKENS.lineWidth - 2 * TOKENS.inset;
const DIAL_COLUMN = 144;
const COLUMN_GAP = 24;
const ROW_GAP = 12;
const SELECT_WIDTH = BODY_WIDTH - 120;
const CHECKBOX_SIZE = 24;
const BACKGROUND = CONTRACT.GuiTheme.encodeParts({
  nextSlot: 1,
  rows: new Map([
    [
      0,
      {
        part: CONTRACT.guiPaintPartIndex({ part: "background" }),
        color: TOKENS.page,
      },
    ],
  ]),
});

/** LAYERS starts visible; SURFACE and STYLE remain available above every shell. */
export function SurfacePanel({ scene }: { readonly scene: GuiScene }) {
  const tab = useStoreValue(scene.state, (state) => state.presentationTab);
  return (
    <Entity id="gui-surface-panel">
      <Style layer={3} />
      <BoxLayout
        kind={COLUMN}
        width={SURFACE_PANEL_WIDTH}
        height={SURFACE_PANEL_HEIGHT}
        margin={[8, 0, 0, 0]}
        padding={[0, TOKENS.lineWidth, 0, TOKENS.lineWidth]}
      />
      <Theme parts={BACKGROUND} em={TOKENS.em} />
      <Skin theme="gui-surface-panel" />
      <Children>
        <SegmentedControl
          id={PRESENTATION_TABS}
          options={[
            { value: "layers", label: "LAYERS" },
            { value: "shell", label: "SURFACE" },
            { value: "style", label: "STYLE" },
          ]}
          value={tab}
          onChange={(value) =>
            scene.setPresentationTab(value as PresentationTab)
          }
        />
        <Separator id="gui-surface-panel-line" />
        <Stack
          id="gui-surface-panel-body"
          width={BODY_WIDTH}
          height={176}
          margin={[12, TOKENS.inset, 0, TOKENS.inset]}
        >
          <Section
            id="gui-layers-section"
            width={BODY_WIDTH}
            height={176}
            hidden={tab !== "layers"}
          >
            <Layers scene={scene} />
          </Section>
          <Section
            id="gui-shell-section"
            width={BODY_WIDTH}
            height={176}
            hidden={tab !== "shell"}
          >
            <Shell scene={scene} />
          </Section>
          <Section
            id="gui-style-section"
            width={BODY_WIDTH}
            height={176}
            hidden={tab !== "style"}
          >
            <Appearance scene={scene} />
          </Section>
        </Stack>
      </Children>
    </Entity>
  );
}

function Layers({ scene }: { readonly scene: GuiScene }) {
  const step = useStoreValue(scene.state, (state) => state.layerStep);
  const exploded = useStoreValue(scene.state, (state) => state.exploded);
  const isolated = useStoreValue(scene.state, (state) => state.vectorOnly);
  const shield = useStoreValue(scene.state, (state) => state.shieldArmed);
  return (
    <Row id="gui-layers-row" width={BODY_WIDTH} height={176}>
      <Stack id="gui-layer-dial-column" width={DIAL_COLUMN} height={176}>
        <Knob
          id="gui-layer-step"
          label="LAYER STEP"
          min={LAYER_STEP_MIN}
          max={LAYER_STEP_MAX}
          step={0.05}
          fineStep={0.01}
          size={96}
          bounds={false}
          units="m"
          format={(value) => value.toFixed(2)}
          value={step}
          onChange={scene.setLayerStep}
          ref={scene.layerStepControl}
          layout={{ align_x: 0 }}
        />
      </Stack>
      <Entity id="gui-layer-toggles">
        <BoxLayout
          kind={COLUMN}
          width={BODY_WIDTH - DIAL_COLUMN - COLUMN_GAP}
          margin={[0, 0, 0, COLUMN_GAP]}
        />
        <Children>
          <TextLine
            id="gui-layer-title"
            text="THIS PANEL"
            tone="accent"
            layout={{ height: TOKENS.smallHeight }}
          />
          <Toggle
            id="gui-explode"
            label="EXPLODE"
            semantic="EXPLODE LAYERS"
            checked={exploded}
            controlRef={scene.explodeControl}
            onToggle={scene.setExploded}
          />
          <Toggle
            id="gui-vector-only"
            label="GUI ONLY"
            semantic="GUI ONLY"
            checked={isolated}
            onToggle={scene.setVectorOnly}
            gap
          />
          <Toggle
            id="gui-shield-toggle"
            label="SHIELD"
            semantic="INPUT SHIELD"
            checked={shield}
            onToggle={scene.setShieldArmed}
            gap
          />
        </Children>
      </Entity>
    </Row>
  );
}

function Shell({ scene }: { readonly scene: GuiScene }) {
  const shape = useStoreValue(scene.state, (state) => state.surfaceShape);
  const facing = useStoreValue(scene.state, (state) => state.surfaceFacing);
  const cache = useStoreValue(scene.state, (state) => state.surfaceCache);
  return (
    <>
      <Setting id="gui-shape-row" label="SHAPE">
        <Dropdown
          id="gui-surface-shape"
          label="PANEL SHAPE"
          options={[
            { key: "flat", label: "FLAT" },
            { key: "cylinder", label: "CYLINDER" },
            { key: "sphere", label: "SPHERE" },
          ]}
          value={shape}
          onChange={(value) =>
            scene.selectSurfaceShape(value as GuiSurfaceShape)
          }
          layout={{ width: SELECT_WIDTH }}
        />
      </Setting>
      <Setting id="gui-facing-row" label="FACING" gap>
        <Dropdown
          id="gui-surface-facing"
          label="CURVED FACING"
          options={[
            { key: "outside", label: "OUTSIDE" },
            { key: "inside", label: "INSIDE" },
          ]}
          value={facing}
          disabled={shape === "flat"}
          onChange={(value) =>
            scene.selectSurfaceFacing(value as SurfaceFacing)
          }
          layout={{ width: SELECT_WIDTH }}
        />
      </Setting>
      <Setting id="gui-cache-row" label="DRAW" gap>
        <Dropdown
          id="gui-surface-cache"
          label="PRESENTATION"
          options={[
            { key: "automatic", label: "AUTO" },
            { key: "cached", label: "CACHED" },
            { key: "direct", label: "DIRECT" },
          ]}
          value={cache}
          onChange={(value) =>
            scene.selectSurfaceCache(value as GuiSurfaceCacheMode)
          }
          layout={{ width: SELECT_WIDTH }}
        />
      </Setting>
    </>
  );
}

function Appearance({ scene }: { readonly scene: GuiScene }) {
  const accent = useStoreValue(scene.state, (state) => state.accent);
  const motion = useStoreValue(scene.state, (state) => state.reducedMotion);
  return (
    <>
      <TextLine
        id="gui-style-title"
        text="THIS PANEL'S LOOK"
        tone="accent"
        layout={{ height: TOKENS.smallHeight }}
      />
      <Setting id="gui-accent-row" label="ACCENT">
        <SegmentedControl
          id="gui-accent"
          options={[
            { value: "cyan", label: "CYAN" },
            { value: "amber", label: "AMBER" },
          ]}
          value={accent}
          onChange={(value) => scene.setAccent(value as Accent)}
          layout={{ width: 160 }}
        />
      </Setting>
      <Toggle
        id="gui-reduced-motion"
        label="REDUCED MOTION"
        semantic="REDUCED MOTION"
        checked={motion}
        controlRef={scene.motionControl}
        onToggle={scene.setReducedMotion}
        gap
      />
    </>
  );
}

function Setting({
  id,
  label,
  gap = false,
  children,
}: {
  readonly id: string;
  readonly label: string;
  readonly gap?: boolean;
  readonly children: ReactNode;
}) {
  return (
    <KitRow
      id={id}
      height={TOKENS.controlHeight}
      layout={{ margin_top: gap ? ROW_GAP : 0 }}
    >
      <TextLine
        id={`${id}/label`}
        text={label}
        tone="text"
        size="small"
        layout={{ flex: 1 }}
      />
      {children}
    </KitRow>
  );
}

function Toggle({
  id,
  label,
  semantic,
  checked,
  gap = false,
  controlRef,
  onToggle,
}: {
  readonly id: string;
  readonly label: string;
  readonly semantic: string;
  readonly checked: boolean;
  readonly gap?: boolean;
  readonly controlRef?: GuiScene["explodeControl"];
  readonly onToggle: (value: boolean) => void;
}) {
  return (
    <KitRow
      id={`${id}-row`}
      height={TOKENS.smallHeight}
      layout={{ margin_top: gap ? ROW_GAP : 0 }}
    >
      <TextLine
        id={`${id}/label`}
        text={label}
        tone="text"
        size="small"
        layout={{ flex: 1 }}
      />
      <Entity id={id}>
        <BoxLayout
          kind={LEAF}
          width={CHECKBOX_SIZE}
          height={CHECKBOX_SIZE}
          alignY={0}
        />
        <Behavior semantic_label={semantic} />
        <Checkbox
          label=""
          checked={checked}
          {...(controlRef === undefined ? {} : { ref: controlRef })}
          onToggle={(event) => onToggle(event.value)}
        />
        <Children>
          <Tooltip id={`${id}/tip`} text={TOGGLE_HINTS[semantic] ?? semantic} />
        </Children>
      </Entity>
    </KitRow>
  );
}

const TOGGLE_HINTS: Readonly<Record<string, string>> = {
  "EXPLODE LAYERS": "Separate this panel's complete sections",
  "GUI ONLY": "Show this panel without the projector",
  "INPUT SHIELD": "Guard FIRE PULSE from pointer input",
  "REDUCED MOTION": "Snap this panel's movement and feedback",
};
