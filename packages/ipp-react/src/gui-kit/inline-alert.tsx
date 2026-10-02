/**
 * Persistent information beside the content it concerns: a frame tinted in
 * the severity's role colour, the severity's icon, one line of text and an
 * optional recovery action. The icon's shape and the wording carry the
 * severity as well as the colour. The alert is a control-height row that
 * fills its container's width; put a validation alert after its field, so
 * that it moves only what lies below.
 */
import { Children, Entity } from "../components.js";
import { Font, Layout } from "../gui/components.js";
import { Skin } from "../gui/theme.js";
import type { GuiPressListener } from "../gui/callbacks.js";
import type { GuiControlRef } from "../gui/control-ref.js";
import { useGuiKit, type GuiKitTone } from "./kit.js";
import { LAYOUT_ROW, Strut, type GuiKitLayout } from "./layout.js";
import { SecondaryButton } from "./secondary-button.js";
import { Icon, TextLine } from "./text.js";
import type { KitThemeName } from "./themes.js";

export type InlineAlertSeverity = "information" | "warning" | "error";

/** A secondary button at the alert's end. */
export interface InlineAlertAction {
  readonly label: string;
  readonly onPress?: GuiPressListener;
  readonly ref?: GuiControlRef;
}

export interface InlineAlertProps {
  readonly id: string;
  readonly severity: InlineAlertSeverity;
  readonly text: string;
  readonly action?: InlineAlertAction;
  readonly layout?: GuiKitLayout;
}

const SEVERITIES: Readonly<
  Record<InlineAlertSeverity, { tone: GuiKitTone; theme: KitThemeName }>
> = {
  information: { tone: "accent", theme: "alertInformation" },
  warning: { tone: "amber", theme: "alertWarning" },
  error: { tone: "error", theme: "alertError" },
};

export function InlineAlert({
  id,
  severity,
  text,
  action,
  layout,
}: InlineAlertProps) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const { tone, theme } = SEVERITIES[severity];
  // The action is a secondary button half an inset from the alert's end,
  // clear of the frame's cut.
  return (
    <Entity id={id}>
      <Layout
        kind={LAYOUT_ROW}
        height={kit.unit(t.controlHeight)}
        padding_left={kit.unit(t.inset)}
        padding_right={kit.unit(action ? t.inset / 2 : t.inset)}
        {...layout}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Skin theme={kit.theme(theme)} />
      <Children>
        <Strut id={`${id}/strut`} height={kit.unit(t.controlHeight)} />
        <Icon id={`${id}/icon`} icon={severity} tone={tone} />
        <TextLine
          id={`${id}/text`}
          text={text}
          layout={{ flex: 1, margin_left: kit.unit(t.inset) }}
        />
        {action && <SecondaryButton id={`${id}/action`} {...action} />}
      </Children>
    </Entity>
  );
}
