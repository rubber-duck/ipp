import { Children, Entity, PresentationGate } from "../../components.js";
import {
  Behavior,
  Bounds,
  Layout,
  Measurement,
  Style,
} from "../../gui/components.js";
import { Skin } from "../../gui/theme.js";
import { useEntityId } from "../../scope/identity.js";
import { useField } from "../../scope/field.js";
import { useGuiKit } from "../kit.js";
import {
  KitLayout,
  Row,
  useGuiKitLayout,
  type GuiKitLayoutProps,
} from "../layout.js";
import { TextLine } from "../text.js";

export interface MeterProps extends GuiKitLayoutProps {
  readonly id?: string;
  readonly label: string;
  readonly value: number;
  readonly min?: number;
  readonly max?: number;
  readonly low?: number;
  readonly high?: number;
  readonly optimum?: number;
  readonly formatValue?: (value: number) => string;
  readonly layer?: number;
}

function useMeasurement(id: string, field: string, fallback: number) {
  const observed = useField(id, "GuiMeter", field);
  return observed.status === "ready" && typeof observed.value === "number"
    ? observed.value
    : fallback;
}

/** Thresholds classify the same bounded measurement used by the readout and fill. */
export function Meter(props: MeterProps) {
  const {
    id: providedId,
    label,
    value,
    min = 0,
    max = 1,
    low = min,
    high = max,
    optimum = (min + max) / 2,
    formatValue = String,
    layer = 0,
    layout,
  } = useGuiKitLayout(props);
  const id = useEntityId(providedId),
    kit = useGuiKit();
  if (
    ![min, max, value, low, high, optimum].every((n) =>
      Number.isFinite(Math.fround(n)),
    ) ||
    Math.fround(min) >= Math.fround(max) ||
    low < min ||
    high > max ||
    low > high ||
    optimum < min ||
    optimum > max
  )
    throw new RangeError("Invalid meter range or thresholds");
  const bounded = Math.max(min, Math.min(max, value));
  const start = useMeasurement(id, "min", min),
    end = useMeasurement(id, "max", max),
    measured = useMeasurement(id, "value", bounded);
  const fraction =
    Math.max(0, Math.min(1, (measured - start) / (end - start))) || 0;
  const band = (v: number) => (v < low ? -1 : v > high ? 1 : 0);
  const distance = Math.abs(band(measured) - band(optimum));
  return (
    <Entity id={id}>
      <PresentationGate open />
      <Style layer={layer} />
      <KitLayout kind={2} fit_height {...layout} />
      <Measurement min={min} max={max} value={bounded} />
      <Behavior semantic_label={label} focusable={false} />
      <Children>
        <Row>
          <TextLine text={label} flex={1} />
          <TextLine id={`${id}/readout`} text={formatValue(measured)} />
        </Row>
        <Entity id={`${id}/track`}>
          <Layout kind={1} height={kit.unit(kit.tokens.smallHeight)} />
          <Skin theme={kit.theme("progressFrame")} />
          <Children>
            {fraction > 0 && (
              <Entity id={`${id}/fill`}>
                <Bounds />
                <Layout
                  flex={fraction}
                  height={kit.unit(kit.tokens.smallHeight)}
                />
                <Skin
                  theme={kit.theme(
                    distance === 0
                      ? "valueAccent"
                      : distance === 1
                        ? "valueAmber"
                        : "valueError",
                  )}
                />
              </Entity>
            )}
            {fraction < 1 && (
              <Entity id={`${id}/remainder`}>
                <Layout flex={1 - fraction} />
              </Entity>
            )}
          </Children>
        </Entity>
      </Children>
    </Entity>
  );
}
