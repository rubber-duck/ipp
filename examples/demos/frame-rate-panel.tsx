/** The FPS monitor is an ordinary streaming plot in the application Canvas. */
import { useMemo } from "react";
import {
  ColumnBindingAsset,
  Entity,
  PlotFrame2d,
  PlotLine2d,
  Resources,
  StreamingDataSourceBinding,
  assetRef,
  type PlotContract,
} from "@ipp/react";
import { Layout } from "@ipp/react/gui";
import {
  Panel,
  PanelBody,
  Column,
  TextLine,
  useGuiKit,
} from "@ipp/react/gui-kit";
import type * as Generated from "@ipp/host-contract";
import { FRAME_RATE_SAMPLES } from "./frame-rate.js";

export type FrameRateContract = PlotContract &
  Pick<typeof Generated, "ExpressionBuilder" | "encodeDataWindows">;

export function FrameRatePanel({
  source,
  contract,
  width,
}: {
  source: string;
  contract: FrameRateContract;
  width: number;
}) {
  const kit = useGuiKit();
  const chartWidth = width - 2 * kit.spacing.inset;
  return (
    <Panel id="demo-frame-rate-panel" width={width} fitHeight>
      <PanelBody gap="label">
        <Column gap="compact">
          <TextLine text="Frame rate" tone="accent" />
          <TextLine
            text="FPS · recent 60 seconds"
            size="small"
            tone="neutral"
          />
        </Column>
        <FrameRateChart
          source={source}
          contract={contract}
          width={chartWidth}
        />
      </PanelBody>
    </Panel>
  );
}

/** A reusable stream plot; the caller owns its surrounding section and lease. */
export function FrameRateChart({
  source,
  contract,
  width,
  height = 140,
  id = "demo-frame-rate-chart",
}: {
  source: string;
  contract: FrameRateContract;
  width: number;
  height?: number;
  id?: string;
}) {
  const kit = useGuiKit();
  const columns = useMemo(() => {
    const x = new contract.ExpressionBuilder();
    const y = new contract.ExpressionBuilder();
    return {
      x: x.encode(x.input("column:seconds", "f32")),
      y: y.encode(y.input("column:fps", "f32")),
    };
  }, [contract]);
  return (
    <Entity id={id}>
      <Resources>
        <ColumnBindingAsset id={`${id}-x`} definition={columns.x} />
        <ColumnBindingAsset id={`${id}-y`} definition={columns.y} />
      </Resources>
      <Layout width={width} height={height} />
      <StreamingDataSourceBinding
        source={source}
        encodeWindows={contract.encodeDataWindows}
        windows={[{ kind: "count", count: BigInt(FRAME_RATE_SAMPLES) }]}
        columns={{
          x: { definition: assetRef(`${id}-x`) },
          y: { definition: assetRef(`${id}-y`) },
        }}
      />
      <PlotFrame2d
        width={width}
        height={height}
        min_y={0}
        max_y={120}
        automatic_y={false}
        source={kit.font}
        font_size={kit.typeSize("small")}
        x_title="Seconds"
        y_title="FPS"
        ticks={3}
        padding_left={56}
        padding_right={24}
        padding_top={24}
        padding_bottom={30}
      />
      <PlotLine2d
        contract={contract}
        interpolation="straight"
        // Dense history is a trace: per-sample dots obscure it and add 600 draws.
        marker_size={0}
        series={{
          nextSlot: 1,
          rows: new Map([
            [
              0,
              {
                name: "FPS",
                x: "x",
                y: "y",
                color: [0.55, 0.84, 0.96, 1],
              },
            ],
          ]),
        }}
      />
    </Entity>
  );
}
