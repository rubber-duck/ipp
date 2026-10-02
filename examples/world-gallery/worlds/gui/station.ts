/**
 * The signal station the dashboard operates: its nodes and their signal,
 * the node table's selection and editing, the operations it runs (node sync,
 * purge and uplink), the pulse transfer, the alert worth saying now and the
 * toasts of completed actions. This is ordinary application state: the GUI
 * controls report what the operator did through their callbacks, and the
 * station decides what the dashboard shows next.
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
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
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
  const online = reached.filter(
    (node) => nodeSignal(node, gain) >= ONLINE_SIGNAL,
  ).length;
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

/** The one alert worth saying now, most important first. */
export interface StationAlert {
  readonly key: string;
  readonly severity: InlineAlertSeverity;
  readonly text: string;
  readonly action?: { readonly label: string; readonly onPress: () => void };
}

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

export interface StationInputs {
  /** Host frames of the panel World, once its session is open. */
  readonly frames: HostFrames | undefined;
  /** The station starts its first node sync once the panel is shown. */
  readonly ready: boolean;
  readonly gain: number;
  readonly autoscan: boolean;
  readonly callsign: string;
  /** Append an operator event to the log. */
  readonly record: (message: string) => void;
  readonly reportFailure: (failure: unknown) => void;
  /** Write SCAN off through its control, as the operator would. */
  readonly stopScan: () => void;
  /** Move focus to the callsign editor. */
  readonly focusCallsign: () => void;
}

export function useStation(inputs: StationInputs) {
  const { frames, ready, gain, autoscan, callsign } = inputs;
  const latest = useRef(inputs);
  latest.current = inputs;

  const [nodes, setNodes] = useState<readonly StationNode[]>([]);
  const nodesRef = useRef(nodes);
  nodesRef.current = nodes;
  const [selected, setSelected] = useState<string>();
  const [editing, setEditing] = useState(false);
  // PURGE asks before it removes the table.
  const [purgeAsked, setPurgeAsked] = useState(false);
  const [operation, setOperation] = useState<StationOperation>();
  const [pulse, setPulse] = useState<PulseTransfer>({
    state: "idle",
    value: 0,
  });
  const [toasts, setToasts] = useState<readonly ToastItem[]>([]);
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

  const dismissToast = useCallback((key: string) => {
    setToasts((current) => current.filter((item) => item.key !== key));
  }, []);
  const toast = useCallback(
    (item: Omit<ToastItem, "key">) => {
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
      // Toasts queue in order: the stack shows the first and the rest wait.
      setToasts((current) => [
        ...current,
        { ...item, ...(action ? { action } : {}), key },
      ]);
    },
    [dismissToast],
  );

  /** Run `body` as the current operation unless another one is running. */
  const run = useCallback(
    (
      body: (current: () => boolean, frames: HostFrames) => Promise<unknown>,
    ) => {
      const clock = latest.current.frames;
      if (!clock) return;
      const id = ++operationRun.current;
      const current = () => operationRun.current === id;
      void body(current, clock).catch((failure: unknown) => {
        if (current()) latest.current.reportFailure(failure);
      });
    },
    [],
  );

  const sync = useCallback(() => {
    run(async (current, clock) => {
      setOperation({ kind: "sync", phase: "pending", value: 0, segments: [] });
      if (!(await hostClock(clock, SYNC_CONNECT, () => {}, current))) return;
      setNodes([]);
      setOperation({ kind: "sync", phase: "running", value: 0, segments: [] });
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
              latest.current.gain,
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
        segments: syncSegments(SYNC_ORDER, total, latest.current.gain),
      });
      purged.current = [];
      const online = STATION_NODES.filter(
        (node) => nodeSignal(node, latest.current.gain) >= ONLINE_SIGNAL,
      ).length;
      latest.current.record(`NODE SYNC ${online}/${total} ONLINE`);
      toast({ severity: "success", text: `${online} nodes online.` });
    });
  }, [run, toast]);

  const purge = useCallback(() => {
    run(async (current, clock) => {
      purged.current = nodesRef.current;
      setSelected(undefined);
      setEditing(false);
      const total = Math.max(purged.current.length, 1);
      setOperation({ kind: "purge", phase: "running", value: 0, segments: [] });
      let removed = 0;
      const done = await hostClock(
        clock,
        PURGE_PER_NODE * total,
        (fraction) => {
          const count = Math.floor(fraction * total);
          if (count === removed) return;
          removed = count;
          setNodes(purged.current.slice(count));
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
      setNodes([]);
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
            setNodes(purged.current);
            purged.current = [];
            latest.current.record("NODE TABLE RESTORED");
          },
        },
      });
    });
  }, [run, toast]);

  const uplink = useCallback(() => {
    const sign = latest.current.callsign;
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
      const weak = latest.current.gain < LOW_GAIN;
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
          action: { label: "RETRY", onPress: () => uplinkRef.current() },
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
  }, [run, toast]);
  const uplinkRef = useRef(uplink);
  uplinkRef.current = uplink;

  /** The pulse transfer follows the pulse's crossing of the scope. */
  const startPulse = useCallback(() => {
    const clock = latest.current.frames;
    if (!clock) return;
    const id = ++pulseRun.current;
    const current = () => pulseRun.current === id;
    setPulse({ state: "running", value: 0 });
    let shown = 0;
    void hostClock(
      clock,
      PULSE_SECONDS,
      (fraction) => {
        const value = Math.floor(fraction * 100) / 100;
        if (value === shown) return;
        shown = value;
        setPulse({ state: value >= 1 ? "complete" : "running", value });
      },
      current,
    ).catch((failure: unknown) => {
      if (current()) latest.current.reportFailure(failure);
    });
  }, []);

  // The station syncs its nodes once the panel first shows.
  useEffect(() => {
    if (!ready || !frames || started.current) return;
    started.current = true;
    sync();
  }, [ready, frames, sync]);

  const busy = operation?.phase === "pending" || operation?.phase === "running";

  const pressRow = useCallback(
    (key: string) => {
      if (key === selected) {
        // Pressing the selected row edits its name; pressing it again while
        // editing leaves the name as it was.
        setEditing((current) => !current);
        return;
      }
      setSelected(key);
      setEditing(false);
    },
    [selected],
  );

  /** Select a node FIND accepted, without editing it. */
  const findNode = useCallback((key: string) => {
    const node = nodesRef.current.find((entry) => entry.key === key);
    if (!node) return;
    setSelected(key);
    setEditing(false);
    latest.current.record(`NODE ${node.name.toUpperCase()} FOUND`);
  }, []);

  /** Select a node and edit its name, as its context menu's Rename does. */
  const editNode = useCallback((key: string) => {
    setSelected(key);
    setEditing(true);
  }, []);

  /** Ask a node for its signal and log the answer. */
  const pingNode = useCallback((key: string) => {
    const node = nodesRef.current.find((entry) => entry.key === key);
    if (!node) return;
    latest.current.record(
      `NODE ${node.name.toUpperCase()} ANSWERED ${nodeSignal(node, latest.current.gain)}%`,
    );
  }, []);

  /** Remove one node; its toast restores it. */
  const removeNode = useCallback(
    (key: string) => {
      const node = nodesRef.current.find((entry) => entry.key === key);
      if (!node) return;
      setNodes((current) => current.filter((entry) => entry.key !== key));
      latest.current.record(`NODE ${node.name.toUpperCase()} REMOVED`);
      toast({
        severity: "warning",
        text: `${node.name} removed.`,
        action: {
          label: "RESTORE",
          onPress: () => {
            setNodes((current) =>
              current.some((entry) => entry.key === key)
                ? current
                : [...current, node],
            );
            latest.current.record(`NODE ${node.name.toUpperCase()} RESTORED`);
          },
        },
      });
    },
    [toast],
  );

  /** PURGE opens the confirmation; its answer purges or cancels. */
  const askPurge = useCallback(() => setPurgeAsked(true), []);
  const answerPurge = useCallback(
    (confirmed: boolean) => {
      setPurgeAsked(false);
      if (confirmed) purge();
      else latest.current.record("NODE PURGE CANCELLED");
    },
    [purge],
  );

  const renameNode = useCallback((cell: DataGridCell, text: string) => {
    const name = text.trim().slice(0, 10);
    setEditing(false);
    if (name === "") return;
    setNodes((current) => {
      const node = current.find(({ key }) => key === cell.row);
      if (!node || node.name === name) return current;
      latest.current.record(
        `NODE ${node.name.toUpperCase()} RENAMED ${name.toUpperCase()}`,
      );
      return current.map((entry) =>
        entry.key === cell.row ? { ...entry, name } : entry,
      );
    });
  }, []);

  // A node the table no longer holds cannot stay selected.
  useEffect(() => {
    if (selected !== undefined && !nodes.some(({ key }) => key === selected)) {
      setSelected(undefined);
      setEditing(false);
    }
  }, [nodes, selected]);

  const rows = useMemo(() => nodeRows(nodes, gain), [nodes, gain]);
  const online = rows.filter(({ cells }) => cells.status === "Online").length;
  const uplinking =
    operation?.kind === "uplink" &&
    (operation.phase === "pending" || operation.phase === "running");

  const alert = useMemo<StationAlert>(() => {
    if (callsign === "")
      return {
        key: "callsign",
        severity: "error",
        text: "Uplink needs a callsign.",
        action: {
          label: "EDIT",
          onPress: () => latest.current.focusCallsign(),
        },
      };
    if (gain < LOW_GAIN)
      return {
        key: "gain",
        severity: "warning",
        text:
          nodes.length > online
            ? `Low gain: ${nodes.length - online} nodes on standby.`
            : "Low gain: the uplink will drop.",
      };
    if (autoscan)
      return {
        key: "scan",
        severity: "information",
        text: "Scanning holds the uplink.",
        action: { label: "STOP", onPress: () => latest.current.stopScan() },
      };
    return {
      key: "ready",
      severity: "information",
      text: `Uplink ready: ${callsign}.`,
    };
  }, [callsign, gain, autoscan, nodes.length, online]);

  const badges = useMemo<readonly StationBadge[]>(
    () => [
      autoscan
        ? { key: "scan", status: "busy", label: "SCANNING" }
        : { key: "scan", status: "inactive", label: "STANDBY" },
      uplinking
        ? { key: "link", status: "busy", label: "UPLINKING" }
        : callsign === ""
          ? { key: "link", status: "error", label: "OFFLINE" }
          : autoscan
            ? { key: "link", status: "inactive", label: "HELD" }
            : { key: "link", status: "active", label: "ONLINE" },
    ],
    [autoscan, uplinking, callsign],
  );

  return {
    nodes,
    rows,
    online,
    selected,
    focusedCell:
      selected === undefined
        ? undefined
        : ({ row: selected, column: "node" } as DataGridCell),
    editingCell:
      selected !== undefined && editing
        ? ({ row: selected, column: "node" } as DataGridCell)
        : undefined,
    operation,
    busy,
    pulse,
    alert,
    badges,
    toasts,
    sync,
    purgeAsked,
    askPurge,
    answerPurge,
    uplink,
    startPulse,
    pressRow,
    findNode,
    editNode,
    pingNode,
    removeNode,
    renameNode,
    toast,
    dismissToast,
  };
}

export type Station = ReturnType<typeof useStation>;
