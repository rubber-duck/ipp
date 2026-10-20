import { Children, Entity } from "@ipp/react";
import { Behavior, Button, Font, Layout, Skin } from "@ipp/react/gui";
import { GUI_KIT_ICONS, useGuiKit } from "@ipp/react/gui-kit";

/** Two independent actions in one segmented frame, not a mode selection. */
export function BrowserNavigation({
  back,
  forward,
}: {
  back?: (() => void) | undefined;
  forward?: (() => void) | undefined;
}) {
  const kit = useGuiKit();
  const height = kit.unit(kit.tokens.controlHeight);
  return (
    <Entity id="cockpit-browser-navigation">
      <Layout kind={1} width={height * 2} height={height} />
      <Font source={kit.font} font_size={kit.fontSize} />
      <Skin theme={kit.theme("frame")} />
      <Children>
        {(
          [
            {
              id: "back",
              icon: GUI_KIT_ICONS.previous,
              action: back,
              theme: "segmentFirst",
              label: "Back",
            },
            {
              id: "forward",
              icon: GUI_KIT_ICONS.next,
              action: forward,
              theme: "segmentLast",
              label: "Forward",
            },
          ] as const
        ).map((item) => (
          <Entity key={item.id} id={`cockpit-browser-${item.id}`}>
            <Layout width={height} height={height} />
            <Skin theme={kit.theme(item.theme)} />
            <Behavior enabled={!!item.action} semantic_label={item.label} />
            <Button label={item.icon} onPress={() => item.action?.()} />
          </Entity>
        ))}
      </Children>
    </Entity>
  );
}
