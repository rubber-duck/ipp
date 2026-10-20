/** A client-managed terminal mount, started explicitly by the application. */
import { useState } from "react";
import { Entity } from "@ipp/react";
import { InputTarget, Layout } from "@ipp/react/gui";
import {
  Button,
  Column,
  TextLine,
  Toolbar,
  useGuiKit,
} from "@ipp/react/gui-kit";

export function TerminalScreen({
  width,
  height,
  start,
}: {
  width: number;
  height: number;
  start?: (() => Promise<void>) | undefined;
}) {
  const [state, setState] = useState("idle");
  const kit = useGuiKit();
  const toolbarHeight = kit.unit(
    Math.max(kit.tokens.controlHeight, kit.tokens.row),
  );
  // The terminal client observes authored dimensions to resize its character grid.
  const terminalHeight = Math.max(
    1,
    height - toolbarHeight - kit.spacing.label,
  );
  return (
    <Column width={width} height={height} fitHeight={false} gap="label">
      <Toolbar height={toolbarHeight}>
        <TextLine
          text={
            state === "idle"
              ? "BashKit shell · Ghostty terminal"
              : state === "running"
                ? "Click to type · Ctrl+Shift+Esc releases focus"
                : state
          }
          size="small"
          tone="neutral"
          flex={1}
        />
        <Button
          id="cockpit-terminal-start"
          label={
            state === "running"
              ? "Terminal running"
              : state === "starting"
                ? "Starting shell…"
                : "Start terminal"
          }
          disabled={!start || state === "starting" || state === "running"}
          onPress={() => {
            if (!start) return;
            setState("starting");
            void start().then(
              () => setState("running"),
              (error) => setState(String(error)),
            );
          }}
        />
      </Toolbar>
      <Entity id="cockpit-terminal">
        <Layout width={width} height={terminalHeight} clip />
        <InputTarget capture_tab capture_escape />
      </Entity>
    </Column>
  );
}
