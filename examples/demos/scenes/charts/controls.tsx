import { TextLine } from "@ipp/react/gui-kit";
import {
  SceneChoice,
  SceneButtons,
  sceneActions,
} from "../../shared/control-panel.js";
import type { GallerySceneControlsProps } from "../../shared/scene.js";
import { CHART_CATALOG } from "./catalog.js";
export function ChartControls(props: GallerySceneControlsProps) {
  const { action } = sceneActions(props),
    state = props.mount.options;
  return (
    <>
      <SceneChoice
        id="charts-focus"
        label="Exhibit"
        value={String(state.focused ?? "center")}
        choices={[
          { key: "center", label: "Center of the ring" },
          { key: "overview", label: "Whole ring" },
          ...CHART_CATALOG.map((chart) => ({
            key: chart.id,
            label: chart.title,
          })),
        ]}
        change={(id) => action("focus", id)}
      />
      <SceneChoice
        id="charts-source"
        label="Data"
        value={String(state.dataMode)}
        choices={[
          { key: "buffer", label: "Fixed samples" },
          { key: "streaming", label: "Live stream" },
        ]}
        change={(source) => action("dataSource", source)}
      />
      {state.dataMode === "streaming" && (
        <>
          <SceneChoice
            id="charts-history"
            label="History"
            value={String(state.dataWindow)}
            choices={[
              { key: "count", label: "Recent samples" },
              { key: "time", label: "Recent seconds" },
            ]}
            change={(window) => action("dataWindow", window)}
          />
          <SceneButtons
            buttons={[
              {
                id: "charts-stream-playback",
                label: state.streamPlaying ? "Pause stream" : "Resume stream",
                press: () => action("streamPlayback", !state.streamPlaying),
              },
            ]}
          />
        </>
      )}
      {state.dataMode === "buffer" && (
        <SceneButtons
          buttons={[
            {
              id: "charts-change-data",
              label: state.changed ? "Original sample" : "New sample",
              press: () => action("changeSamples", !state.changed),
            },
          ]}
        />
      )}
      <TextLine
        id="charts-selected-mark"
        text={
          state.selection
            ? `Selected ${(state.selection as { chart: string }).chart}`
            : "Click a data mark to inspect it"
        }
        size="small"
        tone="neutral"
      />
      {state.selection && (
        <SceneButtons
          buttons={[
            {
              id: "charts-clear-selection",
              label: "Clear selection",
              press: () => action("clearSelection"),
            },
          ]}
        />
      )}
    </>
  );
}
