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
import type {
  GalleryDiagnosticsMonitor,
  GallerySystemSnapshot,
} from "../../frame-rate.js";

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
        <Metric
          label="FRAME RATE"
          monitor={monitor}
          select={(sample) => number(sample.fps, 1)}
          unit="fps"
        />
        <Metric
          label="FRAME INTERVAL"
          monitor={monitor}
          select={(sample) => number(sample.frameMs, 1)}
          unit="ms"
        />
        <Metric
          label="DRAW CALLS"
          monitor={monitor}
          select={(sample) => number(sample.drawCalls)}
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
          <Readout
            label="Triangles"
            monitor={monitor}
            select={(sample) => number(sample.triangles)}
          />
          <Readout
            label="Failed draws"
            monitor={monitor}
            select={(sample) => number(sample.failedDrawCalls)}
          />
          <Readout
            label="Frame upload"
            monitor={monitor}
            select={(sample) => bytes(sample.renderer?.frame.uploadedBytes)}
          />
          <Readout
            label="Layout reflows"
            monitor={monitor}
            select={(sample) => number(sample.renderer?.gui.guiLayoutReflows)}
          />
        </FormSection>
        <FormSection
          title="Client platform"
          description="This client, not the remote Host"
          flex={1}
          fieldGap={kit.spacing.label}
        >
          <Readout
            label="Runtime"
            monitor={monitor}
            select={(sample) => sample.platform.runtime}
          />
          <Readout
            label="Logical cores"
            monitor={monitor}
            select={(sample) => number(sample.platform.logicalCores)}
          />
          <Readout
            label="JS heap"
            monitor={monitor}
            select={(sample) => bytes(sample.platform.jsHeapUsedBytes)}
          />
          <Readout
            label="Output"
            monitor={monitor}
            select={(sample) =>
              sample.viewport
                ? `${sample.viewport.width} × ${sample.viewport.height}`
                : "Pending"
            }
          />
        </FormSection>
      </Row>
      <SampleText
        monitor={monitor}
        select={(sample) =>
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
  monitor,
  select,
  unit,
}: {
  label: string;
  monitor: GalleryDiagnosticsMonitor;
  select: (sample: GallerySystemSnapshot) => string;
  unit: string;
}) {
  const kit = useGuiKit();
  return (
    <Column flex={1} gap={kit.spacing.label}>
      <TextLine text={label} size="small" tone="neutral" />
      <Row minHeight={0} gap={kit.spacing.label}>
        <SampleText
          monitor={monitor}
          select={select}
          size="display"
          layout={{ width: 110 }}
        />
        <TextLine text={unit} size="small" tone="neutral" />
      </Row>
    </Column>
  );
}
function Readout({
  label,
  monitor,
  select,
}: {
  label: string;
  monitor: GalleryDiagnosticsMonitor;
  select: (sample: GallerySystemSnapshot) => string;
}) {
  return (
    <Row minHeight={24} gap={8}>
      <TextLine text={label} size="small" tone="neutral" layout={{ flex: 1 }} />
      <SampleText
        monitor={monitor}
        select={select}
        size="small"
        layout={{ width: 140 }}
      />
    </Row>
  );
}

/** Subscribe at the changing text leaf; unchanged formatted values publish nothing to React. */
function SampleText({
  monitor,
  select,
  ...props
}: Omit<Parameters<typeof TextLine>[0], "text"> & {
  monitor: GalleryDiagnosticsMonitor;
  select: (sample: GallerySystemSnapshot) => string;
}) {
  const text = useSyncExternalStore(monitor.subscribe, () =>
    select(monitor.getSnapshot()),
  );
  return <TextLine {...props} text={text} />;
}
