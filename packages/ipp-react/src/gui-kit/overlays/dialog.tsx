import { useEntityId } from "../../scope/identity.js";
import { Resources } from "../../resources.js";
import { createContext, useContext, type ReactNode } from "react";
import { Button, type ButtonProps } from "../button.js";
import { Column, Row, type GuiKitLayout } from "../layout.js";
import { PanelHeader } from "../containers/panel.js";
import { useGuiKit } from "../kit.js";
import {
  Floating,
  useOverlayOpen,
  type OverlayOpenProps,
  type OverlayOpen,
} from "./overlay.js";

interface DialogScope {
  readonly id: string;
  readonly overlay: OverlayOpen;
}

const DialogContext = createContext<DialogScope | null>(null);

function useDialog() {
  const scope = useContext(DialogContext);
  if (!scope) throw new Error("Dialog parts need an enclosing Dialog.Root");
  return scope;
}

export interface DialogRootProps extends OverlayOpenProps {
  readonly id?: string;
  readonly children?: ReactNode;
}

export function DialogRoot({
  id: providedId,
  children,
  ...state
}: DialogRootProps) {
  const id = useEntityId(providedId);
  const overlay = useOverlayOpen(state);
  return <DialogContext value={{ id, overlay }}>{children}</DialogContext>;
}

export type DialogTriggerProps = ButtonProps;

export function DialogTrigger({ onPress, ...props }: DialogTriggerProps) {
  const { overlay } = useDialog();
  return (
    <Button
      {...props}
      onPress={(event) => {
        onPress?.(event);
        overlay.setOpen(true);
      }}
    />
  );
}

export interface DialogContentProps {
  readonly width?: number;
  readonly layout?: GuiKitLayout;
  readonly children?: ReactNode;
}

/** A modal confined to this World's canvas; runtime owns focus trapping and return. */
export function DialogContent({
  width = 368,
  layout,
  children,
}: DialogContentProps) {
  const { id, overlay } = useDialog();
  const kit = useGuiKit();
  return (
    <Resources>
      <Floating
        id={`${id}/dialog`}
        mode="modal"
        side="centre"
        align="centre"
        band="dialog"
        open={overlay.open}
        controlled={overlay.controlled}
        onVisibleChange={overlay.onVisibleChange}
        layout={{ width: kit.unit(width), ...layout }}
      >
        {children}
      </Floating>
    </Resources>
  );
}

export interface DialogHeaderProps {
  readonly title: string;
  readonly children?: ReactNode;
}

export function DialogHeader({ title, children }: DialogHeaderProps) {
  const { id } = useDialog();
  return (
    <PanelHeader id={`${id}/header`} title={title}>
      {children}
    </PanelHeader>
  );
}

export interface DialogSectionProps {
  readonly layout?: GuiKitLayout;
  readonly children?: ReactNode;
}

export function DialogBody({ layout, children }: DialogSectionProps) {
  const { id } = useDialog();
  const kit = useGuiKit();
  return (
    <Column
      id={`${id}/body`}
      padding={kit.unit(kit.tokens.inset)}
      layout={layout}
    >
      {children}
    </Column>
  );
}

export function DialogActions({ layout, children }: DialogSectionProps) {
  const { id } = useDialog();
  const kit = useGuiKit();
  return (
    <Row
      id={`${id}/actions`}
      height={kit.unit(kit.tokens.controlHeight + 2 * kit.tokens.inset)}
      padding={kit.unit(kit.tokens.inset)}
      layout={layout}
    >
      {children}
    </Row>
  );
}

export type DialogCloseProps = ButtonProps;

export function DialogClose({ onPress, ...props }: DialogCloseProps) {
  const { overlay } = useDialog();
  return (
    <Button
      {...props}
      onPress={(event) => {
        onPress?.(event);
        overlay.setOpen(false);
      }}
    />
  );
}

export const Dialog = {
  Root: DialogRoot,
  Trigger: DialogTrigger,
  Content: DialogContent,
  Header: DialogHeader,
  Body: DialogBody,
  Actions: DialogActions,
  Close: DialogClose,
};
