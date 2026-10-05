/** VESPER: a local login, connection terminal and focused scanner workspace. */
import { Animation, Children, Entity, type AnimationHandle } from "@ipp/react";
import {
  type GuiControlHandle,
  Behavior,
  Button,
  Checkbox,
  Font,
  LayerTransition,
  ScrollView,
  Skin,
  Style,
  TextInput,
  Theme,
} from "@ipp/react/gui";
import {
  Floating,
  InlineAlert,
  Knob,
  Panel,
  PanelHeader,
  PanelFooter,
  LabelledSlider,
  ProgressBar,
  Row as KitRow,
  SecondaryButton,
  SegmentedControl,
  Separator,
  TextLine,
  WindowControl,
  useOverlayOpen,
} from "@ipp/react/gui-kit";
import { useEffect, useRef, type ReactNode } from "react";
import { ColourTab } from "./colour-tab.js";
import {
  BoxLayout,
  COLUMN,
  CONTRACT,
  Fill,
  LEAF,
  Row,
  STACK,
  Stack,
  TextLeaf,
  TOKENS,
  CANVAS_HEIGHT,
  CANVAS_WIDTH,
} from "./presentation.js";
import { LOGIN_FADE_SECONDS } from "./app-state.js";
import { Radar, RadarPaintAsset, contactCount } from "./radar.js";
import { SurfacePanel, SURFACE_PANEL_WIDTH } from "./surface-panel.js";
import { SceneTree } from "./telemetry.js";
import { TuningTab } from "./tuning-tab.js";
import type { GuiScene } from "./scene.js";
import { useStoreValue } from "./store.js";
import { SCAN_RATES, type ScanRate, linearColor } from "./tuning.js";

export const APP_CONTROLS = {
  login: "gui-login",
  password: "gui-password",
  reveal: "gui-reveal-password",
  settings: "gui-settings-open",
  close: "gui-scanner-close",
  charge: "gui-charge",
  strength: "gui-pulse-strength",
  range: "gui-scan-range",
  pause: "gui-scan",
  pulse: "gui-pulse",
  log: "gui-log-open",
  settingsTabs: "gui-settings-tabs",
} as const;
const CARD_BACKGROUND = CONTRACT.GuiTheme.encodeParts({
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
const WORKSPACE_PANEL_BOTTOM = 651.75;
export const PULSE_RECT = [818.75, 547.75, 128, 40] as const;
export function AppButton({
  id,
  label,
  onPress,
  width = 144,
  disabled = false,
}: {
  readonly id: string;
  readonly label: string;
  readonly onPress: () => void;
  readonly width?: number;
  readonly disabled?: boolean;
}) {
  return (
    <Entity id={id}>
      <BoxLayout kind={LEAF} width={width} height={40} />
      <Behavior enabled={!disabled} />
      <Button label={label} onPress={onPress} />
    </Entity>
  );
}
function Caption({
  id,
  text,
  width,
}: {
  readonly id: string;
  readonly text: string;
  readonly width?: number;
}) {
  return (
    <TextLine
      id={id}
      text={text}
      tone="neutral"
      size="small"
      layout={{ height: 24, ...(width === undefined ? {} : { width }) }}
    />
  );
}
function Check({
  id,
  label,
  value,
  onChange,
  controlRef,
}: {
  readonly id: string;
  readonly label: string;
  readonly value: boolean;
  readonly onChange: (value: boolean) => void;
  readonly controlRef?: (handle: GuiControlHandle | null) => void;
}) {
  return (
    <KitRow id={`${id}/row`} height={24}>
      <Entity id={id}>
        <BoxLayout kind={LEAF} width={24} height={24} />
        <Behavior semantic_label={label} />
        <Checkbox
          checked={value}
          {...(controlRef ? { ref: controlRef } : {})}
          label=""
          onToggle={(event) => onChange(event.value)}
        />
      </Entity>
      <TextLine
        id={`${id}/label`}
        text={label}
        size="small"
        layout={{ margin_left: 12 }}
      />
    </KitRow>
  );
}
function Login({ scene }: { readonly scene: GuiScene }) {
  const app = useStoreValue(scene.state, (s) => s.app);
  const callsign = useStoreValue(scene.state, (s) => s.callsign);
  const animation = useRef<AnimationHandle>(null);
  const reduced = useStoreValue(scene.state, (s) => s.reducedMotion);
  useEffect(() => {
    if (app.phase !== "connecting" || !animation.current) return;
    let live = true;
    const handle = animation.current;
    const action = reduced
      ? Promise.all([
          handle.playAtSpeed(0),
          handle.seek(LOGIN_FADE_SECONDS),
          handle.pause(),
        ])
      : handle.play();
    void action.catch((error) => {
      if (live) scene.reportDeclarationFailure(error);
    });
    return () => {
      live = false;
    };
  }, [app.phase, reduced, scene.reportDeclarationFailure]);
  return (
    <Entity id="gui-login-card">
      <BoxLayout
        kind={COLUMN}
        width={400}
        height={392}
        alignX={0}
        alignY={0}
        padding={[24, 24, 24, 24]}
      />
      <Style layer={app.phase === "connecting" ? 2 : 1} opacity={1} />
      <LayerTransition previous_layer={1} progress={0} />
      <Behavior enabled={app.phase === "login"} />
      <Skin theme="gui-app-card-theme" />
      <Children>
        <TextLine
          id="gui-login-title"
          text="VESPER"
          tone="accent"
          size="display"
          layout={{ height: 40 }}
        />
        <Caption
          id="gui-login-subtitle"
          text="NEAR-FIELD SCANNER / LOCAL SIMULATION"
        />
        <Separator
          id="gui-login-line"
          layout={{ margin_top: 12, margin_bottom: 20 }}
        />
        <Caption id="gui-login-user-label" text="OPERATOR" />
        <Entity id="gui-callsign">
          <BoxLayout kind={LEAF} width={352} height={40} />
          <Behavior enabled={app.phase === "login"} />
          <TextInput
            ref={scene.callsignControl}
            text={callsign}
            onTextCommit={(event) => scene.setCallsign(event.value)}
            onSubmit={scene.app.login}
          />
        </Entity>
        <Caption id="gui-login-password-label" text="ACCESS PHRASE" />
        <Entity id={APP_CONTROLS.password}>
          <BoxLayout kind={LEAF} width={352} height={40} />
          <Behavior enabled={app.phase === "login"} />
          <TextInput
            text={app.password}
            masked={!app.reveal}
            placeholder="Anything works in this demo"
            onTextCommit={(event) => scene.app.setPassword(event.value)}
            onSubmit={scene.app.login}
          />
        </Entity>
        <Row id="gui-login-reveal-row" height={32} margin={[8, 0, 12, 0]}>
          <Check
            id={APP_CONTROLS.reveal}
            label="REVEAL PHRASE"
            value={app.reveal}
            onChange={scene.app.setReveal}
          />
        </Row>
        <AppButton
          id={APP_CONTROLS.login}
          label="LOG IN"
          width={352}
          disabled={!scene.ready || app.phase !== "login"}
          onPress={scene.app.login}
        />
        <Caption
          id="gui-login-demo-copy"
          text="Demo only. No account or connection required."
        />
      </Children>
      <Animation
        ref={animation}
        source={scene.motions!.login.source}
        autoPlay={false}
        bindings={scene.motions!.loginFields.map((field, track) => ({
          track,
          property: { component: field.component, offsets: [field.offset] },
          ...(track === 2 ? { target: "gui-connecting" } : {}),
        }))}
      />
    </Entity>
  );
}
function Terminal({
  lines,
  id,
  height,
  onError,
  active = true,
}: {
  readonly lines: readonly string[];
  readonly id: string;
  readonly height: number;
  readonly onError: (error: unknown) => void;
  readonly active?: boolean;
}) {
  const handle = useRef<GuiControlHandle>(null);
  const following = useRef(true);
  const capacity = useRef(0);
  const pendingTail = useRef<number | undefined>(undefined);
  return (
    <Entity id={id}>
      <BoxLayout kind={LEAF} height={height} clip />
      <Behavior semantic_label="CONNECTION TERMINAL" />
      <ScrollView
        ref={handle}
        onRangeChange={(range) => {
          capacity.current = range.capacity[1];
          if (!active || !following.current || !handle.current) return;
          // Geometry callbacks observe acknowledged layout, so a newly appended
          // line is included before its tail scroll is submitted.
          pendingTail.current = range.capacity[1];
          void handle.current
            .action({ kind: "scrollTo", offset: [0, range.capacity[1]] })
            .catch((error) => {
              pendingTail.current = undefined;
              onError(error);
            });
        }}
        onScroll={(event) => {
          const offset = event.value.offset[1];
          if (pendingTail.current !== undefined) {
            if (Math.abs(offset - pendingTail.current) < 1)
              pendingTail.current = undefined;
            return;
          }
          following.current = offset >= capacity.current - 1;
        }}
      />
      <Children>
        <Entity id={`${id}/body`}>
          <BoxLayout kind={COLUMN} padding={[12, 20, 12, 12]} />
          <Children>
            {lines.map((text, index) => (
              <TextLine
                key={index}
                id={`${id}/${index}`}
                text={text}
                tone="accent"
                size="small"
                layout={{ height: 24 }}
              />
            ))}
          </Children>
        </Entity>
      </Children>
    </Entity>
  );
}
function Connecting({ scene }: { readonly scene: GuiScene }) {
  const app = useStoreValue(scene.state, (s) => s.app);
  return (
    <Entity id="gui-connecting">
      <BoxLayout
        kind={COLUMN}
        width={560}
        height={330}
        alignX={0}
        alignY={0}
        padding={[24, 24, 24, 24]}
      />
      <Style layer={1} opacity={0} />
      <Skin theme="gui-app-card-theme" />
      <Behavior visible={app.phase === "connecting"} />
      <Children>
        <TextLine
          id="gui-connecting-title"
          text="LOGGING YOU IN"
          tone="accent"
          size="display"
          layout={{ height: 40 }}
        />
        <Caption
          id="gui-connecting-copy"
          text="Preparing your local scanner workspace"
        />
        <ProgressBar
          id="gui-login-progress"
          label="CONNECTION"
          value={app.progress}
          layout={{ margin_top: 12, margin_bottom: 16 }}
        />
        <Terminal
          active={app.phase === "connecting"}
          onError={scene.reportDeclarationFailure}
          id="gui-login-terminal"
          lines={app.lines}
          height={180}
        />
      </Children>
    </Entity>
  );
}
/** Charge preparation and the emitted pulse have separate, measured tracks. */
function EnergyMeter({
  id,
  label,
  value,
  readout,
  scene,
  width = 312,
  height = 44,
}: {
  readonly id: string;
  readonly label: string;
  readonly value: number;
  readonly readout: string;
  readonly scene: GuiScene;
  readonly width?: number;
  readonly height?: number;
}) {
  const color = useStoreValue(scene.state, (s) => s.tuning.color);
  return (
    <Entity id={id}>
      <BoxLayout kind={COLUMN} height={height} />
      <Children>
        <KitRow id={`${id}/header`} height={24}>
          <TextLine
            id={`${id}/label`}
            text={label}
            size="small"
            layout={{ flex: 1 }}
          />
          <TextLine
            id={`${id}/readout`}
            text={readout}
            size="small"
            tone="accent"
          />
        </KitRow>
        <Entity id={`${id}/track`}>
          <BoxLayout kind={STACK} height={12} />
          <Children>
            <Fill
              id={`${id}/background`}
              color={TOKENS.surface}
              width={width}
              height={12}
            />
            <Fill
              id={`${id}/fill`}
              color={[...linearColor(color), 1]}
              width={width * value}
              height={12}
            />
          </Children>
        </Entity>
      </Children>
    </Entity>
  );
}
/** The kit's labeled panel, with real 16-unit padding around its body. */
function ScannerPanel({
  id,
  title,
  left,
  top,
  width,
  height,
  layer,
  children,
  footer,
}: {
  readonly id: string;
  readonly title: string;
  readonly left: number;
  readonly top: number;
  readonly width: number;
  readonly height: number;
  readonly layer: number;
  readonly children: ReactNode;
  readonly footer?: ReactNode;
}) {
  const footerHeight = footer
    ? TOKENS.smallHeight + TOKENS.inset + TOKENS.lineWidth
    : 0;
  return (
    <Panel
      id={id}
      layer={layer}
      layout={{
        width,
        height,
        margin_left: left,
        margin_top: top,
        align_x: -1,
        align_y: -1,
      }}
    >
      <PanelHeader id={`${id}/header`} title={title} />
      <Entity id={`${id}/body`}>
        <BoxLayout
          kind={COLUMN}
          height={
            height - TOKENS.controlHeight - TOKENS.lineWidth - footerHeight
          }
          padding={[16, 16, 16, 16]}
        />
        <Children>{children}</Children>
      </Entity>
      {footer && <PanelFooter id={`${id}/footer`}>{footer}</PanelFooter>}
    </Panel>
  );
}
function Workspace({ scene }: { readonly scene: GuiScene }) {
  const gain = useStoreValue(scene.state, (s) => s.gain);
  const scanning = useStoreValue(scene.state, (s) => s.autoscan);
  const rate = useStoreValue(scene.state, (s) => s.tuning.rate);
  const shield = useStoreValue(scene.state, (s) => s.shieldArmed);
  const shieldReady = useStoreValue(
    scene.state,
    (s) => !s.shieldArmed || s.vectorOnly || s.shieldBlocker !== undefined,
  );
  const pulse = useStoreValue(scene.state, (s) => s.pulse);
  const app = useStoreValue(scene.state, (s) => s.app);
  const callsign = useStoreValue(scene.state, (s) => s.callsign);
  const events = useStoreValue(scene.state, (s) => s.events);
  return (
    <Panel
      id="gui-workspace"
      layout={{
        kind: STACK,
        width: CANVAS_WIDTH,
        height: CANVAS_HEIGHT,
        padding_left: 0,
        padding_right: 0,
      }}
    >
      <Entity id="gui-window-titlebar">
        <BoxLayout kind={STACK} width={CANVAS_WIDTH} height={48} />
        <Children>
          <Fill
            id="gui-window-titlebar-background"
            color={TOKENS.surface}
            width={CANVAS_WIDTH - 4}
            height={44}
            margin={[2, 0, 0, 2]}
            alignX={-1}
            alignY={-1}
          />
          <KitRow
            id="gui-window-titlebar-row"
            height={48}
            layout={{ padding_left: 24, padding_right: 12 }}
          >
            <TextLine
              id="gui-workspace-title"
              text="VESPER SCANNER"
              tone="accent"
              layout={{ flex: 1 }}
            />
            <WindowControl
              id={APP_CONTROLS.close}
              kind="close"
              label="CLOSE SCANNER"
              onPress={scene.app.logout}
            />
          </KitRow>
        </Children>
      </Entity>
      <Separator id="gui-window-titlebar-divider" layout={{ margin_top: 48 }} />
      <ScannerPanel
        id="gui-sweep-panel"
        title="SWEEP"
        left={48}
        top={64}
        width={550.5}
        height={WORKSPACE_PANEL_BOTTOM - 64}
        layer={1}
        footer={
          <>
            <SecondaryButton
              id={APP_CONTROLS.log}
              label={app.logOpen ? "CLOSE LOG" : "LOG"}
              onPress={scene.app.log}
              layout={{ width: 72, margin_right: 12 }}
            />
            <SecondaryButton
              id={APP_CONTROLS.settings}
              label="SETTINGS"
              onPress={() => scene.app.settings(true)}
              layout={{ width: 104 }}
            />
          </>
        }
      >
        <KitRow id="gui-sweep-display-row" height={416}>
          <Radar scene={scene} />
          <LabelledSlider
            id={APP_CONTROLS.range}
            label="RANGE"
            vertical
            length={320}
            bounds={false}
            min={1}
            max={4}
            step={0.25}
            value={app.range}
            onChange={scene.app.setRange}
            format={(value) => value.toFixed(2)}
            units="km"
            layout={{ width: 80, margin_left: 20 }}
          />
        </KitRow>
        <Separator
          id="gui-sweep-status-divider"
          layout={{ margin_top: 8, margin_bottom: 8 }}
        />
        <TextLine
          id="gui-scanner-status"
          text={`${scanning ? "SCANNING" : "SCAN HELD"} / ${contactCount(app.range)} CONTACTS / ${callsign}`}
          tone="accent"
          layout={{ height: 32 }}
        />
      </ScannerPanel>
      <ScannerPanel
        id="gui-receiver-panel"
        title="RECEIVER"
        left={624}
        top={64}
        width={364}
        height={201.25}
        layer={2}
      >
        <KitRow id="gui-receiver-row" height={128}>
          <Knob
            id="gui-gain"
            ref={scene.gainControl}
            label="GAIN"
            min={0}
            max={1}
            step={0.01}
            size={80}
            bounds={false}
            units="%"
            format={(value) => String(Math.round(value * 100))}
            value={gain}
            onChange={scene.setGain}
            layout={{ width: 120, margin_right: 16 }}
          />
          <Entity id="gui-sweep-controls">
            <BoxLayout kind={COLUMN} width={176} height={128} />
            <Children>
              <TextLine
                id="gui-rate-title"
                text="SWEEP RATE"
                tone="accent"
                layout={{ height: 28 }}
              />
              <SegmentedControl
                id="gui-scan-rate"
                options={SCAN_RATES.map((r) => ({
                  value: r.key,
                  label: r.label,
                }))}
                value={rate}
                onChange={(value) => scene.tuning.setRate(value as ScanRate)}
              />
              <Row id="gui-pause-row" height={24} margin={[16, 0, 0, 0]}>
                <Check
                  id={APP_CONTROLS.pause}
                  label="SCAN"
                  value={scanning}
                  onChange={scene.setAutoscan}
                  controlRef={scene.scanControl}
                />
              </Row>
            </Children>
          </Entity>
        </KitRow>
      </ScannerPanel>
      <ScannerPanel
        id="gui-preparation-panel"
        title="PULSE PREPARATION"
        left={624}
        top={277.25}
        width={364}
        height={165.25}
        layer={2}
      >
        <KitRow id="gui-charge-controls" height={48}>
          <LabelledSlider
            id={APP_CONTROLS.strength}
            label="STRENGTH"
            min={0.1}
            max={1}
            step={0.05}
            bounds={false}
            units="%"
            format={(value) => String(Math.round(value * 100))}
            value={app.strength}
            onChange={scene.app.setStrength}
            layout={{ width: 217.5, margin_right: 16 }}
          />
          <AppButton
            id={APP_CONTROLS.charge}
            label={
              app.charge.phase === "charging"
                ? "WAIT"
                : app.charge.phase === "ready"
                  ? "READY"
                  : "CHARGE"
            }
            width={96}
            disabled={app.charge.phase !== "idle" || pulse.state === "running"}
            onPress={scene.app.charge}
          />
        </KitRow>
        <Entity id="gui-charge-meter-slot">
          <BoxLayout kind={COLUMN} height={36} margin={[8, 0, 0, 0]} />
          <Children>
            <EnergyMeter
              scene={scene}
              id="gui-charge-progress"
              label="PREPARED CHARGE"
              value={app.charge.progress}
              width={329.5}
              height={36}
              readout={
                app.charge.phase === "ready"
                  ? "READY"
                  : app.charge.phase === "idle"
                    ? "EMPTY"
                    : `${Math.floor(app.charge.progress * 100)}%`
              }
            />
          </Children>
        </Entity>
      </ScannerPanel>
      <ScannerPanel
        id="gui-interlock-panel"
        title="INTERLOCK"
        left={624}
        top={454.5}
        width={364}
        height={WORKSPACE_PANEL_BOTTOM - 454.5}
        layer={2}
      >
        <Check
          id="gui-pulse-interlock"
          label="PULSE INTERLOCK"
          value={shield}
          onChange={scene.setShieldArmed}
        />
        <KitRow
          id="gui-interlock-action-row"
          height={44}
          layout={{ margin_top: 12 }}
        >
          <Entity id="gui-emitted-meter-slot">
            <BoxLayout
              kind={COLUMN}
              width={153.5}
              height={44}
              margin={[0, 24, 0, 0]}
            />
            <Children>
              <EnergyMeter
                scene={scene}
                id="gui-pulse-progress"
                label="EMITTED"
                value={pulse.state === "running" ? pulse.value : 0}
                width={153.5}
                readout={
                  pulse.state === "running"
                    ? `${Math.floor(pulse.value * 100)}%`
                    : "IDLE"
                }
              />
            </Children>
          </Entity>
          <Entity id="gui-fire-slot">
            <BoxLayout kind={LEAF} width={128} height={40} alignY={-1} />
          </Entity>
        </KitRow>
      </ScannerPanel>
      <Entity id="gui-pulse-footer">
        <BoxLayout
          kind={ROW_KIND}
          width={PULSE_RECT[2]}
          height={40}
          alignX={-1}
          alignY={-1}
          margin={[PULSE_RECT[1], 0, 0, PULSE_RECT[0]]}
        />
        <Style layer={3} />
        <Children>
          <AppButton
            id={APP_CONTROLS.pulse}
            width={PULSE_RECT[2]}
            label="FIRE PULSE"
            disabled={
              app.charge.phase !== "ready" ||
              pulse.state === "running" ||
              !shieldReady
            }
            onPress={scene.pulse}
          />
        </Children>
      </Entity>
      {app.logOpen && (
        <Entity id="gui-log-drawer">
          <BoxLayout
            kind={COLUMN}
            width={516}
            height={252}
            alignX={-1}
            alignY={1}
            margin={[0, 0, 76, 65.25]}
          />
          <Style layer={2} />
          <Skin theme="gui-app-card-theme" />
          <Children>
            <KitRow id="gui-log-header" height={36}>
              <TextLine
                id="gui-log-title"
                text="SESSION TERMINAL"
                tone="accent"
                layout={{ flex: 1 }}
              />
              <SecondaryButton
                id="gui-clear-log"
                label="CLEAR"
                onPress={scene.clearLog}
                layout={{ width: 80 }}
              />
            </KitRow>
            <Terminal
              onError={scene.reportDeclarationFailure}
              id="gui-session-terminal"
              lines={[...app.lines, ...events.slice(0, 20).reverse()]}
              height={200}
            />
          </Children>
        </Entity>
      )}
    </Panel>
  );
}
const ROW_KIND = 1;
function Settings({ scene }: { readonly scene: GuiScene }) {
  const app = useStoreValue(scene.state, (s) => s.app);
  const issue = useStoreValue(scene.state, (s) => s.declarationIssue);
  const presentation = useStoreValue(scene.state, (s) => s.presentationTab);
  const displayHelp = {
    layers:
      "Resting gap: 0.10 m. LAYER STEP sets the spacing when EXPLODE is on.",
    shell: "Choose the window curvature, facing direction and drawing policy.",
    style: "Choose the accent and reduce animation across this window.",
  }[presentation];
  const overlay = useOverlayOpen({
    open: app.settings,
    onOpenChange: scene.app.settings,
  });
  return (
    <Floating
      id="gui-settings"
      side="centre"
      align="centre"
      mode="modal"
      band="dialog"
      open={app.settings}
      onVisibleChange={overlay.onVisibleChange}
      layout={{ width: 816, height: issue ? 620 : 580 }}
    >
      <KitRow
        id="gui-settings-header"
        height={48}
        layout={{ padding_left: 20, padding_right: 12 }}
      >
        <TextLine
          id="gui-settings-title"
          text="SCANNER SETTINGS"
          tone="accent"
          layout={{ flex: 1 }}
        />
        <WindowControl
          id="gui-settings-close"
          kind="close"
          label="CLOSE SETTINGS"
          onPress={() => scene.app.settings(false)}
        />
      </KitRow>
      <SegmentedControl
        id={APP_CONTROLS.settingsTabs}
        options={[
          { value: "display", label: "DISPLAY" },
          { value: "projection", label: "PROJECTION" },
          { value: "scene", label: "SCENE" },
        ]}
        value={app.settingsPage}
        onChange={(value) =>
          scene.app.settingsPage(value as typeof app.settingsPage)
        }
      />
      <Separator id="gui-settings-line" />
      {issue && (
        <InlineAlert
          id="gui-settings-issue"
          severity="error"
          text="SCENE UPDATE REJECTED · correct the setting to continue"
          layout={{ height: 40 }}
        />
      )}
      <Stack id="gui-settings-content" height={480} padding={[20, 20, 20, 20]}>
        {app.settingsPage === "display" && (
          <Entity id="gui-display-settings">
            <BoxLayout
              kind={COLUMN}
              width={SURFACE_PANEL_WIDTH}
              height={400}
              alignX={0}
            />
            <Children>
              <TextLine
                id="gui-display-copy"
                text="Window presentation"
                tone="accent"
                layout={{ height: 32 }}
              />
              <SurfacePanel scene={scene} />
              <TextLeaf
                id="gui-display-help"
                text={displayHelp}
                font={scene.font.source}
                size={14}
                color={TOKENS.text}
                width={SURFACE_PANEL_WIDTH}
                height={48}
                margin={[20, 0, 0, 0]}
              />
            </Children>
          </Entity>
        )}
        {app.settingsPage === "projection" && (
          <Row id="gui-projection-settings" width={728} height={434} alignX={0}>
            <ScannerPanel
              id="gui-projection-tuning"
              title="BEAM & STUDIO"
              left={0}
              top={0}
              width={348}
              height={434}
              layer={0}
            >
              <TuningTab scene={scene} />
            </ScannerPanel>
            <ScannerPanel
              id="gui-projection-colour"
              title="PROJECTION COLOUR"
              left={16}
              top={0}
              width={364}
              height={434}
              layer={0}
            >
              <ColourTab scene={scene} />
            </ScannerPanel>
          </Row>
        )}
        {app.settingsPage === "scene" && (
          <ScannerPanel
            id="gui-scene-settings"
            title="SCENE INSPECTOR"
            left={20}
            top={0}
            width={728}
            height={434}
            layer={0}
          >
            <TextLine
              id="gui-scene-help"
              text="Select a part to highlight it in the projector"
              tone="text"
              layout={{ height: 24, margin_bottom: 12 }}
            />
            <SceneTree scene={scene} height={issue ? 268 : 324} />
            {issue && (
              <TextLeaf
                id="gui-rejection-detail"
                text={issue}
                font={scene.font.source}
                size={12}
                color={TOKENS.error}
                width={693.5}
                height={48}
                margin={[8, 0, 0, 0]}
              />
            )}
          </ScannerPanel>
        )}
      </Stack>
    </Floating>
  );
}
function Issue({ scene }: { readonly scene: GuiScene }) {
  const issue = useStoreValue(scene.state, (s) => s.declarationIssue);
  if (!issue) return null;
  return (
    <InlineAlert
      id="gui-scene-issue"
      layer={1}
      severity="error"
      text="SCENE UPDATE REJECTED · change the setting to continue"
      action={{ label: "SETTINGS", onPress: () => scene.app.settings(true) }}
      layout={{ width: 944, margin_left: 46, margin_top: 76 }}
    />
  );
}
export function ScannerApp({ scene }: { readonly scene: GuiScene }) {
  const phase = useStoreValue(scene.state, (s) => s.app.phase);
  return (
    <>
      <Entity id="gui-app-card-theme">
        <Theme parts={CARD_BACKGROUND} em={TOKENS.em} />
      </Entity>
      <Entity id="gui-app-paints">
        <RadarPaintAsset />
      </Entity>
      <Entity id="gui-canvas">
        <BoxLayout kind={STACK} width={CANVAS_WIDTH} height={CANVAS_HEIGHT} />
        <Font source={scene.font.source} font_size={16} />
        <Behavior visible={scene.prepared} />
        <Children>
          <Fill
            id="gui-page"
            width={CANVAS_WIDTH}
            height={CANVAS_HEIGHT}
            color={[0.008, 0.018, 0.023, 0.93]}
          />
          {phase !== "workspace" ? (
            <>
              <Connecting scene={scene} />
              <Login scene={scene} />
            </>
          ) : (
            <Workspace scene={scene} />
          )}
          <Issue scene={scene} />
        </Children>
      </Entity>
      {phase === "workspace" && <Settings scene={scene} />}
    </>
  );
}
