# React Plot authoring

[Plot declarations](plots.ts) author the ordinary Plot components; [core Plot source](../../../crates/ipp-core/src/world/systems/plot/components.rs) owns their semantics. Select `ipp.plot`, `ipp.data-bindings` and the appropriate Canvas or scene Systems when creating the World. Put one buffer or streaming binding, one frame and one chart on the same Entity. Frames are auxiliary presentation; Rust prepares all chart geometry.

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

The [chart showcase](../../../examples/chart-showcase/README.md) uses these declarations with actual Data Service sources and native rendering. The maintained [React Plot scenario](../../../tests/integration/scenarios/react-plots.ts) supplies source, parameter-animation and completed-frame assertions for the combined Plot integration suite.
