import { useEntityId } from "../scope/identity.js";
/** Parent-owned flow spacing and alignment through ordinary GuiLayout fields. */
import type { ReactNode } from "react";
import {
  PresentationGate,
  Children,
  Entity,
  type ComponentFields,
} from "../components.js";
import { Style, Layout } from "../gui/components.js";
import { useGuiKit } from "./kit.js";

/**
 * Layout fields a kit component's root takes over its own, in the World's
 * units: margins, flex, alignment or an explicit width to place it.
 */
export type GuiKitLayout = ComponentFields<"GuiLayout">;

/** Named layout props use the World's logical units. Explicit layout fields win. */
export interface GuiKitLayoutProps {
  readonly layout?: GuiKitLayout | undefined;
  readonly width?: number | undefined;
  readonly height?: number | undefined;
  readonly minWidth?: number | undefined;
  readonly minHeight?: number | undefined;
  readonly maxWidth?: number | undefined;
  readonly maxHeight?: number | undefined;
  readonly flex?: number | undefined;
  /** Fit content within the available width; explicit width wins. */
  readonly fitWidth?: boolean | undefined;
  /** Fit content within the available height; explicit height wins. */
  readonly fitHeight?: boolean | undefined;
  readonly alignX?: "start" | "center" | "end" | "default" | undefined;
  readonly alignY?: "start" | "center" | "end" | "default" | undefined;
  readonly padding?: number | undefined;
  readonly margin?: number | undefined;
  readonly clip?: boolean | undefined;
}

/** Flow containers place their children; individual child alignment still wins. */
export interface GuiKitFlowLayoutProps extends GuiKitLayoutProps {
  readonly gap?: number | undefined;
  readonly alignItems?: "start" | "center" | "end" | "default" | undefined;
}

export function layoutFields(
  props: GuiKitFlowLayoutProps,
  defaults: GuiKitLayout = {},
): GuiKitLayout {
  const fields: { -readonly [Key in keyof GuiKitLayout]: GuiKitLayout[Key] } = {
    width: -1,
    height: -1,
    min_width: 0,
    min_height: 0,
    max_width: -1,
    max_height: -1,
    fit_width: false,
    fit_height: false,
    flex: 0,
    align_x: 2,
    align_y: 2,
    gap: 0,
    align_items: 2,
    clip: false,
    padding_top: 0,
    padding_right: 0,
    padding_bottom: 0,
    padding_left: 0,
    margin_top: 0,
    margin_right: 0,
    margin_bottom: 0,
    margin_left: 0,
    ...defaults,
  };
  const lengths = {
    width: props.width,
    height: props.height,
    min_width: props.minWidth,
    min_height: props.minHeight,
    max_width: props.maxWidth,
    max_height: props.maxHeight,
    flex: props.flex,
    gap: props.gap,
    clip: props.clip,
    fit_width: props.fitWidth,
    fit_height: props.fitHeight,
  };
  for (const [name, value] of Object.entries(lengths)) {
    if (value !== undefined) Object.assign(fields, { [name]: value });
  }
  const alignment = { start: -1, center: 0, end: 1, default: 2 } as const;
  if (props.alignX !== undefined) fields.align_x = alignment[props.alignX];
  if (props.alignY !== undefined) fields.align_y = alignment[props.alignY];
  if (props.alignItems !== undefined)
    fields.align_items = alignment[props.alignItems];
  for (const side of ["top", "right", "bottom", "left"] as const) {
    if (props.padding !== undefined) fields[`padding_${side}`] = props.padding;
    if (props.margin !== undefined) fields[`margin_${side}`] = props.margin;
  }
  return { ...fields, ...props.layout };
}

/** GuiLayout operations. */
export const LAYOUT_LEAF = 0;
export const LAYOUT_ROW = 1;
export const LAYOUT_COLUMN = 2;
export const LAYOUT_STACK = 3;
export const LAYOUT_PADDING = 4;

/** A zero-width leaf `height` tall, in the World's units, that its row centres on. */
export function Strut({
  id: providedId,
  height,
}: {
  readonly id?: string;
  readonly height: number;
}) {
  const id = useEntityId(providedId);
  return (
    <Entity id={id}>
      <PresentationGate open />
      <Layout kind={LAYOUT_LEAF} width={0} height={height} />
    </Entity>
  );
}

export interface RowProps extends GuiKitFlowLayoutProps {
  /** Nonnegative layer offset applied once at this component root; zero inherits. */
  readonly layer?: number;
  readonly id?: string;
  readonly children?: ReactNode;
}

/** A naturally sized row with a token minimum and vertically centered children. */
export function Row({
  id: providedId,
  layer = 0,
  children,
  ...layout
}: RowProps) {
  const id = useEntityId(providedId);
  const kit = useGuiKit();
  const height = layout.layout?.height ?? layout.height;
  const fields = layoutFields(layout, {
    fit_height: true,
    min_height:
      height !== undefined && height >= 0 ? 0 : kit.unit(kit.tokens.row),
    align_items: 0,
  });
  return (
    <Entity id={id}>
      <PresentationGate open />
      <Style layer={layer} />
      <Layout kind={LAYOUT_ROW} {...fields} />
      <Children>{children}</Children>
    </Entity>
  );
}

export type ToolbarProps = RowProps;

/** A centered control row with the skin's half-inset between adjacent controls. */
export function Toolbar(props: ToolbarProps) {
  const kit = useGuiKit();
  return <Row {...props} gap={props.gap ?? kit.spacing.label} />;
}

export interface ContainerProps extends GuiKitFlowLayoutProps {
  readonly id?: string;
  readonly layer?: number;
  readonly children?: ReactNode;
}

/** A column that fits its content height by default; explicit height or fitHeight=false fills a bounded height. */
export function Column({
  id: providedId,
  layer = 0,
  children,
  ...layout
}: ContainerProps) {
  const id = useEntityId(providedId);
  return (
    <Entity id={id}>
      <PresentationGate open />
      <Style layer={layer} />
      <Layout
        kind={LAYOUT_COLUMN}
        {...layoutFields(layout, { fit_height: true })}
      />
      <Children>{children}</Children>
    </Entity>
  );
}

/** Place ordinary children on the same box rather than a flow axis. */
export function Stack({
  id: providedId,
  layer = 0,
  children,
  ...layout
}: ContainerProps) {
  const id = useEntityId(providedId);
  return (
    <Entity id={id}>
      <PresentationGate open />
      <Style layer={layer} />
      <Layout kind={LAYOUT_STACK} {...layoutFields(layout)} />
      <Children>{children}</Children>
    </Entity>
  );
}
