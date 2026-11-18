import { useEntityId } from "../../scope/identity.js";
import { Entity } from "../../components.js";
import { Bounds, Layout } from "../../gui/components.js";
import { Skin } from "../../gui/theme.js";
import { useGuiKit } from "../kit.js";
import { Column, Row, Toolbar, type ContainerProps } from "../layout.js";
import { Button } from "../button.js";
import { TextLine } from "../text.js";
import { useApplicationValue } from "../selection/application-value.js";

export interface WaterfallSpan {
  readonly key: string;
  readonly label: string;
  readonly start: number;
  readonly duration: number;
  readonly children?: readonly WaterfallSpan[];
}
export interface WaterfallProps extends Omit<ContainerProps, "children"> {
  readonly spans: readonly WaterfallSpan[];
  readonly selected?: string | null;
  readonly defaultSelected?: string | null;
  readonly onSelect?: (id: string | null) => void;
  readonly expanded?: readonly string[];
  readonly defaultExpanded?: readonly string[];
  readonly onExpandedChange?: (ids: readonly string[]) => void;
  readonly labelWidth?: number;
  readonly formatDuration?: (duration: number) => string;
}

export function Waterfall({
  id: providedId,
  spans,
  selected,
  defaultSelected = null,
  onSelect,
  expanded,
  defaultExpanded = [],
  onExpandedChange,
  labelWidth = 150,
  formatDuration = String,
  ...layout
}: WaterfallProps) {
  const id = useEntityId(providedId),
    kit = useGuiKit(),
    choice = useApplicationValue(selected, defaultSelected, onSelect),
    open = useApplicationValue(expanded, defaultExpanded, onExpandedChange);
  const shown = new Set(open.current),
    seen = new Set<string>();
  const rows: { span: WaterfallSpan; depth: number }[] = [];
  let start = Infinity,
    end = -Infinity;
  const pending = spans
    .map((span) => ({ span, depth: 0, visible: true }))
    .reverse();
  while (pending.length) {
    const { span, depth, visible } = pending.pop()!;
    if (
      seen.has(span.key) ||
      !Number.isFinite(span.start) ||
      !Number.isFinite(span.duration) ||
      span.duration < 0 ||
      !Number.isFinite(span.start + span.duration)
    )
      throw new RangeError("Invalid or duplicate waterfall span");
    seen.add(span.key);
    start = Math.min(start, span.start);
    end = Math.max(end, span.start + span.duration);
    if (visible) rows.push({ span, depth });
    for (let i = (span.children?.length ?? 0) - 1; i >= 0; i--)
      pending.push({
        span: span.children![i]!,
        depth: depth + 1,
        visible: visible && shown.has(span.key),
      });
  }
  const duration = end - start || 1;
  return (
    <Column {...layout} gap={0} id={id}>
      {rows.length === 0 && <TextLine text="No spans" />}
      <Toolbar
        id={`${id}/rows`}
        orientation="vertical"
        gap={0}
        label="Trace spans"
      >
        {rows.map(({ span, depth }) => (
          <Row id={`${id}/${span.key}`} key={span.key} gap={0}>
            <Row
              width={labelWidth}
              gap={0}
              layout={{
                padding_left: Math.min(
                  labelWidth / 2,
                  kit.spacing.compact * depth,
                ),
              }}
            >
              {!!span.children?.length && (
                <Button
                  id={`${id}/${span.key}/toggle`}
                  label={shown.has(span.key) ? "−" : "+"}
                  semanticLabel={`Expand or collapse ${span.label}`}
                  onPress={() =>
                    open.change((old) =>
                      old.includes(span.key)
                        ? old.filter((k) => k !== span.key)
                        : [...old, span.key],
                    )
                  }
                />
              )}
              <Button
                id={`${id}/${span.key}/select`}
                flex={1}
                label={span.label}
                selected={choice.current === span.key}
                semanticLabel={`${span.label}, ${formatDuration(span.duration)}`}
                onPress={() => choice.change(() => span.key)}
              />
            </Row>
            <Row flex={1} gap={0}>
              {(
                [
                  [span.start - start, false],
                  [span.duration, true],
                  [end - span.start - span.duration, false],
                ] as const
              ).map(([raw, mark], i) => {
                const share = Number(raw) / duration;
                return share > 0 || mark ? (
                  <Entity id={`${id}/${span.key}/bar/${i}`} key={i}>
                    {mark && <Bounds />}
                    <Layout
                      flex={share}
                      height={kit.unit(8)}
                      min_width={mark && share === 0 ? 1 : 0}
                    />
                    {mark === true && <Skin theme={kit.theme("valueAccent")} />}
                  </Entity>
                ) : null;
              })}
            </Row>
          </Row>
        ))}
      </Toolbar>
    </Column>
  );
}
