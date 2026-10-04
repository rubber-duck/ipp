/**
 * A read-only compact label of an observed state: a small part-cut frame in
 * the status's role colour, a marker and the application's word for the
 * state. Each status has its own marker shape, so colour is never the only
 * signal. The language reserves circles for dials, radio marks and progress,
 * so the steady markers are squares: lit and filled while active, unlit and
 * outlined while inactive; a busy, warning or error status shows its icon. A
 * badge is never focusable; an actionable badge is a labelled button instead.
 * Its width hugs its label.
 */
import { Children, Entity } from "../components.js";
import { Style, Font, Layout } from "../gui/components.js";
import { Skin } from "../gui/theme.js";
import type { GuiKitIcon } from "./icons.js";
import { useGuiKit, type GuiKitTone } from "./kit.js";
import { LAYOUT_LEAF, LAYOUT_ROW, Strut, type GuiKitLayout } from "./layout.js";
import { Icon, TextLine, glyphSizeForWidth, textWidth } from "./text.js";
import type { KitThemeName } from "./themes.js";

/**
 * `active` (online, running, connected), `busy` (syncing, working), `inactive`
 * (offline, stopped), `warning` (degraded) and `error`.
 */
export type StatusBadgeStatus =
  | "active"
  | "busy"
  | "inactive"
  | "warning"
  | "error";

export interface StatusBadgeProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  readonly id: string;
  readonly status: StatusBadgeStatus;
  /** The state's word, such as Online. */
  readonly label: string;
  readonly layout?: GuiKitLayout;
}

/** Side of the marker, at the tokens' `em`. */
const MARKER = 12;

const STATUSES: Readonly<
  Record<
    StatusBadgeStatus,
    {
      tone: GuiKitTone;
      frame: KitThemeName;
      marker: { theme: KitThemeName } | { icon: GuiKitIcon };
    }
  >
> = {
  active: {
    tone: "accent",
    frame: "badgeAccent",
    marker: { theme: "markerLit" },
  },
  busy: { tone: "accent", frame: "badgeAccent", marker: { icon: "sync" } },
  inactive: {
    tone: "neutral",
    frame: "badgeNeutral",
    marker: { theme: "markerUnlit" },
  },
  warning: { tone: "amber", frame: "badgeAmber", marker: { icon: "warning" } },
  error: { tone: "error", frame: "badgeError", marker: { icon: "error" } },
};

export function StatusBadge({
  id,
  layer = 0,
  status,
  label,
  layout,
}: StatusBadgeProps) {
  const kit = useGuiKit();
  const t = kit.tokens;
  const { tone, frame, marker } = STATUSES[status];
  // The marker sits as far from the start as from the top and bottom, and
  // the label as far from the end; half an inset separates them.
  const edge = (t.smallHeight - MARKER) / 2;
  const gap = t.inset / 2;
  const width =
    kit.unit(edge + MARKER + gap + edge) +
    textWidth(label, kit.typeSize("small"));
  return (
    <Entity id={id}>
      <Style layer={layer} />
      <Layout
        kind={LAYOUT_ROW}
        width={width}
        height={kit.unit(t.smallHeight)}
        padding_left={kit.unit(edge)}
        {...layout}
      />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Skin theme={kit.theme(frame)} />
      <Children>
        <Strut id={`${id}/strut`} height={kit.unit(t.smallHeight)} />
        {"theme" in marker ? (
          <Entity id={`${id}/marker`}>
            <Layout
              kind={LAYOUT_LEAF}
              width={kit.unit(MARKER)}
              height={kit.unit(MARKER)}
              align_y={0}
            />
            <Skin theme={kit.theme(marker.theme)} />
          </Entity>
        ) : (
          <Icon
            id={`${id}/marker`}
            icon={marker.icon}
            tone={tone}
            size={glyphSizeForWidth(MARKER)}
          />
        )}
        <TextLine
          id={`${id}/label`}
          text={label}
          tone={tone}
          size="small"
          layout={{ margin_left: kit.unit(gap) }}
        />
      </Children>
    </Entity>
  );
}
