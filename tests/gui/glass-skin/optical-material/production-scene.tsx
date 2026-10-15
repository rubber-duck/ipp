import { useMemo, useState, type ReactNode } from "react";
import { Asset, Children, Entity, assetRef } from "@ipp/react";
import {
  Behavior,
  Box,
  Button,
  Checkbox,
  Color,
  Font,
  Layout,
  Slider,
  Style,
  Text,
  TextInput,
  type GuiControlHandle,
} from "@ipp/react/gui";
import { DataGrid, Expander, type GuiKitContract } from "@ipp/react/gui-kit";
import {
  InlineAlert,
  Knob,
  MultiSelect,
  NumericStepper,
  Panel,
  PanelHeader,
  ProgressBar,
  RadioGroup,
  SearchableDropdown,
  SegmentedControl,
  StatusBadge,
  Tabs,
  TextLine,
  TreeView,
  ConfirmationDialog,
  CircularProgress,
} from "@ipp/react/gui-kit";
import { glassSkin } from "@ipp/react/gui-glass";
import { cybertronSkin } from "@ipp/react/gui-cybertron";
import { GuiKit } from "@ipp/react/gui-kit";

const options = [
  { key: "clear", label: "Clear glass" },
  { key: "frosted", label: "Frosted glass" },
  { key: "smoked", label: "Smoked glass" },
];
function At({
  id,
  x,
  y,
  width,
  height,
  children,
}: {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
  children: ReactNode;
}) {
  return (
    <Entity id={id}>
      <Layout
        kind={3}
        width={width}
        height={height}
        margin_left={x}
        margin_top={y}
        align_x={-1}
        align_y={-1}
      />
      <Font source={assetRef("font")} font_size={16} />
      <Behavior enabled />
      <Children>{children}</Children>
    </Entity>
  );
}
function Label({
  id,
  x,
  y,
  text,
  size = 16,
}: {
  id: string;
  x: number;
  y: number;
  text: string;
  size?: number;
}) {
  return (
    <Entity id={id}>
      <Style
        x={x}
        y={y}
        red={0.55}
        green={0.75}
        blue={0.8}
        alpha={1}
        layer={1}
      />
      <Text text={text} source={assetRef("font")} font_size={size} />
    </Entity>
  );
}

/** Actual public production components, presented by the same camera/light as the study. */
export function ProductionSamples({
  dropdownY = 178,
  skin = "glass",
  finish = "liquid",
  font,
  contract,
  controls,
}: {
  dropdownY?: number;
  skin?: "glass" | "cybertron";
  finish?: "optical" | "liquid";
  font: Uint8Array<ArrayBuffer>;
  contract: GuiKitContract;
  controls?: Map<string, GuiControlHandle>;
}) {
  const appearance = useMemo(
    () =>
      skin === "glass"
        ? glassSkin(contract, {
            appearance: finish,
            sceneLighting: true,
            reducedMotion: true,
          })
        : cybertronSkin(contract),
    [contract, skin, finish],
  );
  const [dialog, setDialog] = useState(false);
  const [selected, setSelected] = useState("frosted");
  const ref = (name: string) => (handle: GuiControlHandle | null) => {
    if (handle) controls?.set(name, handle);
    else controls?.delete(name);
  };
  return (
    <>
      <Asset id="font" kind={17} data={font} encode={(v) => v} />
      <Entity id="production-background">
        <Style red={0.004} green={0.012} blue={0.018} alpha={1} />
        <Box width={1536} height={1024} />
      </Entity>
      <GuiKit
        id="production-glass"
        contract={contract}
        font={assetRef("font")}
        fontSize={16}
        skin={appearance}
        reducedMotion
      >
        <Label
          id="production-title"
          x={64}
          y={45}
          text="GLASS / PRODUCTION COMPONENTS"
          size={32}
        />
        <Label
          id="production-subtitle"
          x={64}
          y={94}
          text="Native controls · continuous surfaces · view and light responsive"
        />
        <Label id="choices-title" x={64} y={154} text="01  CHOICES" />
        <Label id="dropdown-label" x={230} y={154} text="Searchable dropdown" />
        <At
          id="production-dropdown-cell"
          x={230}
          y={dropdownY}
          width={300}
          height={160}
        >
          <SearchableDropdown
            id="production-dropdown"
            options={options}
            defaultValue="clear"
            layout={{ width: 300, align_y: -1 }}
          />
        </At>
        <Label
          id="multiselect-label"
          x={625}
          y={154}
          text="Multiple selection"
        />
        <At
          id="production-multiselect-cell"
          x={625}
          y={178}
          width={320}
          height={32}
        >
          <MultiSelect
            id="production-multiselect"
            options={[
              { key: "design", label: "Design" },
              { key: "engineering", label: "Engineering" },
            ]}
            defaultValue={["design", "engineering"]}
            layout={{ width: 320, align_y: -1 }}
          />
        </At>
        <At
          id="production-segment-cell"
          x={625}
          y={250}
          width={320}
          height={32}
        >
          <SegmentedControl
            id="production-segment"
            options={[
              { value: "day", label: "Day" },
              { value: "week", label: "Week" },
              { value: "month", label: "Month" },
            ]}
            defaultValue="week"
          />
        </At>
        <Label id="tree-label" x={1050} y={154} text="Tree" />
        <At id="production-tree-cell" x={1050} y={178} width={360} height={132}>
          <TreeView
            id="production-tree"
            nodes={[
              {
                key: "workspace",
                label: "Workspace",
                children: [
                  { key: "materials", label: "Materials" },
                  { key: "components", label: "Components" },
                ],
              },
            ]}
            defaultExpanded={["workspace"]}
            defaultValue="materials"
            layout={{ width: 360, height: 132 }}
          />
        </At>
        <Label id="value-title" x={64} y={366} text="02  VALUES" />
        <Label id="stepper-label" x={230} y={366} text="Numeric stepper" />
        <At id="production-number-cell" x={230} y={390} width={184} height={90}>
          <NumericStepper
            id="production-number"
            min={0}
            max={100}
            step={1}
            defaultValue={24}
            ref={ref("number")}
          />
        </At>
        <At id="production-switch-cell" x={230} y={490} width={56} height={28}>
          <Entity id="production-switch">
            <Layout width={56} height={28} />
            <Checkbox skin="switch" checked ref={ref("switch")} />
          </Entity>
        </At>
        <At id="production-check-cell" x={318} y={490} width={28} height={28}>
          <Entity id="production-check">
            <Layout width={28} height={28} />
            <Checkbox checked ref={ref("checkbox")} />
          </Entity>
        </At>
        <At id="production-input-cell" x={230} y={548} width={300} height={32}>
          <Entity id="production-input">
            <Layout width={300} height={32} />
            <TextInput text="Project name" ref={ref("input")} />
          </Entity>
        </At>
        <At id="production-knob-cell" x={625} y={390} width={160} height={128}>
          <Knob
            id="production-knob"
            label="Exposure"
            min={0}
            max={1}
            step={0.01}
            defaultValue={0.75}
            bounds={false}
            ref={ref("dial")}
          />
        </At>
        <At id="production-slider-cell" x={625} y={548} width={280} height={28}>
          <Entity id="production-slider">
            <Layout width={280} height={28} />
            <Slider min={0} max={100} step={1} value={64} ref={ref("slider")} />
          </Entity>
        </At>
        <Label id="color-label" x={1050} y={366} text="Color field" />
        <At id="production-radio-cell" x={625} y={582} width={320} height={28}>
          <RadioGroup
            id="production-radio"
            horizontal
            defaultValue="clear"
            options={[
              { value: "clear", label: "Clear" },
              { value: "frosted", label: "Frosted" },
            ]}
          />
        </At>
        <At
          id="production-color-cell"
          x={1050}
          y={390}
          width={200}
          height={172}
        >
          <Entity id="production-color">
            <Layout width={200} height={172} />
            <Color
              hue={0.53}
              saturation={0.45}
              value={0.65}
              alpha={1}
              ref={ref("color")}
            />
          </Entity>
        </At>
        <Label id="feedback-title" x={64} y={626} text="03  FEEDBACK" />
        <At id="production-alert-cell" x={230} y={620} width={300} height={32}>
          <InlineAlert
            id="production-alert"
            severity="information"
            text="Changes saved"
          />
        </At>
        <At id="production-status-cell" x={625} y={624} width={100} height={24}>
          <StatusBadge id="production-status" status="active" label="Ready" />
        </At>
        <At id="production-paused-cell" x={749} y={624} width={100} height={24}>
          <StatusBadge id="production-paused" status="warning" label="Paused" />
        </At>
        <At
          id="production-progress-cell"
          x={1050}
          y={606}
          width={360}
          height={56}
        >
          <ProgressBar id="production-progress" label="Progress" value={0.64} />
        </At>
        <Label id="surface-title" x={64} y={716} text="04  SURFACES" />
        <At id="production-grid-cell" x={230} y={704} width={360} height={168}>
          <Panel
            id="production-grid-panel"
            padding={0}
            layout={{ width: 360, height: 168 }}
          >
            <PanelHeader title="Materials" />
            <DataGrid
              id="production-grid"
              columns={[
                { key: "name", title: "Name" },
                { key: "finish", title: "Finish" },
              ]}
              rows={options.map((o) => ({
                key: o.key,
                cells: { name: o.label, finish: o.key },
              }))}
              selected={selected}
              onRowPress={setSelected}
              layout={{ flex: 1 }}
            />
          </Panel>
        </At>
        <At id="production-tabs-cell" x={625} y={704} width={360} height={168}>
          <Tabs
            id="production-tabs"
            tabs={[
              {
                value: "overview",
                label: "Overview",
                content: <TextLine text="One continuous glass surface" />,
              },
              {
                value: "activity",
                label: "Activity",
                content: <TextLine text="Native state and interaction" />,
              },
              {
                value: "settings",
                label: "Settings",
                content: <TextLine text="Light and view responsive" />,
              },
            ]}
            defaultValue="overview"
            layout={{ width: 360, height: 168 }}
          />
        </At>
        <At
          id="production-expander-cell"
          x={1050}
          y={704}
          width={360}
          height={168}
        >
          <Expander
            id="production-expander"
            label="Surface settings"
            defaultExpanded
            layout={{ width: 360, height: 168 }}
          >
            <TextLine
              text="Update the selected surfaces."
              layout={{ margin_left: 12, margin_top: 16 }}
            />
            <Entity id="production-apply">
              <Layout
                width={160}
                height={32}
                margin_left={12}
                margin_top={16}
              />
              <Button
                ref={ref("button")}
                label="Apply material"
                onPress={() => setDialog(true)}
              />
            </Entity>
          </Expander>
        </At>
        <Label id="ring-title" x={64} y={932} text="05  PROGRESS" />
        <At id="production-ring-cell" x={230} y={904} width={180} height={104}>
          <CircularProgress
            id="production-ring"
            label="Uploading"
            value={0.62}
            size="small"
          />
        </At>
        <At
          id="production-complete-cell"
          x={625}
          y={904}
          width={180}
          height={104}
        >
          <CircularProgress
            id="production-complete"
            label="Saved"
            status="complete"
            size="small"
          />
        </At>
        <At
          id="production-error-cell"
          x={1050}
          y={904}
          width={180}
          height={104}
        >
          <CircularProgress
            id="production-error"
            label="Upload"
            value={0.62}
            status="failed"
            size="small"
          />
        </At>
        <ConfirmationDialog
          id="production-dialog"
          open={dialog}
          title="Apply material?"
          body="Update the selected surfaces."
          action="Apply"
          onConfirm={() => setDialog(false)}
          onCancel={() => setDialog(false)}
        />
      </GuiKit>
    </>
  );
}
