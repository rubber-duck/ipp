import { Resources } from "@ipp/react";
import { useRef, useState } from "react";
import {
  Button,
  CircularProgress,
  Column,
  ConfirmationDialog,
  ContextMenu,
  Dialog,
  Field,
  Floating,
  InlineAlert,
  LabelledSlider,
  Menu,
  Popover,
  ProgressBar,
  Row,
  Spinner,
  StatusBadge,
  TextField,
  TextLine,
  ToastStack,
  Tooltip,
  useContextMenu,
  useOverlayOpen,
  type ToastItem,
} from "@ipp/react/gui-kit";

export function Feedback({
  wide,
  maxToastWidth,
}: {
  wide: boolean;
  maxToastWidth: number;
}) {
  const [progress, change] = useState(65);
  const [alert, setAlert] = useState(true);
  const [toasts, show] = useState<ToastItem[]>([]);
  const nextToast = useRef(0);
  const completed = progress === 100;
  return (
    <Column gap="field">
      <Row gap="field" alignItems="center">
        <CircularProgress
          id="progress-ring"
          label="Explored"
          size={wide ? "large" : "small"}
          value={progress / 100}
          {...(completed && { status: "complete" as const })}
        />
        <Column flex={1} gap="label">
          <ProgressBar
            id="progress"
            label="Distance travelled"
            value={progress / 100}
            {...(completed && { status: "complete" as const })}
          />
          <StatusBadge
            status={completed ? "active" : "busy"}
            label={completed ? "Finished" : "On trail"}
          />
          {!completed && <Spinner id="scouting" label="Scouting ahead" />}
        </Column>
      </Row>
      <LabelledSlider
        id="completion"
        label="Route completion"
        min={0}
        max={100}
        step={1}
        units="%"
        value={progress}
        onChange={change}
      />
      {alert && (
        <InlineAlert
          id="trail-warning"
          severity="warning"
          text="Trail closed after dark."
          action={{ label: "Dismiss", onPress: () => setAlert(false) }}
        />
      )}
      <Row gap="label">
        <Button
          id="show-alert"
          label="Show alert"
          variant="secondary"
          onPress={() => setAlert(true)}
        />
        <Button
          id="show-toast"
          label="Show toast"
          onPress={() =>
            show([
              {
                key: `saved-${++nextToast.current}`,
                text: "Checkpoint saved.",
                persistent: true,
              },
            ])
          }
        />
      </Row>
      <Resources>
        <ToastStack
          id="trail-toasts"
          maxWidth={maxToastWidth}
          toasts={toasts}
          onDismiss={() => show([])}
        />
      </Resources>
    </Column>
  );
}

export function Overlays() {
  const commands = useOverlayOpen({});
  const context = useContextMenu<string>();
  const [confirm, setConfirm] = useState(false);
  const [message, report] = useState("Campfire");
  const [name, setName] = useState("Campfire");
  const items = [
    { key: "mark", label: "Mark waypoint" },
    { key: "share", label: "Show coordinates" },
  ] as const;
  const act = (key: string) =>
    report(key === "mark" ? "Waypoint marked" : "Coordinates: 18, 42");
  return (
    <Column gap="field">
      <Field label="Command menu">
        <Button
          id="commands"
          variant="secondary"
          fitWidth
          label="Waypoint actions"
          onPress={() => commands.setOpen(!commands.open)}
        >
          <Floating
            id="command-popup"
            mode="light"
            open={commands.open}
            controlled={commands.controlled}
            onVisibleChange={commands.onVisibleChange}
          >
            <Menu
              id="command-list"
              items={items}
              overlay={commands}
              onSelect={act}
            />
          </Floating>
        </Button>
      </Field>
      <Row gap="label">
        <Popover
          id="map-info"
          label="Popover"
          title="Weather"
          contentWidth={220}
        >
          <TextLine text="Clear skies · 18°C" />
        </Popover>
        <Button id="hint" label="Tooltip" variant="secondary">
          <Tooltip text="Waypoints mark your route." />
        </Button>
      </Row>
      <Field label="Context menu">
        <Button
          id="context-target"
          label="Right-click this waypoint"
          fitWidth
          variant="secondary"
          onContextMenu={context.opener("camp")}
        />
      </Field>
      <Resources>
        <ContextMenu
          id="waypoint-context"
          menu={context}
          items={items}
          onSelect={act}
        />
      </Resources>
      <Dialog.Root id="rename-dialog">
        <Dialog.Trigger
          id="rename-trigger"
          label="Rename checkpoint"
          fitWidth
        />
        <Dialog.Content width={284}>
          <Dialog.Header title="Rename checkpoint" />
          <Dialog.Body>
            <TextField label="Name" value={name} onChange={setName} />
          </Dialog.Body>
          <Dialog.Actions>
            <Dialog.Close label="Save" onPress={() => report(name)} />
          </Dialog.Actions>
        </Dialog.Content>
      </Dialog.Root>
      <Button
        id="reset-route"
        variant="destructive"
        label="Reset checkpoint"
        fitWidth
        onPress={() => setConfirm(true)}
      />
      <Resources>
        <ConfirmationDialog
          id="reset-confirmation"
          open={confirm}
          title="Reset checkpoint?"
          body="Return the waypoint to camp."
          action="Reset"
          destructive
          onCancel={() => setConfirm(false)}
          onConfirm={() => {
            setConfirm(false);
            setName("Campfire");
            report("Checkpoint reset");
          }}
        />
      </Resources>
      <TextLine text={message} />
    </Column>
  );
}
