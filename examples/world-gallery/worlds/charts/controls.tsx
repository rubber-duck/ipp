import type { GallerySceneMount } from "../../shared/scene.js";

/** Browser inspectors delegate all chart changes to the shared scene mount. */
export function ChartControls({
  mount,
  spatial,
  report,
}: {
  readonly mount: GallerySceneMount | undefined;
  readonly spatial: boolean;
  readonly report: (error: string) => void;
}) {
  const update = (patch: Readonly<Record<string, unknown>>) => {
    void mount
      ?.update(patch)
      .catch((failure: unknown) =>
        report(failure instanceof Error ? failure.message : String(failure)),
      );
  };
  return (
    <>
      <div className="panel-heading">
        <span className="step">Data plots</span>
        <h2>{spatial ? "Spatial charts" : "Chart sheet"}</h2>
        <p>Change the shared samples and inspect the runtime's plot output.</p>
      </div>
      <fieldset disabled={!mount}>
        <legend>Dataset</legend>
        <label>
          <input
            id="charts-changed"
            type="checkbox"
            checked={Boolean(mount?.options.changed)}
            onChange={(event) =>
              update({ changed: event.currentTarget.checked })
            }
          />{" "}
          Use changed samples
        </label>
      </fieldset>
      {spatial ? (
        <fieldset disabled={!mount}>
          <legend>View</legend>
          <label>
            <input
              id="charts-rotated"
              type="checkbox"
              checked={Boolean(mount?.options.rotated)}
              onChange={(event) =>
                update({ rotated: event.currentTarget.checked })
              }
            />{" "}
            Rotate cameras
          </label>
        </fieldset>
      ) : (
        <fieldset disabled={!mount}>
          <legend>Smooth series</legend>
          <label htmlFor="charts-parameter">
            Column parameter{" "}
            <output>{Number(mount?.options.parameter ?? 0).toFixed(2)}</output>
          </label>
          <input
            id="charts-parameter"
            type="range"
            min={0}
            max={1}
            step={0.01}
            value={Number(mount?.options.parameter ?? 0)}
            onChange={(event) =>
              update({ parameter: Number(event.currentTarget.value) })
            }
          />
        </fieldset>
      )}
    </>
  );
}
