/**
 * The signal station the dashboard operates: its nodes and their signal,
 * the node table's selection and editing, the operations it runs (node sync,
 * purge and uplink), the pulse transfer, the alert worth saying now and the
 * toasts of completed actions. This is ordinary application state in the
 * page's store: the GUI controls report what the operator did through their
 * callbacks, the station decides what the dashboard shows next, and each part
 * of the dashboard selects what it shows.
 *
 * Operations advance on the Host clock: each step waits for the Host's next
 * frames through the panel World's session and measures elapsed Host time, so
 * a slow or paused Host slows them with everything else, and no client timer
 * runs. Their effects are real: a sync adds the node rows one by one, a purge
 * removes them, and an uplink writes the event log.
 */
import type { GuiPressEvent } from "@ipp/react/gui";
import type {
  DataGridCell,
  DataGridRow,
  InlineAlertSeverity,
  ProgressBarSegment,
  StatusBadgeStatus,
  ToastItem,
} from "@ipp/react/gui-kit";
import { useEffect, useMemo, useRef } from "react";
import type { Store } from "./store.js";
import { PULSE_SECONDS } from "./waveform.js";

export interface StationNode {
  readonly key: string;
  readonly name: string;
  /** Signal strength at full gain, 0..1. */
  readonly base: number;
}

/** The station's nodes as a sync reports them. */
export const STATION_NODES: readonly StationNode[] = [
  { key: "alpha", name: "Alpha", base: 0.96 },
  { key: "bravo", name: "Bravo", base: 0.58 },
  { key: "charlie", name: "Charlie", base: 0.88 },
  { key: "delta", name: "Delta", base: 0.47 },
  { key: "echo", name: "Echo", base: 0.74 },
  { key: "foxtrot", name: "Foxtrot", base: 0.33 },
  { key: "golf", name: "Golf", base: 0.66 },
  { key: "hotel", name: "Hotel", base: 0.81 },
];

/** A sync reports the strongest nodes first, so rows arrive top down. */
const SYNC_ORDER = [...STATION_NODES].sort(
  (left, right) => right.base - left.base,
);

/** A node's signal in percent at the given gain. */
export function nodeSignal(node: StationNode, gain: number): number {
  return Math.round(100 * node.base * (0.35 + 0.65 * gain));
}

/** Nodes at or above this signal are online; the rest stand by. */
export const ONLINE_SIGNAL = 40;

/** Below this gain the uplink fails and the station warns. */
export const LOW_GAIN = 0.25;

/** The node table's rows, strongest signal first. */
export function nodeRows(
  nodes: readonly StationNode[],
  gain: number,
): DataGridRow[] {
  return nodes
    .map((node) => ({ node, signal: nodeSignal(node, gain) }))
    .sort(
      (left, right) =>
        right.signal - left.signal ||
        left.node.key.localeCompare(right.node.key),
    )
    .map(({ node, signal }) => ({
      key: node.key,
      cells: {
        node: node.name,
        signal: `${signal}%`,
        status: signal >= ONLINE_SIGNAL ? "Online" : "Standby",
      },
    }));
}

export type OperationKind = "sync" | "purge" | "uplink";

/**
 * One operation: pending while it connects (a spinner), running with its
 * completed share, then complete or failed.
 */
export interface StationOperation {
  readonly kind: OperationKind;
  readonly phase: "pending" | "running" | "complete" | "failed";
  readonly value: number;
  /**
   * The completed share in parts: a sync's online and standby nodes, a
   * purge's removed nodes in amber, an uplink's sent share.
   */
  readonly segments: readonly ProgressBarSegment[];
  /** The callsign an uplink sends. */
  readonly callsign?: string;
}

/** A sync's completed share: the nodes reached so far, online then standby. */
function syncSegments(
  reached: readonly StationNode[],
  total: number,
  gain: number,
): ProgressBarSegment[] {
  const online = onlineCount(reached, gain);
  return [
    { value: online / total, tone: "accent" },
    { value: (reached.length - online) / total, tone: "neutral" },
  ];
}

/** What the operation row says about an operation. */
export function operationLabel(operation: StationOperation): string {
  switch (operation.kind) {
    case "sync":
      return operation.phase === "pending"
        ? "Connecting to nodes"
        : "Node sync";
    case "purge":
      return "Node purge";
    case "uplink":
      return operation.phase === "pending"
        ? "Opening uplink"
        : `Uplink ${operation.callsign ?? ""}`.trim();
  }
}

/** The pulse transfer the ring shows: idle, running or complete. */
export interface PulseTransfer {
  readonly state: "idle" | "running" | "complete";
  readonly value: number;
}

/** The kinds of alert the station raises, most important first. */
export type StationAlertKind = "callsign" | "gain" | "scan" | "ready";

/** Each alert's severity. */
export const ALERT_SEVERITY: Readonly<
  Record<StationAlertKind, InlineAlertSeverity>
> = {
  callsign: "error",
  gain: "warning",
  scan: "information",
  ready: "information",
};

export interface StationBadge {
  readonly key: string;
  readonly status: StatusBadgeStatus;
  readonly label: string;
}

/** Host time of the next frames the panel World's session receives. */
export type HostFrames = () => Promise<{ readonly time: number }>;

/**
 * Advance `step` with the Host time elapsed over `seconds`, frame by frame,
 * until it reaches one or the operation stops being current.
 */
async function hostClock(
  frames: HostFrames,
  seconds: number,
  step: (fraction: number) => void,
  current: () => boolean,
): Promise<boolean> {
  const start = (await frames()).time;
  for (;;) {
    const { time } = await frames();
    if (!current()) return false;
    const fraction = seconds > 0 ? Math.min(1, (time - start) / seconds) : 1;
    step(fraction);
    if (fraction >= 1) return true;
  }
}

/** Seconds of each operation's phases on the Host clock. */
const SYNC_CONNECT = 0.8;
const SYNC_PER_NODE = 0.22;
const PURGE_PER_NODE = 0.14;
const UPLINK_OPEN = 0.6;
const UPLINK_SEND = 1.8;
/** Where a weak uplink drops. */
const UPLINK_DROP = 0.45;

/** The station's part of the page's state. */
export interface StationState {
  readonly nodes: readonly StationNode[];
  /** The selected node's key. */
  readonly selected: string | undefined;
  /** Whether the selected node's name is being edited. */
  readonly editing: boolean;
  /** Whether PURGE's confirmation is open. */
  readonly purgeAsked: boolean;
  /** The current operation, or the last one. */
  readonly operation: StationOperation | undefined;
  readonly pulse: PulseTransfer;
  /** Toasts in order: the stack shows the first and the rest wait. */
  readonly toasts: readonly ToastItem[];
}

export const INITIAL_STATION: StationState = {
  nodes: [],
  selected: undefined,
  editing: false,
  purgeAsked: false,
  operation: undefined,
  pulse: { state: "idle", value: 0 },
  toasts: [],
};

/** The station's state with the control values it reads. */
export interface StationView extends StationState {
  readonly gain: number;
  readonly autoscan: boolean;
  readonly callsign: string;
}

/** How many of the nodes are online at the given gain. */
export function onlineCount(
  nodes: readonly StationNode[],
  gain: number,
): number {
  return nodes.filter((node) => nodeSignal(node, gain) >= ONLINE_SIGNAL).length;
}

/** Whether an operation is connecting or running. */
export function isBusy(operation: StationOperation | undefined): boolean {
  return operation?.phase === "pending" || operation?.phase === "running";
}

const BADGES = {
  scanning: { key: "scan", status: "busy", label: "SCANNING" },
  standby: { key: "scan", status: "inactive", label: "STANDBY" },
  uplinking: { key: "link", status: "busy", label: "UPLINKING" },
  offline: { key: "link", status: "error", label: "OFFLINE" },
  held: { key: "link", status: "inactive", label: "HELD" },
  online: { key: "link", status: "active", label: "ONLINE" },
} as const satisfies Record<string, StationBadge>;

/** The scan's badge. */
export function scanBadge(station: StationView): StationBadge {
  return station.autoscan ? BADGES.scanning : BADGES.standby;
}

/** The link's badge: uplinking, offline without a callsign, held while SCAN
 * runs, or online. */
export function linkBadge(station: StationView): StationBadge {
  const { operation } = station;
  if (operation?.kind === "uplink" && isBusy(operation))
    return BADGES.uplinking;
  if (station.callsign === "") return BADGES.offline;
  return station.autoscan ? BADGES.held : BADGES.online;
}

/** The alert worth saying now, most important first: a missing callsign, low
 * gain, the uplink SCAN holds, or the ready link. */
export function stationAlertKind(station: StationView): StationAlertKind {
  if (station.callsign === "") return "callsign";
  if (station.gain < LOW_GAIN) return "gain";
  if (station.autoscan) return "scan";
  return "ready";
}

/** What the alert says. */
export function stationAlertText(station: StationView): string {
  switch (stationAlertKind(station)) {
    case "callsign":
      return "Uplink needs a callsign.";
    case "gain": {
      const standby =
        station.nodes.length - onlineCount(station.nodes, station.gain);
      return standby > 0
        ? `Low gain: ${standby} nodes on standby.`
        : "Low gain: the uplink will drop.";
    }
    case "scan":
      return "Scanning holds the uplink.";
    case "ready":
      return `Uplink ready: ${station.callsign}.`;
  }
}

export interface StationInputs {
  /** Host frames of the panel World, once its session is open. */
  readonly frames: HostFrames | undefined;
  /** The station starts its first node sync once the panel is shown. */
  readonly ready: boolean;
  /** Append an operator event to the log. */
  readonly record: (message: string) => void;
  readonly reportFailure: (failure: unknown) => void;
}

/**
 * The station's operations and what the operator does to its nodes. They
 * keep the station's state in `state`, the page's state, which components
 * select from.
 */
export function useStation(state: Store<StationView>, inputs: StationInputs) {
  const { frames, ready } = inputs;
  const latest = useRef(inputs);
  latest.current = inputs;
  const toastSequence = useRef(0);
  const purged = useRef<readonly StationNode[]>([]);
  // The operation and pulse that are current; a newer one, or leaving the
  // page, makes an older one stop at its next frame.
  const operationRun = useRef(0);
  const pulseRun = useRef(0);
  const started = useRef(false);

  useEffect(
    () => () => {
      operationRun.current += 1;
      pulseRun.current += 1;
    },
    [],
  );

  const actions = useMemo(() => {
    /** Replace the nodes; a node the table no longer holds cannot stay
     * selected. */
    const setNodes = (
      next: (nodes: readonly StationNode[]) => readonly StationNode[],
    ) =>
      state.update(({ nodes, selected }) => {
        const updated = next(nodes);
        return selected !== undefined &&
          !updated.some(({ key }) => key === selected)
          ? { nodes: updated, selected: undefined, editing: false }
          : { nodes: updated };
      });
    const setOperation = (operation: StationOperation) =>
      state.update({ operation });

    const dismissToast = (key: string) =>
      state.update(({ toasts }) => ({
        toasts: toasts.filter((item) => item.key !== key),
      }));
    const toast = (item: Omit<ToastItem, "key">) => {
      toastSequence.current += 1;
      const key = `toast-${toastSequence.current}`;
      // A toast's action answers it, so the toast goes once the action runs.
      const action = item.action && {
        ...item.action,
        onPress: (event: GuiPressEvent) => {
          dismissToast(key);
          item.action!.onPress?.(event);
        },
      };
      state.update(({ toasts }) => ({
        toasts: [...toasts, { ...item, ...(action ? { action } : {}), key }],
      }));
    };

    /** Run `body` as the current operation unless another one is running. */
    const run = (
      body: (current: () => boolean, frames: HostFrames) => Promise<unknown>,
    ) => {
      const clock = latest.current.frames;
      if (!clock) return;
      const id = ++operationRun.current;
      const current = () => operationRun.current === id;
      void body(current, clock).catch((failure: unknown) => {
        if (current()) latest.current.reportFailure(failure);
      });
    };

    const sync = () => {
      run(async (current, clock) => {
        setOperation({
          kind: "sync",
          phase: "pending",
          value: 0,
          segments: [],
        });
        if (!(await hostClock(clock, SYNC_CONNECT, () => {}, current))) return;
        setNodes(() => []);
        setOperation({
          kind: "sync",
          phase: "running",
          value: 0,
          segments: [],
        });
        const total = SYNC_ORDER.length;
        let shown = 0;
        const done = await hostClock(
          clock,
          SYNC_PER_NODE * total,
          (fraction) => {
            const count = Math.floor(fraction * total);
            if (count === shown) return;
            shown = count;
            // A sync keeps the names the operator gave the nodes.
            setNodes((previous) =>
              SYNC_ORDER.slice(0, count).map(
                (node) =>
                  previous.find(({ key }) => key === node.key) ??
                  purged.current.find(({ key }) => key === node.key) ??
                  node,
              ),
            );
            setOperation({
              kind: "sync",
              phase: "running",
              value: count / total,
              segments: syncSegments(
                SYNC_ORDER.slice(0, count),
                total,
                state.current.gain,
              ),
            });
          },
          current,
        );
        if (!done) return;
        setOperation({
          kind: "sync",
          phase: "complete",
          value: 1,
          segments: syncSegments(SYNC_ORDER, total, state.current.gain),
        });
        purged.current = [];
        const online = onlineCount(STATION_NODES, state.current.gain);
        latest.current.record(`NODE SYNC ${online}/${total} ONLINE`);
        toast({ severity: "success", text: `${online} nodes online.` });
      });
    };

    const purge = () => {
      run(async (current, clock) => {
        purged.current = state.current.nodes;
        state.update({ selected: undefined, editing: false });
        const total = Math.max(purged.current.length, 1);
        setOperation({
          kind: "purge",
          phase: "running",
          value: 0,
          segments: [],
        });
        let removed = 0;
        const done = await hostClock(
          clock,
          PURGE_PER_NODE * total,
          (fraction) => {
            const count = Math.floor(fraction * total);
            if (count === removed) return;
            removed = count;
            setNodes(() => purged.current.slice(count));
            setOperation({
              kind: "purge",
              phase: "running",
              value: count / total,
              segments: [{ value: count / total, tone: "amber" }],
            });
          },
          current,
        );
        if (!done) return;
        setNodes(() => []);
        setOperation({
          kind: "purge",
          phase: "complete",
          value: 1,
          segments: [{ value: 1, tone: "amber" }],
        });
        latest.current.record("NODE TABLE PURGED");
        toast({
          severity: "warning",
          text: "Nodes purged.",
          action: {
            label: "RESTORE",
            onPress: () => {
              setNodes(() => purged.current);
              purged.current = [];
              latest.current.record("NODE TABLE RESTORED");
            },
          },
        });
      });
    };

    const uplink = () => {
      const sign = state.current.callsign;
      run(async (current, clock) => {
        setOperation({
          kind: "uplink",
          phase: "pending",
          value: 0,
          segments: [],
          callsign: sign,
        });
        if (!(await hostClock(clock, UPLINK_OPEN, () => {}, current))) return;
        // A weak signal drops the link part of the way through.
        const weak = state.current.gain < LOW_GAIN;
        const reach = weak ? UPLINK_DROP : 1;
        let shown = -1;
        const done = await hostClock(
          clock,
          UPLINK_SEND * reach,
          (fraction) => {
            const value = Math.floor(fraction * reach * 100) / 100;
            if (value === shown) return;
            shown = value;
            setOperation({
              kind: "uplink",
              phase: "running",
              value,
              segments: [{ value }],
              callsign: sign,
            });
          },
          current,
        );
        if (!done) return;
        if (weak) {
          setOperation({
            kind: "uplink",
            phase: "failed",
            value: UPLINK_DROP,
            segments: [{ value: UPLINK_DROP }],
            callsign: sign,
          });
          latest.current.record(`UPLINK ${sign} DROPPED`);
          toast({
            severity: "error",
            text: "Uplink dropped.",
            action: { label: "RETRY", onPress: () => uplink() },
          });
          return;
        }
        setOperation({
          kind: "uplink",
          phase: "complete",
          value: 1,
          segments: [{ value: 1 }],
          callsign: sign,
        });
        latest.current.record(`UPLINK ${sign} SENT`);
        toast({ severity: "success", text: "Uplink sent." });
      });
    };

    /** The pulse transfer follows the pulse's crossing of the scope. */
    const startPulse = () => {
      const clock = latest.current.frames;
      if (!clock) return;
      const id = ++pulseRun.current;
      const current = () => pulseRun.current === id;
      state.update({ pulse: { state: "running", value: 0 } });
      let shown = 0;
      void hostClock(
        clock,
        PULSE_SECONDS,
        (fraction) => {
          const value = Math.floor(fraction * 100) / 100;
          if (value === shown) return;
          shown = value;
          state.update({
            pulse: { state: value >= 1 ? "complete" : "running", value },
          });
        },
        current,
      ).catch((failure: unknown) => {
        if (current()) latest.current.reportFailure(failure);
      });
    };

    /** Pressing a row selects it; pressing the selected row edits its name,
     * and pressing it again while editing leaves the name as it was. */
    const pressRow = (key: string) => {
      if (key === state.current.selected)
        state.update(({ editing }) => ({ editing: !editing }));
      else state.update({ selected: key, editing: false });
    };

    /** Select a node FIND accepted, without editing it. */
    const findNode = (key: string) => {
      const node = state.current.nodes.find((entry) => entry.key === key);
      if (!node) return;
      state.update({ selected: key, editing: false });
      latest.current.record(`NODE ${node.name.toUpperCase()} FOUND`);
    };

    /** Select a node and edit its name, as its context menu's Rename does. */
    const editNode = (key: string) =>
      state.update({ selected: key, editing: true });

    /** Ask a node for its signal and log the answer. */
    const pingNode = (key: string) => {
      const node = state.current.nodes.find((entry) => entry.key === key);
      if (!node) return;
      latest.current.record(
        `NODE ${node.name.toUpperCase()} ANSWERED ${nodeSignal(node, state.current.gain)}%`,
      );
    };

    /** Remove one node; its toast restores it. */
    const removeNode = (key: string) => {
      const node = state.current.nodes.find((entry) => entry.key === key);
      if (!node) return;
      setNodes((nodes) => nodes.filter((entry) => entry.key !== key));
      latest.current.record(`NODE ${node.name.toUpperCase()} REMOVED`);
      toast({
        severity: "warning",
        text: `${node.name} removed.`,
        action: {
          label: "RESTORE",
          onPress: () => {
            setNodes((nodes) =>
              nodes.some((entry) => entry.key === key)
                ? nodes
                : [...nodes, node],
            );
            latest.current.record(`NODE ${node.name.toUpperCase()} RESTORED`);
          },
        },
      });
    };

    /** PURGE opens the confirmation; its answer purges or cancels. */
    const askPurge = () => state.update({ purgeAsked: true });
    const answerPurge = (confirmed: boolean) => {
      state.update({ purgeAsked: false });
      if (confirmed) purge();
      else latest.current.record("NODE PURGE CANCELLED");
    };

    const renameNode = (cell: DataGridCell, text: string) => {
      const name = text.trim().slice(0, 10);
      state.update({ editing: false });
      if (name === "") return;
      const node = state.current.nodes.find(({ key }) => key === cell.row);
      if (!node || node.name === name) return;
      latest.current.record(
        `NODE ${node.name.toUpperCase()} RENAMED ${name.toUpperCase()}`,
      );
      setNodes((nodes) =>
        nodes.map((entry) =>
          entry.key === cell.row ? { ...entry, name } : entry,
        ),
      );
    };

    return {
      sync,
      askPurge,
      answerPurge,
      uplink,
      startPulse,
      cancelPulse: () => {
        pulseRun.current += 1;
        state.update({ pulse: { state: "idle", value: 0 } });
      },
      pressRow,
      findNode,
      editNode,
      pingNode,
      removeNode,
      renameNode,
      toast,
      dismissToast,
    };
  }, [state]);

  // The station syncs its nodes once the panel first shows.
  useEffect(() => {
    if (!ready || !frames || started.current) return;
    started.current = true;
    actions.sync();
  }, [ready, frames, actions]);

  return actions;
}

export type StationActions = ReturnType<typeof useStation>;
