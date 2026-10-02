/**
 * STATUS: what the station is doing, as feedback components show it. Badges
 * name the scan, the link and the input shield; one inline alert says the
 * most important thing worth saying now, with its recovery action; the pulse
 * ring follows a PULSE's crossing of the scope, and the operation row shows
 * the current or last operation: a spinner while it connects, then its
 * progress and outcome.
 */
import { Children, Entity } from "@ipp/react";
import {
  CircularProgress,
  InlineAlert,
  Panel,
  PanelHeader,
  ProgressBar,
  Row as KitRow,
  Spinner,
  StatusBadge,
} from "@ipp/react/gui-kit";
import { BoxLayout, COLUMN, PanelBody, TOKENS } from "./presentation.js";
import type { GuiScene } from "./scene.js";
import {
  ALERT_SEVERITY,
  linkBadge,
  operationLabel,
  scanBadge,
  stationAlertKind,
  stationAlertText,
  type StationBadge,
} from "./station.js";
import { useStoreValue } from "./store.js";

/** The ring's row and the operation beside it. */
const OPERATION_ROW = 96;

/** The pulse ring's column: its ring, or its caption PULSE 100%. */
const PULSE_RING_WIDTH = 96;

/** A progress bar's height: its label row, a gap and its frame. */
const PROGRESS_HEIGHT = TOKENS.denseRow + 4 + TOKENS.smallHeight;
const BADGE_GAP = TOKENS.inset / 2;

/** The panel's height: header, badges, alert and the operation row. */
export const STATUS_HEIGHT =
  TOKENS.controlHeight +
  TOKENS.lineWidth +
  TOKENS.inset +
  TOKENS.smallHeight +
  BADGE_GAP +
  TOKENS.controlHeight +
  BADGE_GAP +
  OPERATION_ROW +
  TOKENS.inset;

const SHIELD_ARMED: StationBadge = {
  key: "shield",
  status: "active",
  label: "SHIELD ARMED",
};
const SHIELD_LIFTED: StationBadge = {
  key: "shield",
  status: "warning",
  label: "SHIELD LIFTED",
};

/** The panel's frame; its badges, alert, pulse ring and operation each
 * select what they show. */
export function StatusPanel({ scene }: { readonly scene: GuiScene }) {
  return (
    <Panel id="gui-status-panel" layout={{ height: STATUS_HEIGHT }}>
      <PanelBody id="gui-status-content">
        <PanelHeader id="gui-status-header" title="STATUS" />
        <Entity id="gui-status-body">
          <BoxLayout
            kind={COLUMN}
            padding={[TOKENS.inset, TOKENS.inset, TOKENS.inset, TOKENS.inset]}
          />
          <Children>
            <Badges scene={scene} />
            <Alert scene={scene} />
            <KitRow
              id="gui-operations"
              height={OPERATION_ROW}
              layout={{ margin_top: BADGE_GAP }}
            >
              <PulseRing scene={scene} />
              <Operation scene={scene} />
            </KitRow>
          </Children>
        </Entity>
      </PanelBody>
    </Panel>
  );
}

/** Badges for the scan, the link and the input shield. */
function Badges({ scene }: { readonly scene: GuiScene }) {
  const scan = useStoreValue(scene.state, scanBadge);
  const link = useStoreValue(scene.state, linkBadge);
  const shield = useStoreValue(scene.state, (state) =>
    state.shieldArmed ? SHIELD_ARMED : SHIELD_LIFTED,
  );
  return (
    <KitRow id="gui-badges" height={TOKENS.smallHeight}>
      {[scan, link, shield].map((badge, index) => (
        <StatusBadge
          key={badge.key}
          id={`gui-badge-${badge.key}`}
          status={badge.status}
          label={badge.label}
          {...(index > 0 ? { layout: { margin_left: BADGE_GAP } } : {})}
        />
      ))}
    </KitRow>
  );
}

/** The one alert worth saying now, with its recovery action. */
function Alert({ scene }: { readonly scene: GuiScene }) {
  const kind = useStoreValue(scene.state, stationAlertKind);
  const text = useStoreValue(scene.state, stationAlertText);
  const action =
    kind === "callsign"
      ? { label: "EDIT", onPress: scene.focusCallsign }
      : kind === "scan"
        ? { label: "STOP", onPress: scene.stopScan }
        : undefined;
  return (
    <InlineAlert
      key={kind}
      id="gui-alert"
      severity={ALERT_SEVERITY[kind]}
      text={text}
      {...(action ? { action } : {})}
      layout={{ margin_top: BADGE_GAP }}
    />
  );
}

/**
 * The pulse transfer as a ring. Before the first PULSE nothing is being
 * sent, so the ring is idle: the quiet track alone, without an arc or a
 * percentage.
 */
function PulseRing({ scene }: { readonly scene: GuiScene }) {
  const pulse = useStoreValue(scene.state, (state) => state.pulse);
  return (
    <CircularProgress
      id="gui-pulse-ring"
      label="PULSE"
      size="small"
      value={pulse.value}
      idle={pulse.state === "idle"}
      {...(pulse.state === "complete" ? { status: "complete" as const } : {})}
      // Wide enough for its completed caption, so completing moves nothing
      // beside it.
      layout={{ width: PULSE_RING_WIDTH }}
    />
  );
}

/** The current or last operation: a spinner while it connects, then its
 * progress and outcome. */
function Operation({ scene }: { readonly scene: GuiScene }) {
  const operation = useStoreValue(scene.state, (state) => state.operation);
  return (
    <Entity id="gui-operation">
      <BoxLayout
        kind={COLUMN}
        flex={1}
        height={PROGRESS_HEIGHT}
        alignY={0}
        margin={[0, 0, 0, TOKENS.inset]}
      />
      <Children>
        {operation?.phase === "pending" ? (
          <Spinner
            id="gui-operation-pending"
            label={`${operationLabel(operation)}…`}
          />
        ) : (
          <ProgressBar
            id="gui-operation-progress"
            label={operation ? operationLabel(operation) : "Node sync"}
            segments={operation?.segments ?? []}
            {...(operation?.phase === "complete"
              ? { status: "complete" as const }
              : operation?.phase === "failed"
                ? { status: "failed" as const }
                : {})}
          />
        )}
      </Children>
    </Entity>
  );
}
