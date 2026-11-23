import { Resources } from "@ipp/react";
import { useState } from "react";
import {
  Button,
  Column,
  ContextMenu,
  DataGrid,
  EmptyState,
  Expander,
  Panel,
  PanelBody,
  PanelFooter,
  PanelHeader,
  ScrollArea,
  Stepper,
  Tabs,
  TextLine,
  TreeView,
  WindowControls,
  useGuiKit,
  useContextMenu,
} from "@ipp/react/gui-kit";

export function Navigation() {
  const kit = useGuiKit();
  const contentHeight = kit.unit(kit.tokens.controlHeight);
  const [done, finish] = useState(false);
  return (
    <Column gap="field">
      <Tabs
        id="journal-tabs"
        height={kit.unit(84)}
        defaultValue="map"
        tabs={[
          {
            value: "map",
            label: "Map",
            content: <TextLine text="Pine forest · West trail" />,
          },
          {
            value: "runner",
            label: "Runner",
            content: <TextLine text="Pip · Scout" />,
          },
          {
            value: "gear",
            label: "Gear",
            content: <TextLine text="Compass and lantern" />,
          },
        ]}
      />
      <Stepper
        id="camp-steps"
        defaultValue="pack"
        steps={[
          {
            value: "pack",
            label: "Pack",
            content: (
              <TextLine
                height={contentHeight}
                text="Pack the lantern before leaving camp."
              />
            ),
          },
          {
            value: "walk",
            label: "Hike",
            content: (
              <TextLine
                height={contentHeight}
                text="Follow the green trail markers."
              />
            ),
          },
          {
            value: "arrive",
            label: "Arrive",
            content: (
              <Button
                id="check-in"
                label={done ? "Checked in" : "Check in"}
                disabled={done}
                fitWidth
                onPress={() => finish(true)}
              />
            ),
          },
        ]}
      />
      <Expander id="world-map" fitHeight label="World tree" defaultExpanded>
        <TreeView
          id="route-tree"
          height={kit.unit(4 * kit.tokens.row + kit.tokens.inset)}
          layout={{ margin_bottom: kit.spacing.compact }}
          defaultValue="bridge"
          defaultExpanded={["forest"]}
          nodes={[
            {
              key: "forest",
              label: "Pine forest",
              children: [
                { key: "bridge", label: "Rope bridge" },
                { key: "camp", label: "Campfire" },
              ],
            },
            {
              key: "cave",
              label: "Crystal cave",
              children: [{ key: "lake", label: "Underground lake" }],
            },
          ]}
        />
      </Expander>
    </Column>
  );
}

const supplies = [
  ["Pinecones", "Forage", 12],
  ["Camp lantern", "Gear", 1],
  ["Brass compass", "Gear", 1],
  ["Climbing rope", "Tools", 2],
  ["Trail boots", "Clothing", 1],
  ["Forest map", "Maps", 1],
  ["Wild berries", "Forage", 8],
  ["Tent stakes", "Tools", 6],
  ["Canteen", "Gear", 2],
  ["Sun cap", "Clothing", 1],
  ["Dry kindling", "Forage", 9],
  ["Field journal", "Gear", 1],
] as const;
const columns = [
  { key: "name", title: "Item", flex: 2.1 },
  { key: "kind", title: "Type", flex: 1.15 },
  { key: "count", title: "Qty", width: 62 },
];
const gridRow = (i: number) => {
  const [name, kind, quantity] = supplies[i]!;
  return { key: String(i), cells: { name, kind, count: String(quantity) } };
};

export function Inventory() {
  const kit = useGuiKit();
  const [selection, setSelection] = useState("0");
  const [visible, show] = useState(true);
  const [minimized, minimize] = useState(false);
  const [expanded, expand] = useState(false);
  const [message, notify] = useState("");
  const context = useContextMenu<string>();
  return (
    <Column gap="label">
      {visible ? (
        <Panel id="inventory-window" minimized={minimized}>
          <PanelHeader title="Scout’s backpack">
            <WindowControls
              onMinimize={() => minimize(!minimized)}
              {...(expanded
                ? { onRestore: () => expand(false) }
                : {
                    onMaximize: () => {
                      minimize(false);
                      expand(true);
                    },
                  })}
              onClose={() => show(false)}
            />
          </PanelHeader>
          {!minimized && (
            <PanelBody padding="label" gap={0}>
              <DataGrid
                id="supplies"
                columns={columns}
                rowCount={supplies.length}
                row={gridRow}
                height={kit.unit(
                  (expanded ? 8 : 5) *
                    (kit.tokens.gridRowHeight ?? kit.tokens.row),
                )}
                selected={selection}
                onRowPress={(key) => {
                  setSelection(key);
                  notify("");
                }}
                onRowContextMenu={(key, event) => context.opener(key)(event)}
              />
            </PanelBody>
          )}
          {!minimized && (
            <PanelFooter
              leading={
                <TextLine
                  size="small"
                  text={
                    message ||
                    `Selected · ${gridRow(Number(selection)).cells.name}`
                  }
                />
              }
            >
              <TextLine
                size="small"
                tone="neutral"
                text={`${supplies.length} items`}
              />
            </PanelFooter>
          )}
        </Panel>
      ) : (
        <Column gap="label">
          <EmptyState text="Inventory window closed" />
          <Button
            id="reopen-inventory"
            label="Open inventory"
            onPress={() => show(true)}
            fitWidth
          />
        </Column>
      )}
      <Expander id="trail-log" label="Trail log" fitHeight defaultExpanded>
        <Column
          id="trail-log-inset"
          padding={kit.unit(16)}
          layout={{ padding_right: 0 }}
        >
          <ScrollArea
            id="log-scroll"
            height={kit.unit(3 * kit.tokens.denseRow + 12)}
            fitHeight={false}
            padding={0}
            gap={kit.unit(6)}
            framed={false}
          >
            {[
              "Left camp",
              "Crossed rope bridge",
              "Found lantern",
              "Reached lookout",
              "Returned to camp",
            ].map((item) => (
              <TextLine
                key={item}
                height={kit.unit(kit.tokens.denseRow)}
                text={item}
              />
            ))}
          </ScrollArea>
        </Column>
      </Expander>
      <Resources>
        <ContextMenu
          id="item-menu"
          menu={context}
          items={[
            { key: "inspect", label: "Inspect item" },
            { key: "pin", label: "Pin item" },
          ]}
          onSelect={(key, target) =>
            notify(
              `${key === "pin" ? "Pinned" : "Inspected"} ${gridRow(Number(target)).cells.name}`,
            )
          }
        />
      </Resources>
    </Column>
  );
}
