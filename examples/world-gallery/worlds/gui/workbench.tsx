/**
 * The right column's tabbed panel. Tabs organise the station's tools in the
 * panel's height instead of growing the dashboard: NODES (FIND and the node
 * grid), CONTROLS (the projection's value and selection controls) and COLOUR
 * (the projection's colour), above the footer's node count, SYNC and PURGE.
 * Only the selected tab's content is declared; what the tabs set lives in
 * the station and the tuning, so it stays while another tab shows.
 */
import { Panel, Tabs } from "@ipp/react/gui-kit";
import { ColourTab } from "./colour-tab.js";
import { NodesFooter, NodesTab, type NodeMenu } from "./nodes.js";
import { WORKBENCH_HEIGHT } from "./presentation.js";
import type { GuiSceneState, WorkbenchTab } from "./scene.js";
import { TuningTab } from "./tuning-tab.js";

export const WORKBENCH_TABS = "gui-workbench";

export function Workbench({
  scene,
  menu,
}: {
  readonly scene: GuiSceneState;
  readonly menu: NodeMenu;
}) {
  return (
    <Panel id="gui-workbench-panel" layout={{ height: WORKBENCH_HEIGHT }}>
      <Tabs
        id={WORKBENCH_TABS}
        tabs={[
          {
            value: "nodes",
            label: "NODES",
            content: <NodesTab scene={scene} menu={menu} />,
          },
          {
            value: "controls",
            label: "CONTROLS",
            content: <TuningTab scene={scene} />,
          },
          {
            value: "colour",
            label: "COLOUR",
            content: <ColourTab scene={scene} />,
          },
        ]}
        defaultValue="nodes"
        onChange={(value) => scene.setWorkbenchTab(value as WorkbenchTab)}
        layout={{ flex: 1 }}
      />
      <NodesFooter scene={scene} />
    </Panel>
  );
}
