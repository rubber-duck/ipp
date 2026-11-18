import type { ReactNode } from "react";
import { Children, Entity, PresentationGate } from "../../components.js";
import { Behavior, Layout, Style } from "../../gui/components.js";
import { Skin } from "../../gui/theme.js";
import { useField } from "../../scope/field.js";
import { useEntityId } from "../../scope/identity.js";
import { useGuiKit } from "../kit.js";
import { Column, Row, type ContainerProps } from "../layout.js";
import { TextLine } from "../text.js";

export interface TimelineItem {
  readonly key: string;
  readonly timestamp: string;
  readonly title: string;
  readonly body?: ReactNode;
  readonly actions?: ReactNode;
  readonly status?: "normal" | "warning" | "error";
}
export interface TimelineProps extends Omit<ContainerProps, "children"> {
  readonly items: readonly TimelineItem[];
  readonly emptyLabel?: string;
  readonly loading?: string;
}
export function Timeline({
  id: providedId,
  items,
  emptyLabel = "No events",
  loading,
  ...layout
}: TimelineProps) {
  const id = useEntityId(providedId);
  return (
    <Column {...layout} id={id} gap={0}>
      {items.map((item, index) => (
        <TimelineEntry
          key={item.key}
          id={`${id}/${item.key}`}
          item={item}
          connect={index + 1 < items.length}
        />
      ))}
      {loading !== undefined ? (
        <TextLine text={loading} />
      ) : (
        !items.length && <TextLine text={emptyLabel} />
      )}
    </Column>
  );
}

function TimelineEntry({
  id,
  item,
  connect,
}: {
  id: string;
  item: TimelineItem;
  connect: boolean;
}) {
  const kit = useGuiKit();
  const marker = kit.unit(8),
    line = kit.unit(2);
  const body = `${id}/body`;
  const measured = useField(body, "CanvasBounds", "local_height");
  const height = Math.max(
    marker,
    typeof measured.value === "number" ? measured.value : 0,
  );
  return (
    <Entity id={id}>
      <PresentationGate open />
      <Layout kind={1} gap={kit.spacing.compact} fit_height />
      <Behavior
        focusable={false}
        semantic_label={`${item.timestamp} ${item.title}`}
      />
      <Children>
        <Entity id={`${id}/rail`}>
          <Layout kind={2} width={marker} height={height} align_items={0} />
          <Children>
            <Entity id={`${id}/marker`}>
              <Layout width={marker} height={marker} />
              <Skin
                theme={kit.theme(
                  item.status === "error"
                    ? "valueError"
                    : item.status === "warning"
                      ? "valueAmber"
                      : "valueAccent",
                )}
              />
            </Entity>
            {connect && (
              <Entity id={`${id}/connector`}>
                <Layout width={line} flex={1} />
                <Style />
                <Skin theme={kit.theme("valueNeutral")} />
              </Entity>
            )}
          </Children>
        </Entity>
        <Column id={body} measureBounds flex={1} padding="compact">
          <Row>
            <TextLine text={item.timestamp} tone="neutral" />
            <TextLine text={item.title} flex={1} />
          </Row>
          {typeof item.body === "string" ? (
            <TextLine text={item.body} />
          ) : (
            item.body
          )}
          {item.actions}
        </Column>
      </Children>
    </Entity>
  );
}
