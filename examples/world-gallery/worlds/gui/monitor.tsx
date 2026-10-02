/**
 * SIGNAL MONITOR: the scope with its gain readout, then the controls that
 * drive the scene. GAIN sets the projector light, the cube's energy, the wave
 * amplitude and the nodes' signal; SCAN runs the trace and holds the uplink;
 * PULSE sends a burst across the scope; the callsign names the station and
 * UPLINK sends it. The header's SCOPE popover chooses the scope paint's
 * pattern and whether its sweep band runs; its window controls minimise the
 * panel to its title bar, restore it and close it.
 *
 * PULSE and UPLINK are the primary actions: they reference the action theme,
 * whose rows the ACCENT choice switches between the button look and the
 * amber look in place, so their identities, focus and values never change.
 * Each explains itself in a tooltip.
 */
import { Children, Entity } from "@ipp/react";
import {
  Behavior,
  Button,
  Checkbox,
  Skin,
  Slider,
  TextInput,
} from "@ipp/react/gui";
import {
  Panel,
  PanelHeader,
  Popover,
  RadioGroup,
  Row as KitRow,
  Separator,
  TextLine,
  Tooltip,
  WindowControls,
} from "@ipp/react/gui-kit";
import {
  BoxLayout,
  COLUMN,
  LEAF,
  PanelBody,
  ROW,
  Section,
  TOKENS,
} from "./presentation.js";
import type { GuiSceneState } from "./scene.js";
import { SCOPE_GRIDS, type ScopeGrid } from "./tuning.js";
import { WAVE_HEIGHT, Waveform } from "./waveform.js";

/** Theme entities the monitor's controls reference. */
export const ACTION_THEME = "gui-theme-action";
export const SWITCH_THEME = "gui-theme-switch";

/** Symbolic IDs of the monitor's controls. */
export const MONITOR_CONTROLS = {
  gain: "gui-gain",
  scan: "gui-scan",
  pulse: "gui-pulse",
  callsign: "gui-callsign",
  uplink: "gui-uplink",
  scope: "gui-scope-options",
  grid: "gui-scope-grid",
  sweep: "gui-scope-sweep",
} as const;

export const MONITOR_WIDTH = 384;

/** Control rows: a label column, the control, and an action at the end. */
const LABEL_WIDTH = 72;
const ACTION_WIDTH = 112;
const SWITCH_WIDTH = 72;
const ROW_GAP = TOKENS.inset / 2;

/** The scope row: the waveform beside the gain readout, at the inset. */
const SCOPE_HEIGHT = WAVE_HEIGHT + 2 * TOKENS.inset;

/** The monitor's height while open: header, scope, division and controls. */
export const MONITOR_HEIGHT =
  TOKENS.controlHeight +
  TOKENS.lineWidth +
  SCOPE_HEIGHT +
  TOKENS.lineWidth +
  2 * TOKENS.inset +
  3 * TOKENS.controlHeight +
  2 * ROW_GAP;

/** Initial values; each control keeps the operator's value afterwards. */
export const INITIAL_GAIN = 0.64;
export const INITIAL_AUTOSCAN = true;
export const INITIAL_CALLSIGN = "VESPER-7";
export const INITIAL_GRID: ScopeGrid = "lines";
export const INITIAL_SWEEP = true;

export function SignalMonitor({ scene }: { readonly scene: GuiSceneState }) {
  const window = scene.monitorWindow;
  const minimized = window === "minimized";
  const uplinkEnabled =
    !scene.autoscan && scene.callsign !== "" && !scene.station.busy;
  return (
    <Entity id="gui-monitor-slot">
      <BoxLayout
        kind={COLUMN}
        height={
          window === "closed"
            ? 0
            : minimized
              ? TOKENS.controlHeight
              : MONITOR_HEIGHT
        }
      />
      {window === "closed" && <Behavior visible={false} />}
      <Children>
        <Panel
          id="gui-monitor-panel"
          minimized={minimized}
          layout={{ height: MONITOR_HEIGHT }}
        >
          <PanelBody id="gui-monitor-content">
            <PanelHeader id="gui-monitor-header" title="SIGNAL MONITOR">
              <ScopePopover scene={scene} />
              <WindowControls
                id="gui-monitor-window"
                {...(minimized
                  ? { onRestore: () => scene.setMonitorWindow("normal") }
                  : { onMinimize: () => scene.setMonitorWindow("minimized") })}
                onClose={() => scene.setMonitorWindow("closed")}
              />
            </PanelHeader>
            <Section id="gui-monitor-body" hidden={minimized}>
              <Scope scene={scene} />
              <Separator id="gui-monitor-division" />
              <Entity id="gui-monitor-controls">
                <BoxLayout
                  kind={COLUMN}
                  padding={[
                    TOKENS.inset,
                    TOKENS.inset,
                    TOKENS.inset,
                    TOKENS.inset,
                  ]}
                />
                <Children>
                  <KitRow id="gui-gain-row" height={TOKENS.controlHeight}>
                    <Label id="gui-gain-label" text="GAIN" />
                    <Entity id={MONITOR_CONTROLS.gain}>
                      <BoxLayout
                        kind={LEAF}
                        height={TOKENS.controlHeight}
                        flex={1}
                      />
                      <Behavior semantic_label="GAIN" />
                      <Slider
                        value={INITIAL_GAIN}
                        min={0}
                        max={1}
                        step={0.05}
                        onScalarCommit={(event) => scene.setGain(event.value)}
                        onInteractionChange={(event) =>
                          scene.holdGain(event.pressed)
                        }
                      />
                    </Entity>
                  </KitRow>
                  <KitRow
                    id="gui-scan-row"
                    height={TOKENS.controlHeight}
                    layout={{ margin_top: ROW_GAP }}
                  >
                    <Label id="gui-scan-label" text="SCAN" />
                    <Entity id={MONITOR_CONTROLS.scan}>
                      <BoxLayout
                        kind={LEAF}
                        width={SWITCH_WIDTH}
                        height={TOKENS.smallHeight}
                        alignY={0}
                      />
                      <Skin theme={SWITCH_THEME} />
                      <Behavior semantic_label="SCAN" />
                      <Checkbox
                        label=""
                        checked={INITIAL_AUTOSCAN}
                        ref={scene.scanControl}
                        onToggle={(event) => scene.setAutoscan(event.value)}
                      />
                    </Entity>
                    <TextLine
                      id="gui-scan-state"
                      text={scene.autoscan ? "ACTIVE" : "STANDBY"}
                      tone="neutral"
                      layout={{ flex: 1, margin_left: TOKENS.inset }}
                    />
                    <Action
                      id={MONITOR_CONTROLS.pulse}
                      label="PULSE"
                      hint="Send a burst across the scope"
                      onPress={scene.pulse}
                    />
                  </KitRow>
                  <KitRow
                    id="gui-callsign-row"
                    height={TOKENS.controlHeight}
                    layout={{ margin_top: ROW_GAP }}
                  >
                    <Label id="gui-callsign-label" text="CALLSIGN" />
                    <Entity id={MONITOR_CONTROLS.callsign}>
                      <BoxLayout
                        kind={LEAF}
                        height={TOKENS.controlHeight}
                        flex={1}
                        margin={[0, TOKENS.inset, 0, 0]}
                      />
                      <Behavior semantic_label="CALLSIGN" />
                      <TextInput
                        text={INITIAL_CALLSIGN}
                        placeholder="CALLSIGN"
                        ref={scene.callsignControl}
                        onTextCommit={(event) => scene.setCallsign(event.value)}
                      />
                    </Entity>
                    <Action
                      id={MONITOR_CONTROLS.uplink}
                      label="UPLINK"
                      hint="Send the callsign to the relay"
                      enabled={uplinkEnabled}
                      onPress={scene.station.uplink}
                    />
                  </KitRow>
                </Children>
              </Entity>
            </Section>
          </PanelBody>
        </Panel>
      </Children>
    </Entity>
  );
}

/**
 * The scope's options in a popover from the header, beside the window
 * controls: the paint's pattern and whether the sweep band runs while SCAN
 * does. The trigger stands docked in the header's height.
 */
function ScopePopover({ scene }: { readonly scene: GuiSceneState }) {
  const tuning = scene.tuning;
  return (
    <Popover
      id={MONITOR_CONTROLS.scope}
      label="SCOPE"
      title="SCOPE"
      width={200}
      layout={{
        height: TOKENS.dockedHeight,
        margin_right: TOKENS.inset / 2,
      }}
    >
      <RadioGroup
        id={MONITOR_CONTROLS.grid}
        label="GRID"
        options={SCOPE_GRIDS.map(({ key, label }) => ({ value: key, label }))}
        defaultValue={INITIAL_GRID}
        onChange={(value) => tuning.setGrid(value as ScopeGrid)}
      />
      <KitRow
        id="gui-scope-sweep-row"
        height={TOKENS.smallHeight}
        layout={{ margin_top: TOKENS.inset / 2 }}
      >
        <TextLine
          id="gui-scope-sweep-label"
          text="SWEEP"
          tone="accent"
          layout={{ flex: 1 }}
        />
        <Entity id={MONITOR_CONTROLS.sweep}>
          <BoxLayout
            kind={LEAF}
            width={TOKENS.smallHeight}
            height={TOKENS.smallHeight}
          />
          <Behavior semantic_label="SWEEP" />
          <Checkbox
            label=""
            checked={INITIAL_SWEEP}
            onToggle={(event) => tuning.setSweepShown(event.value)}
          />
        </Entity>
      </KitRow>
    </Popover>
  );
}

/** The scope: the waveform, a vertical division and the gain readout. */
function Scope({ scene }: { readonly scene: GuiSceneState }) {
  return (
    <Entity id="gui-scope">
      <BoxLayout
        kind={ROW}
        height={SCOPE_HEIGHT}
        padding={[TOKENS.inset, TOKENS.inset, TOKENS.inset, TOKENS.inset]}
      />
      <Children>
        <Waveform scene={scene} />
        <Separator
          id="gui-scope-separator"
          vertical
          layout={{ margin_left: TOKENS.inset, margin_right: TOKENS.inset }}
        />
        <Entity id="gui-gain-readout">
          <BoxLayout kind={COLUMN} flex={1} alignY={0} />
          <Children>
            <TextLine
              id="gui-gain-readout-label"
              text="GAIN"
              tone="accent"
              size="small"
              layout={{ height: TOKENS.denseRow, align_y: -1 }}
            />
            <TextLine
              id="gui-gain-readout-value"
              text={`${Math.round(scene.gain * 100)}%`}
              size="display"
              layout={{ align_y: -1 }}
            />
          </Children>
        </Entity>
      </Children>
    </Entity>
  );
}

/** A row's label: accent body text in the label column. */
function Label({ id, text }: { readonly id: string; readonly text: string }) {
  return (
    <TextLine
      id={id}
      text={text}
      tone="accent"
      layout={{ width: LABEL_WIDTH }}
    />
  );
}

/**
 * A primary action at the end of a row, in the action theme, with a tooltip
 * the runtime opens while it is hovered or holds visible focus.
 */
function Action({
  id,
  label,
  hint,
  enabled = true,
  onPress,
}: {
  readonly id: string;
  readonly label: string;
  readonly hint: string;
  readonly enabled?: boolean;
  readonly onPress: () => void;
}) {
  return (
    <Entity id={id}>
      <BoxLayout
        kind={LEAF}
        width={ACTION_WIDTH}
        height={TOKENS.controlHeight}
        alignY={0}
      />
      <Skin theme={ACTION_THEME} />
      <Behavior enabled={enabled} />
      <Button label={label} onPress={onPress} />
      <Children>
        <Tooltip id={`${id}/tip`} text={hint} />
      </Children>
    </Entity>
  );
}
