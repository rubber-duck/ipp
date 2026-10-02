/**
 * Lines one idle line weight thick whose layout box is the line, filling their
 * container across: horizontal in a column, or vertical in a row; and the
 * labelled separator that names a section.
 */
import { Entity } from "../components.js";
import { Layout } from "../gui/components.js";
import { Skin } from "../gui/theme.js";
import { useGuiKit } from "./kit.js";
import { LAYOUT_COLUMN, LAYOUT_ROW, Row, type GuiKitLayout } from "./layout.js";
import { TextLine } from "./text.js";

export interface SeparatorProps {
  readonly id: string;
  /**
   * `division` (neutral) divides a panel, `rule` (accent) underlines a
   * heading, and `quiet` (the quiet line) separates what repeats between data,
   * such as rows.
   */
  readonly tone?: "division" | "rule" | "quiet";
  readonly vertical?: boolean;
  readonly layout?: GuiKitLayout;
}

export function Separator({
  id,
  tone = "division",
  vertical = false,
  layout,
}: SeparatorProps) {
  const kit = useGuiKit();
  const thickness = kit.unit(kit.tokens.lineWidth);
  return (
    <Entity id={id}>
      <Layout
        kind={vertical ? LAYOUT_COLUMN : LAYOUT_ROW}
        {...(vertical ? { width: thickness } : { height: thickness })}
        {...layout}
      />
      <Skin theme={kit.theme(tone)} />
    </Entity>
  );
}

export interface LabelledSeparatorProps {
  readonly id: string;
  /** The section's label, in the accent at body size. */
  readonly label: string;
  /**
   * Start with a short line before the label, the content inset long, as a
   * division across a panel does; without it the label starts the row, as a
   * section heading at the content's edge does.
   */
  readonly stub?: boolean;
  readonly layout?: GuiKitLayout;
}

/**
 * A panel division that names the section below it: an optional short line,
 * the label, and a line filling the rest of the row, half an inset from the
 * label on either side. It is a dense row high.
 */
export function LabelledSeparator({
  id,
  label,
  stub = true,
  layout,
}: LabelledSeparatorProps) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const gap = kit.unit(t.inset / 2);
  const line = { align_y: 0 };
  return (
    <Row id={id} height={t.denseRow} {...(layout ? { layout } : {})}>
      {stub && (
        <Separator
          id={`${id}/stub`}
          layout={{ ...line, width: kit.unit(t.inset), margin_right: gap }}
        />
      )}
      <TextLine id={`${id}/label`} text={label} tone="accent" />
      <Separator
        id={`${id}/line`}
        layout={{ ...line, flex: 1, margin_left: gap }}
      />
    </Row>
  );
}
