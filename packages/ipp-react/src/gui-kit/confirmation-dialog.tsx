/**
 * A confirmation dialog: a modal floating surface centred on the canvas,
 * whose header strip holds the action-specific title, in the accent, and a
 * close button, its only control, which cancels; then the consequence, one
 * line of body text a row, and Cancel beside the action, equal and a content
 * inset apart. Cancel is a secondary button; the action a primary one with
 * its own label, in the amber variant when it is destructive.
 *
 * While open it blocks pointer, hover, wheel and keyboard input to what lies
 * beneath it in its canvas, so a press outside it confirms nothing, and Tab
 * stays inside it. Opening moves focus to Cancel, the first control that
 * takes focus, since the close button takes none and Escape is its key;
 * Escape cancels. Closing returns focus to the control that held it.
 * `onConfirm` or `onCancel` reports the answer once for each opening, and the
 * application then closes the dialog by declaring it closed.
 *
 * A dialog is a root of the canvas on the dialog plane, below the toasts:
 * declare it outside any `Children`. Its width is the language's dialog
 * width unless given.
 */
import { useRef } from "react";
import { Children, Entity } from "../components.js";
import { Button } from "../gui/controls.js";
import { Layout } from "../gui/components.js";
import { Skin } from "../gui/theme.js";
import { useGuiKit } from "./kit.js";
import { LAYOUT_COLUMN, LAYOUT_LEAF, LAYOUT_ROW, Row } from "./layout.js";
import { Floating, useOverlayOpen } from "./overlay.js";
import { PanelHeader } from "./panel.js";
import { TextLine } from "./text.js";
import { WindowControl } from "./window-controls.js";

/**
 * A dialog's width by default, at the tokens' `em`: two 160-unit buttons
 * with the content inset beside and between them.
 */
const WIDTH = 368;

export interface ConfirmationDialogProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  /** Symbolic id of the dialog; its parts extend it. */
  readonly id: string;
  /** Whether the dialog is open; the application closes it once answered. */
  readonly open: boolean;
  /** The action-specific title, such as "Delete node?". */
  readonly title: string;
  /** The consequence, one line of text each. */
  readonly body: string | readonly string[];
  /** The action button's label, such as Delete. */
  readonly action: string;
  /** Cancel's label; Cancel by default. */
  readonly cancel?: string;
  /** The amber variant, for a destructive action; true by default. */
  readonly destructive?: boolean;
  readonly onConfirm?: () => void;
  /** Cancel, the close button or Escape. */
  readonly onCancel?: () => void;
  /** Width at the tokens' `em`; 368 by default. */
  readonly width?: number;
}

export function ConfirmationDialog({
  id,
  layer = 0,
  open,
  title,
  body,
  action,
  cancel = "Cancel",
  destructive = true,
  onConfirm,
  onCancel,
  width = WIDTH,
}: ConfirmationDialogProps) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const lines = typeof body === "string" ? [body] : body;
  const inset = kit.unit(t.inset);

  // One answer per opening: the first of Cancel, close, Escape or the action.
  const answered = useRef(!open);
  const opened = useRef(open);
  if (open !== opened.current) {
    opened.current = open;
    if (open) answered.current = false;
  }
  const answer = (confirmed: boolean) => {
    if (answered.current) return;
    answered.current = true;
    (confirmed ? onConfirm : onCancel)?.();
  };
  // Escape: the runtime closes the dialog and the client hears of it.
  const overlay = useOverlayOpen({
    open,
    onOpenChange: (next) => {
      if (!next) answer(false);
    },
  });

  const button = (
    name: "cancel" | "action",
    label: string,
    theme: "secondary" | "amber" | undefined,
    onPress: () => void,
    first: boolean,
  ) => (
    <Entity id={`${id}/${name}`}>
      {/* A flexible box's margin comes out of its own share of the row, so
          each button gives half the gap and both stay equal. */}
      <Layout
        kind={LAYOUT_LEAF}
        flex={1}
        height={kit.unit(t.controlHeight)}
        {...(first ? { margin_right: inset / 2 } : { margin_left: inset / 2 })}
      />
      {theme && <Skin theme={kit.theme(theme)} />}
      <Button label={label} onPress={onPress} />
    </Entity>
  );
  return (
    <Floating
      id={id}
      layer={layer}
      side="centre"
      align="centre"
      mode="modal"
      band="dialog"
      open={open}
      onVisibleChange={overlay.onVisibleChange}
      layout={{ width: kit.unit(width) }}
    >
      <PanelHeader id={`${id}/header`} title={title}>
        <WindowControl
          id={`${id}/close`}
          kind="close"
          label={cancel}
          focusable={false}
          onPress={() => answer(false)}
        />
      </PanelHeader>
      <Entity id={`${id}/body`}>
        <Layout
          kind={LAYOUT_COLUMN}
          height={
            inset +
            lines.length * kit.unit(t.denseRow) +
            inset +
            kit.unit(t.controlHeight) +
            inset
          }
          padding_left={inset}
          padding_right={inset}
          padding_top={inset}
          padding_bottom={inset}
        />
        <Children>
          {lines.map((line, index) => (
            <Row key={index} id={`${id}/line/${index}`} height={t.denseRow}>
              <TextLine id={`${id}/line/${index}/text`} text={line} />
            </Row>
          ))}
          <Entity id={`${id}/actions`}>
            <Layout
              kind={LAYOUT_ROW}
              height={kit.unit(t.controlHeight)}
              margin_top={inset}
            />
            <Children>
              {button("cancel", cancel, "secondary", () => answer(false), true)}
              {button(
                "action",
                action,
                destructive ? "amber" : undefined,
                () => answer(true),
                false,
              )}
            </Children>
          </Entity>
        </Children>
      </Entity>
    </Floating>
  );
}
