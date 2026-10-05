import type { DatasetValue } from "@ipp/client";
import { plotColorScaleColor, type DataWindow } from "@ipp/react";
import { CHART_SCHEMA, chartData, type ChartSpec } from "./catalog.js";
import { CHART_HEIGHT_SCALE, chartCategoryColor } from "./colors.js";

export type ChartDataMode = "buffer" | "streaming";
export type ChartWindow = "count" | "time";
export const STREAM_SCHEMA = [
  ...CHART_SCHEMA,
  { name: "time", kind: "f32" },
] as const;
export const streamKey = (spec: ChartSpec) =>
  spec.component === "PlotLine2d" ? "lines" : spec.id;
const snapshotCount = (spec: ChartSpec) =>
  spec.id === "height-surface" ? 81 : spec.id === "grid-bars" ? 12 : 4;

export function streamWindows(
  spec: ChartSpec,
  profile: ChartWindow,
): readonly DataWindow[] {
  const rolling = spec.component === "PlotLine2d" || spec.id === "point-plot";
  const count = rolling
    ? profile === "time"
      ? 64
      : spec.id === "straight"
        ? 8
        : 16
    : snapshotCount(spec);
  return [
    { kind: "count", count: BigInt(count) },
    ...(profile === "time"
      ? [
          {
            kind: "range" as const,
            column: "time",
            width: spec.id === "straight" ? 2 : 4,
            anchor: { kind: "latest" as const },
          },
        ]
      : []),
  ];
}

/** Small elapsed timestamps keep raw f32 range windows precise; each append is bounded. */
export function streamRows(
  spec: ChartSpec,
  sequence: number,
  elapsed: number,
): DatasetValue[][] {
  const timed = (values: DatasetValue[], time = elapsed) => [
    ...values,
    { kind: "f32" as const, value: time },
  ];
  if (spec.component === "PlotLine2d")
    return Array.from({ length: 4 }, (_, i) => {
      const time = elapsed - (3 - i) * 0.125;
      return timed(
        chartData(
          time,
          50 + 30 * Math.sin(time * 2),
          0,
          50 + 25 * Math.cos(time * 1.3),
        ),
        time,
      );
    });
  if (spec.id === "point-plot")
    return Array.from({ length: 4 }, (_, i) => {
      const index = sequence * 4 + i;
      return timed(
        chartData(
          -0.25 + (index % 8) * 0.45,
          55 + 20 * Math.sin(index * 0.8),
          -0.25 + (Math.floor(index / 8) % 8) * 0.35,
          40 + 12 * Math.cos(index),
        ),
      );
    });
  if (spec.id === "height-surface")
    return Array.from({ length: 81 }, (_, i) => {
      const x = (Math.floor(i / 9) * 10) / 8,
        z = ((i % 9) * 10) / 8;
      const peak = 3 + Math.sin(elapsed) * 1.5;
      const height =
        0.3 +
        2.5 * Math.exp(-((x - peak) ** 2 + (z - 4) ** 2) / 5) +
        2 * Math.exp(-((x - 7.5) ** 2 + (z - 7.5) ** 2) / 4);
      return timed(
        chartData(
          x,
          height,
          z,
          height,
          1,
          1,
          0.5,
          plotColorScaleColor(CHART_HEIGHT_SCALE, height),
        ),
      );
    });
  return Array.from({ length: snapshotCount(spec) }, (_, i) => {
    if (spec.component.includes("Pie")) {
      const row = spec.rows[i]!;
      return timed(
        chartData(
          0,
          20 + 12 * Math.sin(elapsed * 1.8 + i),
          0,
          0,
          1,
          2.5 + 0.5 * Math.sin(elapsed + i),
          1.6 + 0.6 * Math.cos(elapsed + i),
          row[7]!.value as readonly number[],
        ),
      );
    }
    const flat = spec.component.endsWith("2d");
    return timed(
      chartData(
        flat ? i + 1 : i % 4,
        55 + 25 * Math.sin(elapsed * 2 + i),
        spec.id === "grid-bars" ? Math.floor(i / 4) : 0,
        0,
        1,
        1,
        0.5,
        chartCategoryColor(i),
      ),
    );
  });
}

/** A client ingestion timer, never a runtime clock: one acknowledged batch before the next. */
export class ChartFeed {
  private timer: ReturnType<typeof setTimeout> | undefined;
  private pending: Promise<void> | undefined;
  private epoch = 0;
  private generation = 0;
  sequence = 0;
  playing = false;
  error: string | null = null;
  status: "idle" | "running" | "paused" | "failed" = "idle";
  constructor(
    private readonly append: (
      sequence: number,
      elapsed: number,
    ) => Promise<void>,
    private readonly changed: () => void,
  ) {}
  get inFlight() {
    return this.pending !== undefined;
  }
  async prime() {
    await this.pause();
    this.epoch = performance.now() - 375;
    this.sequence = 0;
    this.error = null;
    this.status = "paused";
    await this.send();
  }
  private async send() {
    const pending = this.append(
      this.sequence,
      Math.max(0.375, (performance.now() - this.epoch) / 1000),
    );
    this.pending = pending;
    try {
      await pending;
      this.sequence++;
    } catch (error) {
      this.error = error instanceof Error ? error.message : String(error);
      this.playing = false;
      this.status = "failed";
      throw error;
    } finally {
      this.pending = undefined;
      this.changed();
    }
  }
  start() {
    if (this.playing || this.error) return;
    this.playing = true;
    this.status = "running";
    const generation = ++this.generation;
    const schedule = () => {
      if (!this.playing || generation !== this.generation) return;
      this.timer = setTimeout(() => {
        this.timer = undefined;
        void this.send().then(schedule, () => {});
      }, 500);
    };
    schedule();
    this.changed();
  }
  async pause(idle = false) {
    this.playing = false;
    ++this.generation;
    clearTimeout(this.timer);
    this.timer = undefined;
    await this.pending?.catch(() => {});
    if (!this.error) this.status = idle ? "idle" : "paused";
    this.changed();
  }
}
