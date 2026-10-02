/**
 * What a list or grid shows when it holds nothing: a content frame, cut as a
 * list frame is, around one line of secondary text centred in it. It is a
 * control-height row that fills its container's width.
 */
import { Children, Entity } from "../components.js";
import { Font, Layout } from "../gui/components.js";
import { Skin } from "../gui/theme.js";
import { useGuiKit } from "./kit.js";
import { LAYOUT_STACK, type GuiKitLayout } from "./layout.js";
import { TextLine } from "./text.js";

export interface EmptyStateProps {
  readonly id: string;
  /** The message, such as No records. */
  readonly text: string;
  readonly layout?: GuiKitLayout;
}

export function EmptyState({ id, text, layout }: EmptyStateProps) {
  const kit = useGuiKit();
  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_STACK}
        height={kit.unit(kit.tokens.controlHeight)}
        {...layout}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Skin theme={kit.theme("frame")} />
      <Children>
        <TextLine
          id={`${id}/text`}
          text={text}
          tone="neutral"
          size="small"
          layout={{ align_x: 0 }}
        />
      </Children>
    </Entity>
  );
}
