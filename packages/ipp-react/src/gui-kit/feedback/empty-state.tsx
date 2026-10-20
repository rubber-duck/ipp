import {
  KitLayout,
  useGuiKitLayout,
  type GuiKitLayoutProps,
  LAYOUT_STACK,
} from "../layout.js";

import { useEntityId } from "../../scope/identity.js";
/**
 * What a list or grid shows when it holds nothing: a content frame, cut as a
 * list frame is, around one line of secondary text centred in it. It is a
 * control-height row that fills its container's width.
 */
import { PresentationGate, Children, Entity } from "../../components.js";
import { Style, Font } from "../../gui/components.js";
import { Skin } from "../../gui/theme.js";
import { useGuiKit } from "../kit.js";

import { TextLine } from "../text.js";

export interface EmptyStateProps extends GuiKitLayoutProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  readonly id?: string;
  /** The message, such as No records. */
  readonly text: string;
}

export function EmptyState(props: EmptyStateProps) {
  const { id: providedId, layer = 0, text, layout } = useGuiKitLayout(props);
  const id = useEntityId(providedId);
  const kit = useGuiKit();
  return (
    <Entity id={id}>
      <PresentationGate open />
      <Style layer={layer} />
      <KitLayout
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
