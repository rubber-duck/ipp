import type { ReactNode } from "react";
import { Children, Entity } from "@ipp/react";
import { Landscape } from "./landscape.js";
import { Layout } from "@ipp/react/gui";
import { CartoonTitle } from "@ipp/react/gui-cartoon";
import {
  Button,
  Dropdown,
  Panel,
  ScrollArea,
  TextLine,
  useGuiKit,
} from "@ipp/react/gui-kit";
import {
  Actions,
  Choices,
  Colours,
  GameSettings,
  Values,
  type Settings,
} from "./inputs.js";
import { Inventory, Navigation } from "./collections.js";
import { Feedback, Overlays } from "./feedback.js";

export const sections = [
  {
    key: "settings",
    height: [275, 275],
    label: "Game settings",
    title: "Game settings",
  },
  {
    key: "actions",
    height: [375, 565],
    label: "Buttons and fields",
    title: "Runner profile",
  },
  {
    key: "choices",
    height: [383, 780],
    label: "Choice controls",
    title: "Plan your route",
  },
  {
    key: "values",
    height: [345, 590],
    label: "Sliders and dials",
    title: "Trail controls",
  },
  {
    key: "colour",
    height: [419, 560],
    label: "Colour picker",
    title: "Trail marker",
  },
  {
    key: "navigation",

    label: "Navigation",
    title: "Field journal",
  },
  {
    key: "inventory",
    height: [610, 610],
    label: "Windows and lists",
    title: "Camp supplies",
  },
  {
    key: "feedback",
    height: [418, 420],
    label: "Progress and alerts",
    title: "Expedition progress",
  },
  {
    key: "overlays",
    height: [450, 465],
    label: "Menus and dialogs",
    title: "Waypoints",
  },
] as const;
export type Section = (typeof sections)[number]["key"];

function at(x: number, y: number) {
  return { align_x: -1, align_y: -1, margin_left: x, margin_top: y };
}
export function Showcase({
  width,
  height,
  section,
  select,
  saved,
  save,
}: {
  width: number;
  height: number;
  section: Section;
  select: (section: Section) => void;
  saved: Settings;
  save: (settings: Settings) => void;
}) {
  const kit = useGuiKit();
  const narrow = width < 600;
  const margin = narrow ? 12 : 30;
  const headerHeight = narrow ? 124 : 108;
  const index = sections.findIndex((entry) => entry.key === section);
  const controlWidth = Math.min(680, width - margin * 2);
  const left = (width - controlWidth) / 2;
  const sectionInfo = sections[index]!;
  const availableHeight = height - headerHeight - margin;
  const sizing =
    "height" in sectionInfo
      ? {
          height: Math.min(
            availableHeight,
            kit.unit(sectionInfo.height[controlWidth >= 620 ? 0 : 1]),
          ),
          fitHeight: false,
        }
      : { maxHeight: availableHeight, fitHeight: true };
  const top = narrow ? 68 : 35;
  const navWidth = narrow ? controlWidth : Math.min(360, controlWidth - 235);
  const navLeft = narrow ? left : left + controlWidth - navWidth;
  const arrowWidth = kit.unit(44);
  const pages: Record<Section, ReactNode> = {
    settings: <GameSettings saved={saved} save={save} />,
    actions: <Actions wide={controlWidth >= 620} />,
    choices: <Choices wide={controlWidth >= 620} />,
    values: <Values wide={controlWidth >= 620} />,
    colour: <Colours wide={controlWidth >= 620} />,
    navigation: <Navigation />,
    inventory: <Inventory />,
    feedback: (
      <Feedback wide={controlWidth >= 620} maxToastWidth={controlWidth} />
    ),
    overlays: <Overlays />,
  };
  return (
    <Entity id="gallery">
      <Layout
        kind={3}
        width={width}
        height={height}
        align_x={-1}
        align_y={-1}
      />
      <Children>
        <Landscape width={width} height={height} />
        <CartoonTitle
          id="trail-title"
          text="TRAIL RUN"
          fontSize={narrow ? 39 : 47}
          layout={at(left, narrow ? 4 : 9)}
        />
        {!narrow && (
          <TextLine
            id="gallery-caption"
            text="Control gallery"
            layout={at(left + 3, 70)}
            size="small"
          />
        )}
        <Dropdown
          id="section"
          width={navWidth - 2 * arrowWidth - 14}
          label="Control gallery section"
          options={sections}
          value={section}
          onChange={(key) => {
            const page = sections.find((entry) => entry.key === key);
            if (page) select(page.key);
          }}
          layout={at(navLeft, top)}
        />
        <Button
          id="previous"
          label="‹"
          semanticLabel="Previous section"
          variant="secondary"
          width={arrowWidth}
          layout={at(navLeft + navWidth - 2 * arrowWidth - 7, top)}
          onPress={() =>
            select(
              sections[(index + sections.length - 1) % sections.length]!.key,
            )
          }
        />
        <Button
          id="next"
          label="›"
          semanticLabel="Next section"
          variant="secondary"
          width={arrowWidth}
          layout={at(navLeft + navWidth - arrowWidth, top)}
          onPress={() => select(sections[(index + 1) % sections.length]!.key)}
        />
        <Panel
          id="page"
          width={controlWidth}
          {...sizing}
          layout={at(left, headerHeight)}
        >
          <ScrollArea
            key={section}
            id={`page-${section}`}
            padding="inset"
            {...sizing}
            gap="label"
            framed={false}
          >
            {section !== "settings" && (
              <TextLine text={sections[index]!.title} size="display" />
            )}
            {pages[section]}
          </ScrollArea>
        </Panel>
      </Children>
    </Entity>
  );
}
