import type { ReactNode } from "react";
import { Children, Entity, PresentationGate } from "../../components.js";
import { Behavior, Bounds, Group, Layout } from "../../gui/components.js";
import { useEntityId } from "../../scope/identity.js";
import { Button } from "../button.js";
import { Column, Toolbar, type ContainerProps } from "../layout.js";
import { useApplicationValue } from "../selection/application-value.js";
import { FieldMotion } from "../values/field-motion.js";

interface RegionProps {
  id: string;
  open: boolean;
  content: ReactNode;
  unmount: boolean;
  label: string;
}
function Region({ id, open, content, unmount, label }: RegionProps) {
  if (!open && unmount) return null;
  return (
    <Entity id={id}>
      <PresentationGate open />
      <Bounds />
      <Layout kind={2} clip fit_height height_fraction={0} />
      <Behavior enabled={open} focusable={false} semantic_label={label} />
      <Children>
        <Column id={`${id}/content`} fitHeight>
          {content}
        </Column>
      </Children>
      <FieldMotion
        target={id}
        component="GuiLayout"
        field="height_fraction"
        from={0}
        to={1}
        fraction={open ? 1 : 0}
      />
    </Entity>
  );
}

export interface CollapsibleProps extends ContainerProps {
  readonly label: string;
  readonly expanded?: boolean;
  readonly defaultExpanded?: boolean;
  readonly onExpandedChange?: (next: boolean) => void;
  readonly disabled?: boolean;
  readonly unmountOnClose?: boolean;
}
export function Collapsible({
  id: providedId,
  label,
  expanded,
  defaultExpanded = false,
  onExpandedChange,
  disabled = false,
  unmountOnClose = false,
  children,
  ...layout
}: CollapsibleProps) {
  const id = useEntityId(providedId),
    state = useApplicationValue(expanded, defaultExpanded, onExpandedChange);
  return (
    <Column {...layout} id={id} gap={0}>
      <Button
        id={`${id}/header`}
        label={label}
        selected={state.current}
        disabled={disabled}
        onPress={() => state.change((previous) => !previous)}
      />
      <Region
        id={`${id}/region`}
        label={label}
        open={state.current}
        content={children}
        unmount={unmountOnClose}
      />
    </Column>
  );
}

export interface AccordionItem {
  readonly key: string;
  readonly label: string;
  readonly content: ReactNode;
  readonly disabled?: boolean;
}
export interface AccordionProps extends Omit<ContainerProps, "children"> {
  readonly items: readonly AccordionItem[];
  readonly multiple?: boolean;
  readonly collapsible?: boolean;
  readonly expanded?: readonly string[];
  readonly defaultExpanded?: readonly string[];
  readonly onExpandedChange?: (keys: readonly string[]) => void;
  readonly unmountOnClose?: boolean;
}
export function Accordion({
  id: providedId,
  items,
  multiple = false,
  collapsible = true,
  expanded,
  defaultExpanded = [],
  onExpandedChange,
  unmountOnClose = false,
  ...layout
}: AccordionProps) {
  const id = useEntityId(providedId),
    state = useApplicationValue(expanded, defaultExpanded, onExpandedChange);
  function existing(keys: readonly string[]) {
    const found = items.filter((s) => keys.includes(s.key)).map((s) => s.key);
    return multiple ? found : found.slice(0, 1);
  }
  const shown = existing(state.current);
  return (
    <Toolbar
      {...layout}
      id={id}
      label="Sections"
      orientation="vertical"
      gap={0}
    >
      {items.map((item) => {
        const key = `${id}/${item.key}`,
          isOpen = shown.includes(item.key);
        return (
          <Column id={key} key={item.key} gap={0}>
            <Button
              id={`${key}/header`}
              label={item.label}
              selected={isOpen}
              disabled={item.disabled ?? false}
              onPress={() =>
                state.change((before) => {
                  const current = existing(before);
                  if (current.includes(item.key))
                    return collapsible || current.length > 1
                      ? current.filter((k) => k !== item.key)
                      : before;
                  return multiple ? [...current, item.key] : [item.key];
                })
              }
            />
            <AccordionRegion
              id={`${key}/body`}
              label={item.label}
              open={isOpen}
              unmount={unmountOnClose}
              content={item.content}
            />
          </Column>
        );
      })}
    </Toolbar>
  );
}

/** A nested group fences rich region controls from the outer header navigation. */
function AccordionRegion(props: RegionProps) {
  return (
    <Entity id={`${props.id}/scope`}>
      <Layout kind={2} fit_height />
      <Group axis={1} selection={0} />
      <Children>
        <Region {...props} />
      </Children>
    </Entity>
  );
}
