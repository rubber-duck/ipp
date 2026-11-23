import { useLayoutEffect, useRef, useState } from "react";
import { Skin } from "../gui/theme.js";
import { useGuiKit } from "./kit.js";

/** Keep the outgoing finish through deselection and vary the next selected finish. */
export function VariantSkin({
  theme,
  selected = false,
}: {
  theme: string;
  selected?: boolean;
}) {
  const kit = useGuiKit();
  const variants = kit.themeVariants?.[theme];
  const [sample, setSample] = useState(Math.random);
  const previous = useRef(selected);
  useLayoutEffect(() => {
    if (!previous.current && selected && variants && variants.length > 1) {
      const count = variants.length;
      const offset = 1 + Math.floor(Math.random() * (count - 1));
      setSample(
        (value) =>
          (((Math.floor(value * count) + offset) % count) + 0.5) / count,
      );
    }
    previous.current = selected;
  }, [selected, variants]);
  return (
    <Skin theme={variants?.[Math.floor(sample * variants.length)] ?? theme} />
  );
}
