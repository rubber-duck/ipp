# React Plot authoring

[Plot declarations](declarations.ts) author the ordinary Plot components; [core Plot source](../../../../crates/ipp-core/src/world/systems/plot/component.rs) owns their semantics. Select `ipp.plot`, `ipp.data-bindings` and the appropriate Canvas or scene Systems when creating the World. Put one buffer or streaming binding, one frame and one chart on the same Entity. Frames are auxiliary presentation; Rust prepares all chart geometry.

Pass the generated module for the receiving Host as `contract`. The wrappers use its component descriptors and row encoder, so no client code supplies field offsets or packs a row table. Use `PlotLine2d`, `PlotBars2d`, `PlotPie2d`, `PlotGridBars3d`, `PlotHeightSurface3d`, `PlotPoints3d` or `PlotPie3d` with `PlotFrame2d` or `PlotFrame3d`. Line interpolation is `"straight"` or `"smooth"`.

```tsx
import * as contract from "./generated.js";
import {
  Entity, BufferDataSourceBinding, ColumnBindingAsset,
  PlotFrame2d, PlotLine2d, assetRef,
} from "@ipp/react";

// Definitions are encoded from this target's ExpressionBuilder.
<>
  <ColumnBindingAsset id="time" definition={timeDefinition} />
  <ColumnBindingAsset id="signal" definition={signalDefinition} />
  <Entity id="sensor-chart">
    <BufferDataSourceBinding source="datasets://sensor" columns={{
      x: { definition: assetRef("time") },
      y: { definition: assetRef("signal"), parameter: 1 },
    }} />
    <PlotFrame2d width={600} height={320} source={assetRef("font")}
      x_title="TIME / S" y_title="SIGNAL / %" />
    <PlotLine2d contract={contract} interpolation="smooth"
      series={{ nextSlot: 4, rows: new Map([
        [2, { name: "Sensor A", x: "x", y: "y" }],
      ]) }}
      labels={{ nextSlot: 1, rows: new Map([
        [0, { series: 2, row: 9007199254740993n,
          text: "Selected sample", highlighted: true, offset: [20, -40] }],
      ]) }} />
  </Entity>
</>
```

`RowsInput` retains caller-owned slot identities: removing slot 2 does not authorize reusing it, and array order does not renumber a series. Keep `nextSlot` monotonically increasing. Label `series` names the series slot; label `row` is the Data Service's positive `u64` source-row identity, supplied as `bigint`. The wrapper converts it directly to its canonical decimal string, including identities above JavaScript's safe integer range. A value edit preserves that identity. Labels refer to retained source rows, without copying source samples into React.

Omitting `series`, `labels` or a component prop preserves its last authored field value. Supply an empty table with its existing `nextSlot` to remove live rows. Replacing a table is a complete field write. Column parameter animation uses the existing `Animation` declarations and `AnimationHandle` playback/seek methods; Hosts still own their clocks.

## Value changes and axis placement

Import `fixed` from `@ipp/react` and set a column's `interpolation: fixed(25)` to move existing displayed values toward new projections at up to 25 units per Host second. The [binding declarations](../data/declarations.ts) apply the rate separately to each numeric lane, retarget from the displayed value and stop work when settled. Raw data remains unchanged, while chart geometry, picking and `bindingView` share the interpolated result. New rows and replacement sources or definitions initialize immediately; append-only stream rows never borrow the previous row's value. Set `interpolation: null` to return to immediate updates; omitting it preserves the authored setting.

Use `interpolation: percent(1)` for a speed of 1% of the fitted axis maximum per Host second. Plot resolves the output’s axis from series selectors and recalculates its reference from displayed values while motion is active, including automatic range changes. Signed ranges use the largest absolute endpoint; a range of −100 to 0 therefore gives 1 unit per second. If an output maps to multiple axes or has no Cartesian axis, supply an explicit nonnegative reference with `percent(1, 100)`; this also supports headless bindings. A zero reference holds the displayed value without accumulating elapsed time. Unsupported or ambiguous implicit references fail when movement requires them.

Frames can fit their numeric range with `automatic_x`, `automatic_y` and, in 3D, `automatic_z`. This changes the mapping inside the authored chart dimensions, not the physical box. Range fitting follows current displayed values and has no separate smoothing or hysteresis policy. Keep category axes fixed when their spacing carries meaning.

`PlotFrame3d` starts with `adaptive_axes={false}`. Enable it for an interacting chart to allow camera-aware boundary placement; disable it to freeze that view's current station, including halfway through a transition. Reenabling resumes from the displayed station without a return jump. The client owns interaction policy, independently of camera navigation.

## Color legends

`PlotLegend` is a reusable Canvas composition for categorical swatches and labels, or a numeric color scale. It reads only caller-authored colors and ranges. Share those inputs with series colors, color-column producers or column expressions so source updates and palette edits retain the same meaning; a legend does not infer categories, query datasets or observe chart state. Colors are straight linear RGBA in `0..1`, matching Canvas tint and Plot color columns.

```tsx
import {
  PlotLegend, plotLegendSize, plotLegendPlacement, plotColorScaleColor,
} from "@ipp/react";

const entries = [
  { id: "sensor-a", label: "Sensor A", color: [0.1, 0.7, 1, 1] as const },
  { id: "sensor-b", label: "Sensor B", color: [1, 0.3, 0.1, 1] as const },
];
const legend = { entries, title: "SENSORS", width: 180 };
const size = plotLegendSize(legend);
const placement = plotLegendPlacement({
  bounds: [0, 0, 600, 320], size, yDirection: "down",
});

// In the same Canvas World, alongside the Plot entity.
<PlotLegend id="sensor-legend" font={assetRef("font")} {...legend}
  x={placement.canvasPosition[0]} y={placement.canvasPosition[1]} />;

// An authored surface range; use the returned color in each source sample.
const scale = {
  min: 0, max: 100,
  colors: [[0.1, 0.2, 1, 1], [0.1, 1, 0.4, 1], [1, 0.2, 0.1, 1]] as const,
  format: (value: number) => `${value} °C`,
};
const sampleColor = plotColorScaleColor(scale, 25);
<PlotLegend id="temperature-legend" font={assetRef("font")}
  scale={scale} title="TEMPERATURE" />;
```

Series and category entries have stable caller IDs, so reordering or editing a label or color preserves their entities. An empty categorical key without a title declares nothing. `plotLegendSize` returns a positive extent of at least one row even for that empty key, including with zero padding; callers may omit its Surface entirely when empty. The explicit width bounds each label, using Canvas clipping rather than guessing font advances; increase the width for longer text. Text occupies one row and any overflow is clipped. Numeric scales require at least two colors, equally spaced over a finite increasing range; `plotColorScaleColor` clamps finite values to its endpoints and interpolates every RGBA channel. The legend presents that scale using 32 retained vertical strips, with the maximum label at the top and minimum label at the bottom, to the ramp's right. A title or endpoint formatter can state the units.

Placement uses ordered numeric bounds `[minX, minY, maxX, maxY]`, the legend size and gap in the same units. The default gap is one tenth of the legend width, so it scales with logical Canvas units or scene metres. Its default side is the opposite horizontal side of a bottom-left origin: right and vertically centered. `origin` also accepts bottom-right, top-left and top-right; `side` explicitly chooses left, right, top or bottom. Use `yDirection="down"` for Canvas placement and `"up"` for scene XY. The returned `bounds` and `center` stay in that coordinate space; `canvasPosition` names its top-left corner. Include the legend bounds when framing or focusing the chart.

For a 3D chart, derive the size in metres from the Canvas logical size and density, then call the helper with the chart's local XY bounds and `yDirection="up"`. Place an ordinary `FlatSurface` at the returned `center`, and author `PlotLegend` at `[0, 0]` in its attached `CanvasWorld`; the Surface maps +Y-down Canvas content to +Y-up scene XY. Parent or transform the chart and Surface together to retain the chart-local plane. A 2D legend already belongs to its chart's Canvas World, including when that Canvas is presented on a curved Surface; expand that Surface's content extent to include the legend.

The legend creates no Worlds, assets, subscriptions or animation controllers. Supply a font in the containing root's asset scope. Removing its declarations deletes their ordinary entities; unmount follows the root contract and preserves authored state. A caller-owned `CanvasWorld` retains its existing creation and removal rules.

The [chart showcase](../../../../examples/chart-showcase/README.md) uses these declarations with actual Data Service sources and native rendering. The maintained [React Plot scenario](../../../../tests/data/scenarios/react-plots.ts) supplies source, parameter-animation and completed-frame assertions for the combined Plot integration suite.
