/** Pair within repetition so drift between sweeps does not become a removal cost. */
type Backend = "metal" | "gles" | "webgpu";
interface GuiResult {
  readonly name: string;
  readonly theme: string;
  readonly projection: string;
  readonly timing: { meanFrameMs: number } | null;
  readonly gpuFrameMs?: { p50?: number; count: number; recorded: number };
  readonly dynamic?: {
    rowsPerUpdate: number;
    updateToFrameMs: { p50: number; p95: number };
    commitMs: { p50: number };
  };
  readonly scroll?: { p50: number; p95: number };
}
export interface GuiComparisonRun {
  readonly backend: Backend;
  readonly repeat: number;
  readonly result: { runs: readonly GuiResult[] };
}

export function compareGuiRuns(
  runs: readonly GuiComparisonRun[],
  instrumented: boolean,
) {
  const selections = new Map(
    runs.flatMap((run) =>
      run.result.runs.map(
        (r) => [[r.theme, r.projection, r.name].join("/"), r] as const,
      ),
    ),
  );
  const metric = (r: GuiResult | undefined) =>
    instrumented ? r?.gpuFrameMs?.p50 : r?.timing?.meanFrameMs;
  const median = (values: number[]) => {
    if (!values.length) return null;
    const sorted = values.toSorted((a, b) => a - b);
    const middle = Math.floor(sorted.length / 2);
    return sorted.length % 2
      ? sorted[middle]!
      : (sorted[middle - 1]! + sorted[middle]!) / 2;
  };
  const hasWebGpu = runs.some((r) => r.backend === "webgpu");
  const cases = [...selections.values()];
  const withUpdates = !instrumented && cases.some((r) => r.dynamic);
  const withScroll = !instrumented && cases.some((r) => r.scroll);
  const rows = cases.map(({ name, theme, projection, dynamic }) => {
    const times = {
      metal: [] as number[],
      gles: [] as number[],
      webgpu: [] as number[],
    };
    const savings = {
      metal: [] as number[],
      gles: [] as number[],
      webgpu: [] as number[],
    };
    const available = { metal: 0, gles: 0, webgpu: 0 };
    const recorded = { metal: 0, gles: 0, webgpu: 0 };
    const edits: Record<Backend, NonNullable<GuiResult["dynamic"]>[]> = {
      metal: [],
      gles: [],
      webgpu: [],
    };
    const wheels: Record<Backend, NonNullable<GuiResult["scroll"]>[]> = {
      metal: [],
      gles: [],
      webgpu: [],
    };
    for (const run of runs) {
      const same = run.result.runs.filter(
        (r) => r.theme === theme && r.projection === projection,
      );
      const selected = same.find((r) => r.name === name);
      if (selected?.dynamic) edits[run.backend].push(selected.dynamic);
      if (selected?.scroll) wheels[run.backend].push(selected.scroll);
      if (selected?.gpuFrameMs) {
        available[run.backend] += selected.gpuFrameMs.count;
        recorded[run.backend] += selected.gpuFrameMs.recorded;
      }
      const ms = metric(selected);
      if (ms === undefined) continue;
      times[run.backend].push(ms);
      const all = metric(
        runs
          .filter(
            (peer) =>
              peer.backend === run.backend && peer.repeat === run.repeat,
          )
          .flatMap((peer) => peer.result.runs)
          .find(
            (r) =>
              r.name === "all" &&
              r.theme === theme &&
              r.projection === projection,
          ),
      );
      if (name.startsWith("without-") && all !== undefined)
        savings[run.backend].push(all - ms);
    }
    return [
      theme,
      projection,
      name,
      median(times.metal),
      median(times.gles),
      median(savings.metal),
      median(savings.gles),
      times.metal.length,
      times.gles.length,
      ...(hasWebGpu
        ? [median(times.webgpu), median(savings.webgpu), times.webgpu.length]
        : []),
      ...(instrumented
        ? [available.metal, recorded.metal, available.gles, recorded.gles]
        : []),
      ...(withUpdates
        ? [
            dynamic?.rowsPerUpdate,
            ...[
              edits.metal,
              edits.gles,
              ...(hasWebGpu ? [edits.webgpu] : []),
            ].flatMap((samples) => [
              median(samples.map((r) => r.updateToFrameMs.p50)),
              median(samples.map((r) => r.updateToFrameMs.p95)),
              median(samples.map((r) => r.commitMs.p50)),
            ]),
          ]
        : []),
      ...(withScroll
        ? [
            wheels.metal,
            wheels.gles,
            ...(hasWebGpu ? [wheels.webgpu] : []),
          ].flatMap((samples) => [
            median(samples.map((r) => r.p50)),
            median(samples.map((r) => r.p95)),
          ])
        : []),
    ];
  });
  const metricName = instrumented ? "gpu_frame_p50_ms" : "mean_submission_ms";
  return (
    [
      [
        "theme",
        "projection",
        "case",
        "metal_" + metricName,
        "gles_" + metricName,
        "metal_saved_ms",
        "gles_saved_ms",
        "metal_repeats",
        "gles_repeats",
        ...(hasWebGpu
          ? ["webgpu_" + metricName, "webgpu_saved_ms", "webgpu_repeats"]
          : []),
        ...(instrumented
          ? [
              "metal_gpu_samples",
              "metal_gpu_frame_scopes",
              "gles_gpu_samples",
              "gles_gpu_frame_scopes",
            ]
          : []),
        ...(withUpdates
          ? [
              "rows_per_update",
              ...["metal", "gles", ...(hasWebGpu ? ["webgpu"] : [])].flatMap(
                (backend) => [
                  backend + "_update_to_frame_p50_ms",
                  backend + "_update_to_frame_p95_ms",
                  backend + "_update_commit_p50_ms",
                ],
              ),
            ]
          : []),
        ...(withScroll
          ? [
              "metal_scroll_to_frame_p50_ms",
              "metal_scroll_to_frame_p95_ms",
              "gles_scroll_to_frame_p50_ms",
              "gles_scroll_to_frame_p95_ms",
              ...(hasWebGpu
                ? [
                    "webgpu_scroll_to_frame_p50_ms",
                    "webgpu_scroll_to_frame_p95_ms",
                  ]
                : []),
            ]
          : []),
      ],
      ...rows,
    ]
      .map((row) => row.join(","))
      .join("\n") + "\n"
  );
}
