/** Visible content and reserved height of each specimen in the renderer board. */
import { useState, type ReactNode } from "react";
import type { GuiPhysicalContext } from "@ipp/client";
import { Drawing, Layout, Style, type GuiControlHandle } from "@ipp/react/gui";
import { Entity, assetRef } from "@ipp/react";
import * as K from "@ipp/react/gui-kit";

import type { Family } from "./gui-cases.js";

export interface GuiSpecimen {
  id: string;
  family: Family;
  height: number;
  body: ReactNode;
}

const options = ["North", "East", "West"].map((label) => ({
  key: label,
  label,
}));
const choices = options.map(({ key, label }) => ({ value: key, label }));
const pages = choices.map((item) => ({
  ...item,
  content: <K.TextLine text={item.label + " station"} />,
}));
const sections = options.map((item) => ({
  ...item,
  content: <K.TextLine text={item.label + " station"} />,
}));
const photo = assetRef("board-image");
const noop = () => {};

type Entry = [id: string, height: number, body: ReactNode];
function family(family: Family, entries: Entry[]): GuiSpecimen[] {
  return entries.map(([id, height, body]) => ({ family, id, height, body }));
}

function ContextActions() {
  const menu = K.useContextMenu<string>();
  return (
    <>
      <K.Button label="Waypoint actions" onContextMenu={menu.opener("north")} />
      <K.ContextMenu id="context-actions" menu={menu} items={options} />
    </>
  );
}

export const controls = [
  ...family("paint", [
    [
      "curve-drawing",
      136,
      <Entity id="trace">
        <Layout width={202} height={120} clip />
        <Style x={0} y={60} scale_x={0.6} scale_y={0.6} />
        <Drawing source={assetRef("board-wave")} />
      </Entity>,
    ],
    [
      "glass-panel",
      136,
      <K.Panel width={202} height={130}>
        <K.TextLine text="Receiver status" />
      </K.Panel>,
    ],
  ]),
  ...family("actions", [
    ["button", 32, <K.Button label="Connect" />],
    ["secondary-button", 32, <K.SecondaryButton label="Disconnect" />],
    ["checkbox", 32, <K.Checkbox label="Record samples" defaultChecked />],
    ["switch", 32, <K.Switch label="Telemetry" defaultChecked />],
    ["toggle", 32, <K.Toggle label="Pin north" defaultPressed />],
    [
      "toggle-group",
      32,
      <K.ToggleGroup options={options} defaultValue={["East"]} />,
    ],
    [
      "radio-group",
      40,
      <K.RadioGroup options={choices} horizontal defaultValue="East" />,
    ],
    [
      "segmented-control",
      32,
      <K.SegmentedControl options={choices} defaultValue="East" />,
    ],
    [
      "icons",
      28,
      <K.Toolbar>
        <K.Icon icon="warning" />
        <K.Icon icon="sync" />
        <K.CheckMark />
      </K.Toolbar>,
    ],
  ]),
  ...family("editing", [
    [
      "text-field",
      48,
      <K.TextField label="Station" defaultValue="North terminal" />,
    ],
    [
      "textarea",
      64,
      <K.Textarea
        label="Notes"
        rows={2}
        defaultValue={"Samples received\nSignal stable"}
      />,
    ],
    ["password", 48, <K.PasswordInput label="Key" defaultValue="calibrate" />],
    ["otp", 64, <K.OtpInput label="Code" length={6} defaultValue="248163" />],
    [
      "token-input",
      52,
      <K.TokenInput
        label="Filter"
        defaultValue={[
          { kind: "text", text: "station = " },
          { kind: "token", id: "north", label: "North" },
        ]}
      />,
    ],
    ["file-input", 38, <K.FileInput label="Choose log file" />],
    [
      "form",
      90,
      <K.Form>
        <K.FormSection title="Receiver">
          <K.Field label="Channel">
            <K.TextField defaultValue="A-16" />
          </K.Field>
        </K.FormSection>
      </K.Form>,
    ],
    ["label", 32, <K.Label text="Operator" target={null} required />],
  ]),
  ...family("values", [
    [
      "slider",
      62,
      <K.LabelledSlider
        label="Output"
        min={0}
        max={100}
        defaultValue={64}
        scale={{ count: 5 }}
      />,
    ],
    [
      "range-slider",
      76,
      <K.RangeSlider
        label="Passband"
        min={0}
        max={100}
        defaultValue={[20, 75]}
      />,
    ],
    ["knob", 86, <K.Knob label="Gain" min={0} max={100} defaultValue={65} />],
    [
      "numeric-stepper",
      55,
      <K.NumericStepper label="Samples" min={1} max={64} defaultValue={16} />,
    ],
    [
      "color-picker",
      235,
      <K.ColorPicker
        label="Trace colour"
        alpha
        defaultValue={{ hue: 0.48, saturation: 0.8, value: 0.9, alpha: 1 }}
      />,
    ],
    [
      "rating",
      50,
      <K.Rating label="Quality" defaultValue={3.5} precision={0.5} />,
    ],
  ]),
  ...family("selection", [
    [
      "dropdown",
      46,
      <K.Dropdown label="Station" options={options} defaultValue="East" />,
    ],
    [
      "searchable-dropdown",
      44,
      <K.SearchableDropdown label="Search station" options={options} />,
    ],
    [
      "multi-select",
      42,
      <K.MultiSelect options={options} defaultValue={["East", "West"]} />,
    ],
    [
      "autocomplete",
      42,
      <K.Autocomplete suggestions={options} placeholder="Find station" />,
    ],
    [
      "combobox",
      42,
      <K.Combobox label="Route" options={options} defaultValue={["North"]} />,
    ],
    ["listbox", 90, <K.Listbox options={options} defaultValue={["West"]} />],
    [
      "tree",
      118,
      <K.TreeView
        nodes={[{ key: "sites", label: "Stations", children: options }]}
        defaultExpanded={["sites"]}
      />,
    ],
    ["pagination", 32, <K.Pagination totalItems={64} pageSize={8} />],
  ]),
  ...family("navigation", [
    ["tabs", 64, <K.Tabs tabs={pages} height={58} />],
    ["stepper", 68, <K.Stepper steps={pages} />],
    ["carousel", 94, <K.Carousel width={190} height={62} slides={sections} />],
    ["accordion", 92, <K.Accordion items={sections} />],
    [
      "collapsible",
      54,
      <K.Collapsible label="History" defaultExpanded>
        <K.TextLine text="08:42  Calibrated" />
      </K.Collapsible>,
    ],
    [
      "expander",
      54,
      <K.Expander label="Details" defaultExpanded>
        <K.TextLine text="Sampling at 128 Hz" />
      </K.Expander>,
    ],
    [
      "menubar",
      36,
      <K.Menubar
        menus={[
          { key: "file", label: "File", items: options },
          { key: "view", label: "View", items: options },
        ]}
      />,
    ],
    [
      "navigation-menu",
      36,
      <K.NavigationMenu
        sections={[
          {
            id: "route",
            label: "Stations",
            destinations: options.map((o) => ({ id: o.key, label: o.label })),
          },
        ]}
        onNavigate={noop}
      />,
    ],
  ]),
  ...family("containers", [
    [
      "panel",
      126,
      <K.Panel padding={0}>
        <K.PanelHeader title="System">
          <K.WindowControls
            onClose={noop}
            onMinimize={noop}
            onMaximize={noop}
          />
        </K.PanelHeader>
        <K.PanelBody>
          <K.TextLine text="All systems online" />
          <K.LabelledSeparator label="Network" />
        </K.PanelBody>
        <K.PanelFooter>
          <K.SecondaryButton label="Details" />
        </K.PanelFooter>
      </K.Panel>,
    ],
    [
      "scroll-area",
      64,
      <K.ScrollArea height={58}>
        <K.Column>
          {options.map((o) => (
            <K.TextLine key={o.key} text={o.label} />
          ))}
        </K.Column>
      </K.ScrollArea>,
    ],
    [
      "data-grid",
      108,
      <K.DataGrid
        columns={[
          { key: "site", title: "Site" },
          { key: "ms", title: "Latency" },
        ]}
        rowCount={256}
        row={(i) => ({
          key: String(i),
          cells: { site: "Node " + i, ms: String(11 + (i % 9)) + " ms" },
        })}
        height={100}
        selected="1"
      />,
    ],
    [
      "resizable",
      54,
      <K.Resizable width={188} height={44} panes={sections.slice(0, 2)} />,
    ],
    ["empty-state", 44, <K.EmptyState text="No pending alarms" />],
  ]),
  ...family("feedback", [
    ["progress", 44, <K.ProgressBar label="Transfer" value={0.68} />],
    [
      "circular-progress",
      76,
      <K.CircularProgress label="Charge" size="small" value={0.72} />,
    ],
    ["spinner", 36, <K.Spinner label="Scanning" />],
    ["meter", 45, <K.Meter label="Signal" min={0} max={100} value={72} />],
    ["badge", 32, <K.StatusBadge label="Connected" status="active" />],
    ["alert", 48, <K.InlineAlert severity="warning" text="Low reserve" />],
    ["toast", 58, <K.Toast persistent text="Samples saved" />],
    [
      "timeline",
      86,
      <K.Timeline
        items={[
          { key: "a", timestamp: "08:40", title: "Connected" },
          { key: "b", timestamp: "08:42", title: "Calibrated" },
        ]}
      />,
    ],
    [
      "waterfall",
      80,
      <K.Waterfall
        labelWidth={66}
        spans={[
          {
            key: "frame",
            label: "Frame",
            start: 0,
            duration: 12,
            children: [{ key: "draw", label: "Draw", start: 2, duration: 8 }],
          },
        ]}
      />,
    ],
  ]),
  ...family("dates-media", [
    ["calendar", 200, <K.Calendar defaultMonth="2024-05-01" />],
    ["date-input", 116, <K.DateInput height={108} defaultValue="2024-05-16" />],
    ["time-input", 140, <K.TimeInput height={134} defaultValue="08:42" />],
    ["date-picker", 128, <K.DatePicker defaultValue="2024-05-16" />],
    ["avatar", 52, <K.Avatar name="North" source={photo} size={40} />],
    [
      "image-cropper",
      122,
      <K.ImageCropper
        source={photo}
        imageWidth={16}
        imageHeight={16}
        previewWidth={120}
        previewHeight={80}
        defaultCrop={{ x: 2, y: 3, width: 9, height: 9 }}
      />,
    ],
  ]),
  ...family("overlays", [
    ["menu", 82, <K.Menu items={options} />],
    ["context-menu", 34, <ContextActions />],
    [
      "popover",
      34,
      <K.Popover label="Station info" title="North">
        <K.TextLine text="Online" />
      </K.Popover>,
    ],
    [
      "tooltip",
      34,
      <K.Button label="Signal help">
        <K.Tooltip text="Current signal" />
      </K.Button>,
    ],
    [
      "hover-card",
      34,
      <K.Button label="Station details">
        <K.HoverCard>
          <K.TextLine text="North station" />
        </K.HoverCard>
      </K.Button>,
    ],
    [
      "dialog",
      34,
      <K.Dialog.Root>
        <K.Dialog.Trigger label="Edit station" />
        <K.Dialog.Content>
          <K.Dialog.Header title="Station" />
          <K.Dialog.Body>
            <K.TextLine text="North terminal" />
          </K.Dialog.Body>
        </K.Dialog.Content>
      </K.Dialog.Root>,
    ],
  ]),
];

const candidates = () => options.map((o) => ({ ...o, insert: "@" + o.key }));
function MentionField({ input }: { input: GuiPhysicalContext | undefined }) {
  const [field, setField] = useState<GuiControlHandle | null>(null);
  return (
    <>
      <K.TextField ref={setField} label="Mention" />
      {input && (
        <K.TriggerMenu
          id="mentions"
          context={input}
          target={field?.target ?? null}
          triggers={["@"]}
          candidates={candidates}
        />
      )}
    </>
  );
}

export function modalBody(id: string, input?: GuiPhysicalContext) {
  switch (id) {
    case "trigger-menu-open":
      return <MentionField input={input} />;
    case "toast-stack":
      return (
        <K.ToastStack
          toasts={[
            { key: "log", text: "Samples saved", persistent: true },
            { key: "route", text: "Station connected", persistent: true },
          ]}
        />
      );
    case "context-menu-open":
      return <ContextActions />;
    case "popover-open":
      return (
        <K.Popover open label="Station info" title="North">
          <K.TextLine text="Station online" />
        </K.Popover>
      );
    case "tooltip-open":
      return (
        <K.Button label="Signal help">
          <K.Tooltip id="signal-tip" text="Current signal" />
        </K.Button>
      );
    case "hover-card-open":
      return (
        <K.Button label="Station details">
          <K.HoverCard id="station-card">
            <K.TextLine text="North terminal" />
          </K.HoverCard>
        </K.Button>
      );
    case "command-open":
      return <K.Command open items={options} onCommand={noop} />;
    case "confirmation-open":
      return (
        <K.ConfirmationDialog
          open
          title="Reset station?"
          body="Discard current samples."
          action="Reset"
          onConfirm={noop}
          onCancel={noop}
        />
      );
    case "drawer-open":
      return (
        <K.Drawer open label="Stations" edge="bottom" snapPoints={[120]}>
          <K.Listbox options={options} />
        </K.Drawer>
      );
    case "dialog-open":
      return (
        <K.Dialog.Root open>
          <K.Dialog.Content>
            <K.Dialog.Header title="Edit station" />
            <K.Dialog.Body>
              <K.TextField label="Station" defaultValue="North" />
            </K.Dialog.Body>
            <K.Dialog.Actions>
              <K.Dialog.Close label="Save" />
            </K.Dialog.Actions>
          </K.Dialog.Content>
        </K.Dialog.Root>
      );
    default:
      throw new Error("Unknown modal " + id);
  }
}
