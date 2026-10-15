import {
  ScrollView,
  VirtualList,
  type ScrollViewProps,
  type VirtualListProps,
} from "../gui/scroll.js";
import { Skin } from "../gui/theme.js";
import { useGuiKit } from "../gui-kit/kit.js";

/** Glass appearance on the caller's Entity; native scrolling owns every value and hit. */
export function GlassScrollView(props: ScrollViewProps) {
  const theme = useGuiKit().controlTheme("scroll");
  return (
    <>
      {theme && <Skin theme={theme} />}
      <ScrollView {...props} />
    </>
  );
}

/** Realization still uses VirtualList's published native range and stable item keys. */
export function GlassVirtualList(props: VirtualListProps) {
  const theme = useGuiKit().controlTheme("scroll");
  return (
    <>
      {theme && <Skin theme={theme} />}
      <VirtualList {...props} />
    </>
  );
}
