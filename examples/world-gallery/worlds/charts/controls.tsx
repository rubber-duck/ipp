import type { GallerySceneMount } from "../../shared/scene.js";
import { CHART_CATALOG } from "./catalog.js";
import type { ChartMark } from "./scene.js";

function describe(mark: ChartMark | null | undefined) {
  if (!mark) return "None";
  const values = mark.values
    .map((value, index) =>
      value.valid && value.value.kind === "f32"
        ? `${mark.columns[index] ?? "value"} ${value.value.value.toFixed(2)}`
        : "",
    )
    .filter(Boolean)
    .join(", ");
  return `${CHART_CATALOG.find((chart) => chart.id === mark.chart)?.title ?? mark.chart} · series ${mark.series + 1} · row ${mark.rowId}${values ? ` · ${values}` : ""}`;
}

export function ChartControls({
  mount,
  report,
}: {
  readonly mount: GallerySceneMount | undefined;
  readonly report: (error: string) => void;
}) {
  const action = (name: string, args?: unknown) => {
    void mount
      ?.action(name, args)
      .catch((failure) =>
        report(failure instanceof Error ? failure.message : String(failure)),
      );
  };
  return (
    <>
      <div className="panel-heading">
        <span className="step">Charts around you</span>
        <h2>Turn toward a chart</h2>
        <p>
          Drag to turn in place at the center, or orbit after focusing a chart.
          Middle-drag to pan, scroll to move closer, and click a mark to select
          it.
        </p>
      </div>
      <fieldset disabled={!mount}>
        <legend>Camera · 2 second focus</legend>
        <button
          type="button"
          id="charts-focus-center"
          className="secondary-button"
          onClick={() => action("focus", "center")}
        >
          Return to center
        </button>
        <button
          type="button"
          id="charts-focus-overview"
          className="secondary-button"
          onClick={() => action("focus", "overview")}
        >
          View whole ring
        </button>
        <div className="chart-focus-grid">
          {CHART_CATALOG.map((chart) => (
            <button
              key={chart.id}
              id={`charts-focus-${chart.id}`}
              type="button"
              className="secondary-button"
              aria-pressed={mount?.options.focused === chart.id}
              onClick={() => action("focus", chart.id)}
            >
              {chart.title}
            </button>
          ))}
        </div>
      </fieldset>
      <fieldset disabled={!mount}>
        <legend>Dataset</legend>
        <label className="mesh-select" htmlFor="charts-data-source">
          <span>Source</span>
          <select
            id="charts-data-source"
            value={String(mount?.options.dataMode ?? "buffer")}
            onChange={(event) =>
              action("dataSource", event.currentTarget.value)
            }
          >
            <option value="buffer">Fixed samples</option>
            <option value="streaming">Live synthetic stream</option>
          </select>
        </label>
        <label className="mesh-select" htmlFor="charts-data-window">
          <span>Live history</span>
          <select
            id="charts-data-window"
            disabled={mount?.options.dataMode !== "streaming"}
            value={String(mount?.options.dataWindow ?? "count")}
            onChange={(event) =>
              action("dataWindow", event.currentTarget.value)
            }
          >
            <option value="count">Recent rows · 8 / 16</option>
            <option value="time">Recent time · 2 / 4 seconds</option>
          </select>
        </label>
        <button
          id="charts-stream-playback"
          type="button"
          className="secondary-button"
          disabled={mount?.options.dataMode !== "streaming"}
          aria-pressed={Boolean(mount?.options.streamPlaying)}
          onClick={() =>
            action("streamPlayback", !mount?.options.streamPlaying)
          }
        >
          {mount?.options.streamPlaying ? "Pause stream" : "Resume stream"}
        </button>
        <p>
          Lines share one stream with different history windows. Points keep
          recent marks; bars, grids and pies show the newest complete sample.
        </p>
        {mount?.options.streamError ? (
          <p role="alert">
            Stream stopped: {String(mount.options.streamError)}
          </p>
        ) : null}
        <label>
          <input
            id="charts-changed"
            disabled={mount?.options.dataMode === "streaming"}
            type="checkbox"
            checked={Boolean(mount?.options.changed)}
            onChange={(event) =>
              action("changeSamples", event.currentTarget.checked)
            }
          />{" "}
          Use changed samples
        </label>
      </fieldset>
      <fieldset disabled={!mount}>
        <legend>Data marks</legend>
        <p>
          Hover{" "}
          <output id="charts-hover">
            {describe(mount?.options.hover as ChartMark | null)}
          </output>
        </p>
        <p>
          Selected{" "}
          <output id="charts-selection">
            {describe(mount?.options.selection as ChartMark | null)}
          </output>
        </p>
        <button
          id="charts-clear-selection"
          type="button"
          className="secondary-button"
          onClick={() => action("clearSelection")}
        >
          Clear selection
        </button>
      </fieldset>
    </>
  );
}
