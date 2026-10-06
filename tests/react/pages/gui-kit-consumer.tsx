/**
 * A public consumer of `@ipp/react/gui-kit` for the React build: it resolves
 * the entry through the package exports, development sources or the built
 * distribution, and must stay free of browser composition.
 */
import type { GuiKitContract } from "@ipp/react/gui-kit";
import {
  Expander,
  GuiKit,
  InlineAlert,
  ProgressBar,
  StatusBadge,
} from "@ipp/react/gui-kit";
import { Entity, Children } from "@ipp/react";

export function kitPanel(contract: GuiKitContract, font: string) {
  return (
    <GuiKit contract={contract} font={font} fontSize={16}>
      <Entity id="panel">
        <Children>
          <InlineAlert id="alert" severity="warning" text="Connection lost." />
          <StatusBadge id="status" status="active" label="Online" />
          <ProgressBar id="progress" label="Uploading" value={0.5} />
          <Expander id="advanced" label="Advanced settings" />
        </Children>
      </Entity>
    </GuiKit>
  );
}
