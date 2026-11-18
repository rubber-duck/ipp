import {
  Children,
  Entity,
  PresentationGate,
} from "../../../../packages/ipp-react/src/index.js";
import { Layout, Style } from "../../../../packages/ipp-react/src/gui.js";
import {
  Drawer,
  Menubar,
  TextLine,
  Rating,
  Resizable,
  TokenInput,
} from "../../../../packages/ipp-react/src/gui-kit.js";

/** One ordinary GUI subtree whose dimensions stay fixed in local units. */
export function TransformedControls({
  scale,
  report,
  drawer,
}: {
  scale: number;
  drawer: boolean;
  report: (event: string) => void;
}) {
  if (drawer)
    return (
      <Entity id="drawer-anchor">
        <PresentationGate open />
        <Layout kind={2} width={10} height={100} />
        <Style x={150} y={42} scale_x={scale} />
        <Children>
          <Drawer
            id="scaled-drawer"
            label="Inspector"
            mode="light"
            defaultOpen
            edge="right"
            snapPoints={[20, 40, 60]}
            onSnapChange={(index) => report("snap/" + index)}
          />
        </Children>
      </Entity>
    );
  return (
    <Entity id="scaled-controls">
      <PresentationGate open />
      <Layout kind={2} width={172} height={185} gap={3} />
      <Style x={6} y={5} scale_x={scale} scale_y={1.25} />
      <Children>
        <Rating
          id="scaled-rating"
          label="Quality"
          onChange={(n) => report("rating/" + n)}
        />
        <Resizable
          id="scaled-split"
          width={150}
          height={30}
          layout={{ padding_left: 8, padding_right: 6 }}
          panes={[
            { key: "a", content: <TextLine text="Left" /> },
            { key: "b", content: <TextLine text="Right" /> },
          ]}
          onSizesChange={(value) => report("pane/" + value.a)}
        />
        <TokenInput
          id="scaled-token"
          width={60}
          label="Tokens"
          defaultValue={[{ kind: "text", text: "abcdefghijklmnopqrstuvwx" }]}
          onSelectionChange={(selection) =>
            report("token/" + selection.join("/"))
          }
        />
        <Menubar
          id="scaled-menus"
          label="Commands"
          menus={[
            {
              key: "file",
              label: "File",
              items: [{ key: "save", label: "Save" }],
            },
            {
              key: "edit",
              label: "Edit",
              items: [{ key: "copy", label: "Copy" }],
            },
          ]}
        />
      </Children>
    </Entity>
  );
}
