import { useSyncExternalStore } from "react";
import {
  Column,
  FormSection,
  Row,
  Separator,
  TextLine,
  useGuiKit,
} from "@ipp/react/gui-kit";
import {
  FrameRateChart,
  type FrameRateContract,
} from "../../frame-rate-panel.js";
import type { GalleryDiagnosticsMonitor } from "../../frame-rate.js";

const number = (value: number | null | undefined, digits = 0) =>
  value == null ? "—" : value.toFixed(digits);
const bytes = (value: number | null | undefined) =>
  value == null ? "Not available" : `${(value / 1048576).toFixed(1)} MiB`;
export function SystemScreen({
  monitor,
  contract,
  width,
}: {
  monitor?: GalleryDiagnosticsMonitor | undefined;
  contract: FrameRateContract;
  width: number;
}) {
  return monitor ? (
    <LiveSystemScreen monitor={monitor} contract={contract} width={width} />
  ) : (
    <Column>
      <TextLine text="System monitor" size="display" />
      <TextLine text="Waiting for presentation diagnostics…" tone="neutral" />
    </Column>
  );
}
function LiveSystemScreen({
  monitor,
  contract,
  width,
}: {
  monitor: GalleryDiagnosticsMonitor;
  contract: FrameRateContract;
  width: number;
}) {
  const kit = useGuiKit();
  const sample = useSyncExternalStore(monitor.subscribe, monitor.getSnapshot);
  const renderer = sample.renderer;
  return (
    <Column gap={kit.spacing.section}>
      <Column gap={kit.spacing.label}>
        <TextLine text="The engine, at a glance" size="display" />
        <TextLine
          text="Live presentation history · updated every 100 ms"
          tone="neutral"
          size="small"
        />
      </Column>
      <Row gap={kit.spacing.section} minHeight={0}>
        <Metric label="FRAME RATE" value={number(sample.fps, 1)} unit="fps" />
        <Metric
          label="FRAME INTERVAL"
          value={number(sample.frameMs, 1)}
          unit="ms"
        />
        <Metric
          label="DRAW CALLS"
          value={number(sample.drawCalls)}
          unit="latest frame"
        />
      </Row>
      <FrameRateChart
        id="cockpit-fps-chart"
        source={monitor.source}
        contract={contract}
        width={width}
        height={180}
      />
      <Separator />
      <Row gap={kit.spacing.section * 1.5} alignItems="start" minHeight={0}>
        <FormSection
          title="IPP renderer"
          description="Host-wide counters"
          flex={1}
          fieldGap={kit.spacing.label}
        >
          <Readout label="Triangles" value={number(sample.triangles)} />
          <Readout
            label="Failed draws"
            value={number(sample.failedDrawCalls)}
          />
          <Readout
            label="Frame upload"
            value={bytes(renderer?.frame.uploadedBytes)}
          />
          <Readout
            label="Layout reflows"
            value={number(renderer?.gui.guiLayoutReflows)}
          />
        </FormSection>
        <FormSection
          title="Client platform"
          description="This client, not the remote Host"
          flex={1}
          fieldGap={kit.spacing.label}
        >
          <Readout label="Runtime" value={sample.platform.runtime} />
          <Readout
            label="Logical cores"
            value={number(sample.platform.logicalCores)}
          />
          <Readout
            label="JS heap"
            value={bytes(sample.platform.jsHeapUsedBytes)}
          />
          <Readout
            label="Output"
            value={
              sample.viewport
                ? `${sample.viewport.width} × ${sample.viewport.height}`
                : "Pending"
            }
          />
        </FormSection>
      </Row>
      <TextLine
        text={
          sample.rendererStatus === "unavailable"
            ? "Detailed renderer counters are unavailable on this connection."
            : "FPS measures completed presentations; it is not GPU execution time."
        }
        tone="neutral"
        size="small"
      />
    </Column>
  );
}
function Metric({
  label,
  value,
  unit,
}: {
  label: string;
  value: string;
  unit: string;
}) {
  const kit = useGuiKit();
  return (
    <Column flex={1} gap={kit.spacing.label}>
      <TextLine text={label} size="small" tone="neutral" />
      <Row minHeight={0} gap={kit.spacing.label}>
        <TextLine text={value} size="display" />
        <TextLine text={unit} size="small" tone="neutral" />
      </Row>
    </Column>
  );
}
function Readout({ label, value }: { label: string; value: string }) {
  return (
    <Row minHeight={24} gap={8}>
      <TextLine text={label} size="small" tone="neutral" layout={{ flex: 1 }} />
      <TextLine text={value} size="small" />
    </Row>
  );
}
