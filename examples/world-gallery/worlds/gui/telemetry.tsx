/**
 * TELEMETRY: the station's readouts beside a vertical separator, wrapped
 * operator notes, a choice between two views, and a footer with the event
 * count and CLEAR. EVENTS shows the event log; SCENE a tree of the 3D
 * scene's nodes, whose selection the FOCUS readout names and the scene
 * brightens. The panel's body is a ScrollView, and the event log a
 * VirtualList nested in it: a wheel over the log scrolls the log until it
 * reaches an end, then the runtime passes the rest outward to the body.
 */
import { Children, Entity } from "@ipp/react";
import {
  Behavior,
  ScrollView,
  Skin,
  Text,
  VirtualList,
  type GuiVirtualRange,
} from "@ipp/react/gui";
import {
  Row as KitRow,
  Panel,
  PanelFooter,
  PanelHeader,
  SecondaryButton,
  SegmentedControl,
  Separator,
  TextLine,
  TreeView,
} from "@ipp/react/gui-kit";
import { useState } from "react";
import {
  BoxLayout,
  COLUMN,
  LEAF,
  PanelBody,
  ROW,
  TOKENS,
  TextLeaf,
  Tint,
} from "./presentation.js";
import type { GuiSceneState } from "./scene.js";
import {
  SCENE_TREE,
  SCENE_TREE_EXPANDED,
  sceneNodeLabel,
} from "./scene-tree.js";

/** Symbolic IDs of the panel's scrolling controls. */
export const TELEMETRY_ENTITY = "gui-telemetry";
export const EVENT_LOG_ENTITY = "gui-event-log";
export const TELEMETRY_VIEW_ENTITY = "gui-telemetry-view";
export const SCENE_TREE_ENTITY = "gui-scene-tree";

/** The TELEMETRY body's two views. */
export type TelemetryView = "events" | "scene";

/** The theme entity of scroll views drawn inside a panel: no frame. */
export const FRAMELESS_SCROLL_THEME = "gui-theme-frameless-scroll";

/** The panel's size in its column. */
export const TELEMETRY_WIDTH = 300;
export const TELEMETRY_HEIGHT = 640;

/** Body content width inside the frame's lines. */
const BODY_WIDTH = TELEMETRY_WIDTH - 2 * TOKENS.lineWidth;

/**
 * The column a vertical scroll bar takes at a scrolling view's right edge:
 * the bar between an inset on either side.
 */
const BAR_COLUMN = 3 * TOKENS.bar;

/** Readout rows: dense rows of body text. */
const READOUT_ROW = TOKENS.denseRow;
const READOUT_LABEL_WIDTH = 96;

/** Wrapped operator notes, in small neutral text. */
export const TELEMETRY_NOTES =
  "GAIN DRIVES THE PROJECTOR LIGHT AND THE WAVE AMPLITUDE. " +
  "SCAN HOLDS THE UPLINK. THE AMBER SHIELD GUARDS PURGE.";

/**
 * The event log's viewport height: fifteen dense rows, so that with the
 * readouts and notes above it the body overflows its viewport and scrolls.
 */
export const EVENT_LOG_HEIGHT = 15 * TOKENS.denseRow;

/** Event entries: small text inset from the log's frame. */
export const EVENT_TEXT_SIZE = TOKENS.textSmall;
const EVENT_TEXT_WIDTH =
  BODY_WIDTH - 2 * TOKENS.inset - TOKENS.inset - BAR_COLUMN;
/** An entry's margin above and below its text, and its least text box: a
 * one-line entry is a dense row. */
const EVENT_MARGIN = 4;
const EVENT_MIN_TEXT = TOKENS.inset;

/**
 * Item extent estimate: a two-line entry. Estimating the taller item means
 * measuring items only ever shortens the content, so the anchored offset
 * settles at the end of the log when its thumb is dragged there.
 */
const EVENT_ITEM_EXTENT = 38;

/** Event log items declared beyond each end of the visible ones. */
const EVENT_LOG_OVERSCAN = 2;

function readouts(scene: GuiSceneState) {
  const station = scene.station;
  const link =
    scene.callsign === ""
      ? "OFFLINE"
      : station.operation?.kind === "uplink" &&
          station.operation.phase === "running"
        ? `UPLINK ${Math.floor(station.operation.value * 100)}%`
        : scene.autoscan
          ? "HELD"
          : "READY";
  return [
    ["SIGNAL", `${Math.round(scene.gain * 100)}%`],
    ["OUTPUT", `${(2.1 + scene.gain * 1.7).toFixed(1)} KW`],
    ["SCAN", scene.autoscan ? "ACTIVE" : "STANDBY"],
    ["LINK", link],
    ["NODES", `${station.online}/${station.nodes.length} ONLINE`],
    ["FOCUS", sceneNodeLabel(scene.tuning.tuning.focus) ?? "NONE"],
  ] as const;
}

export function Telemetry({ scene }: { readonly scene: GuiSceneState }) {
  const rows = readouts(scene);
  const [view, setView] = useState<TelemetryView>("events");
  return (
    <Panel id="gui-telemetry-panel" layout={{ height: TELEMETRY_HEIGHT }}>
      <PanelBody id="gui-telemetry-content">
        <PanelHeader id="gui-telemetry-header" title="TELEMETRY" />
        <Entity id={TELEMETRY_ENTITY}>
          <BoxLayout kind={LEAF} width={BODY_WIDTH} flex={1} />
          <Skin theme={FRAMELESS_SCROLL_THEME} />
          <Behavior semantic_label="TELEMETRY" />
          <ScrollView />
          <Children>
            <Entity id="gui-telemetry-body">
              <BoxLayout
                kind={COLUMN}
                width={BODY_WIDTH}
                padding={[TOKENS.inset, BAR_COLUMN, TOKENS.inset, TOKENS.inset]}
              />
              <Children>
                <Entity id="gui-readouts">
                  <BoxLayout kind={ROW} height={rows.length * READOUT_ROW} />
                  <Children>
                    <Entity id="gui-readout-names">
                      <BoxLayout kind={COLUMN} width={READOUT_LABEL_WIDTH} />
                      <Children>
                        {rows.map(([name]) => (
                          <KitRow
                            key={name}
                            id={`gui-readout-${name.toLowerCase()}`}
                            height={READOUT_ROW}
                          >
                            <TextLine
                              id={`gui-readout-${name.toLowerCase()}-name`}
                              text={name}
                              tone="neutral"
                            />
                          </KitRow>
                        ))}
                      </Children>
                    </Entity>
                    <Separator
                      id="gui-readout-separator"
                      vertical
                      layout={{ margin_right: TOKENS.inset }}
                    />
                    <Entity id="gui-readout-values">
                      <BoxLayout kind={COLUMN} flex={1} />
                      <Children>
                        {rows.map(([name, value]) => (
                          <KitRow
                            key={name}
                            id={`gui-readout-${name.toLowerCase()}-row`}
                            height={READOUT_ROW}
                          >
                            <TextLine
                              id={`gui-readout-${name.toLowerCase()}-value`}
                              text={value}
                            />
                          </KitRow>
                        ))}
                      </Children>
                    </Entity>
                  </Children>
                </Entity>
                <TextLeaf
                  id="gui-notes"
                  text={TELEMETRY_NOTES}
                  font={scene.font.source}
                  size={TOKENS.textSmall}
                  color={TOKENS.neutral}
                  width={BODY_WIDTH - TOKENS.inset - BAR_COLUMN}
                  margin={[TOKENS.inset, 0, 0, 0]}
                />
                <SegmentedControl
                  id={TELEMETRY_VIEW_ENTITY}
                  options={[
                    { value: "events", label: "EVENTS" },
                    { value: "scene", label: "SCENE" },
                  ]}
                  defaultValue="events"
                  onChange={(value) => setView(value as TelemetryView)}
                  layout={{
                    margin_top: TOKENS.inset,
                    margin_bottom: TOKENS.inset / 2,
                  }}
                />
                {view === "events" ? (
                  <EventLog scene={scene} />
                ) : (
                  <SceneTree scene={scene} />
                )}
              </Children>
            </Entity>
          </Children>
        </Entity>
        <PanelFooter id="gui-telemetry-footer">
          <TextLine
            id="gui-event-count"
            text={`${scene.events.length} EVENTS`}
            layout={{ flex: 1 }}
          />
          <SecondaryButton
            id="gui-clear"
            label="CLEAR"
            disabled={scene.events.length <= 1}
            onPress={scene.clearLog}
          />
        </PanelFooter>
      </PanelBody>
    </Panel>
  );
}

/**
 * The scene's nodes in a tree as tall as the event log. Its selection is the
 * scene's focus; the operator's expansion stays while the view is shown.
 */
function SceneTree({ scene }: { readonly scene: GuiSceneState }) {
  const tuning = scene.tuning;
  const focus = tuning.tuning.focus;
  return (
    <TreeView
      id={SCENE_TREE_ENTITY}
      nodes={SCENE_TREE}
      defaultExpanded={SCENE_TREE_EXPANDED}
      {...(focus === undefined ? {} : { value: focus })}
      onChange={(key) => tuning.setFocus(key, sceneNodeLabel(key))}
      layout={{ height: EVENT_LOG_HEIGHT }}
    />
  );
}

/**
 * The event log: a VirtualList over the whole history, newest first, in the
 * default list look. The runtime scrolls it by the item count and estimate
 * and asks for the items it shows; React declares only those entries, and
 * each declared entry's measured extent replaces the estimate.
 */
function EventLog({ scene }: { readonly scene: GuiSceneState }) {
  return (
    <Entity id={EVENT_LOG_ENTITY}>
      <BoxLayout kind={LEAF} height={EVENT_LOG_HEIGHT} />
      <Behavior semantic_label="EVENT LOG" />
      <VirtualList
        item_count={scene.events.length}
        item_extent={EVENT_ITEM_EXTENT}
        overscan={EVENT_LOG_OVERSCAN}
        onRangeChange={(range: GuiVirtualRange) =>
          scene.setEventWindow({ first: range.first, last: range.last })
        }
        // Each item is a column holding its entry with a margin above and
        // below, so its measured extent places the next item after it.
        renderItem={(index) => (
          <>
            <BoxLayout kind={COLUMN} />
            <Children>
              <Entity id={`gui-event-${index}`}>
                <BoxLayout
                  kind={LEAF}
                  width={EVENT_TEXT_WIDTH}
                  minHeight={EVENT_MIN_TEXT}
                  margin={[EVENT_MARGIN, 0, EVENT_MARGIN, TOKENS.inset]}
                />
                <Tint color={index === 0 ? TOKENS.text : TOKENS.neutral} />
                <Text
                  text={scene.events[index] ?? ""}
                  source={scene.font.source}
                  font_size={EVENT_TEXT_SIZE}
                />
              </Entity>
            </Children>
          </>
        )}
      />
    </Entity>
  );
}
