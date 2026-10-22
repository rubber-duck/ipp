/** Runtime measurements for client presentation decisions that need numeric extents. */
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { Entity, PresentationGate } from "../components.js";
import { Style, Text } from "../gui/components.js";
import { Resources } from "../resources.js";
import { useEntityId } from "../scope/identity.js";
import { useField } from "../scope/field.js";
import type { AssetReference } from "../assets/declarations.js";
import { useGuiKit } from "./kit.js";

type Request = readonly [text: string, size: number];

/**
 * Prefer fit_width/fit_height for ordinary control sizing. A composite that
 * must choose content or place a label around authored geometry can observe
 * unwrapped CanvasText bounds. Font metrics, kerning and readiness belong to
 * the runtime; these transparent leaves never contribute to its layout tree.
 * A changed request gets fresh observations, so old dimensions cannot size new
 * text. Pending widths are provisional; controls retain their identity and
 * eligibility while observations arrive.
 */
export function useTextMeasurements(requests: readonly Request[]) {
  const kit = useGuiKit();
  const signature = JSON.stringify([kit.font, requests]);
  const request = useMemo(
    () => ({ font: kit.font, values: requests }),
    // Equivalent asset references and request arrays are one measurement.
    [signature],
  );
  const [observed, setObserved] = useState<{
    request: typeof request;
    widths: ReadonlyMap<number, number>;
  }>();
  const widths = observed?.request === request ? observed.widths : undefined;
  const ready = requests.every((_, index) => widths?.has(index));
  const nodes: ReactNode = (
    <Resources>
      {request.values.map(([text, size], index) => (
        <TextMeasurement
          key={`${signature}/${index}`}
          text={text}
          size={size}
          font={request.font}
          report={(width) =>
            setObserved((previous) => {
              const values = new Map(
                previous?.request === request ? previous.widths : [],
              );
              if (
                values.get(index) === width &&
                values.has(index) === (width !== undefined)
              )
                return previous;
              if (width === undefined) values.delete(index);
              else values.set(index, width);
              return { request, widths: values };
            })
          }
        />
      ))}
    </Resources>
  );
  return {
    nodes,
    ready,
    /** Room for a measured line, with a small layout-rounding margin. */
    lineWidth(text: string, size: number): number {
      return this.width(text, size) + size / 100;
    },
    /** Only read for a declared request; zero is provisional while pending. */
    width(text: string, size: number): number {
      const index = request.values.findIndex(
        ([value, scale]) => value === text && scale === size,
      );
      if (index < 0) throw new Error("Text measurement was not requested");
      return widths?.get(index) ?? 0;
    },
  };
}

function TextMeasurement({
  text,
  size,
  font,
  report,
}: {
  readonly text: string;
  readonly size: number;
  readonly font: string | AssetReference;
  readonly report: (width: number | undefined) => void;
}) {
  const id = useEntityId();
  const width = useField(id, "CanvasBounds", "width");
  const height = useField(id, "CanvasBounds", "height");
  if (width.status === "failed" || height.status === "failed")
    throw (
      width.error ??
      height.error ??
      new Error("Runtime text measurement failed")
    );
  const measured =
    width.status === "ready" &&
    height.status === "ready" &&
    typeof width.value === "number" &&
    typeof height.value === "number" &&
    height.value > 0
      ? width.value
      : undefined;
  useEffect(() => {
    report(measured);
  }, [measured, report]);
  return (
    <Entity id={id}>
      <PresentationGate open />
      <Style opacity={0} />
      <Text text={text} source={font} font_size={size} />
    </Entity>
  );
}
