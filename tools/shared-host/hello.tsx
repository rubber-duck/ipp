/**
 * The smallest shared-Host client module: one canvas World with a panel and
 * a line of text, captured as `hello.png`. Copy it to start a client.
 *
 *   node tools/shared-host/shared-host.mjs run tools/shared-host/hello.tsx "Some text"
 */
import { canvasOutput, type Client, type RootBinding } from "@ipp/client";
import {
  Asset,
  Children,
  Entity,
  assetRef,
  createRoot,
  type ReactWorldClient,
  type ReactWorldRoot,
} from "@ipp/react";
import { Box, Layout, Style, Text } from "@ipp/react/gui";
import { defineClient, type ClientContext } from "./client.js";

const EXTENT = [320, 96] as const;

interface Hello {
  readonly world: Awaited<ReturnType<ClientContext["host"]["createWorld"]>>;
  readonly client: Client;
  readonly root: ReactWorldRoot;
  readonly binding: RootBinding;
  readonly font: Uint8Array<ArrayBuffer>;
}

function scene(font: Uint8Array<ArrayBuffer>, message: string) {
  return (
    <>
      <Asset
        id="font"
        kind={17}
        data={font}
        encode={(bytes: Uint8Array<ArrayBuffer>) => bytes}
      />
      <Entity id="page">
        <Layout
          kind={3}
          width={EXTENT[0]}
          height={EXTENT[1]}
          align_x={-1}
          align_y={-1}
        />
        <Children>
          <Entity id="panel">
            <Layout
              width={300}
              height={76}
              margin_left={10}
              margin_top={10}
              align_x={-1}
              align_y={-1}
            />
            <Style red={0.01} green={0.05} blue={0.1} />
            <Box width={300} height={76} radius_x={6} radius_y={6} />
          </Entity>
          <Entity id="message">
            <Layout
              margin_left={24}
              margin_top={36}
              align_x={-1}
              align_y={-1}
            />
            <Style red={0.5} green={0.9} blue={1} />
            <Text text={message} source={assetRef("font")} font_size={20} />
          </Entity>
        </Children>
      </Entity>
    </>
  );
}

/** Text appears only once the font is loaded, so wait before capturing. */
async function fontLoaded(root: ReactWorldRoot): Promise<void> {
  const deadline = performance.now() + 30_000;
  while (root.getAsset("font")?.status !== "loaded") {
    if (performance.now() > deadline) throw new Error("The font did not load");
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

export default defineClient<Hello>({
  async open(context) {
    // A temporary World ends with this client's connection.
    const world = await context.host.createWorld({
      symbolicId: `hello/${context.name}`,
      temporary: true,
      selectedSystems: [
        "ipp.animation",
        "ipp.gui",
        "ipp.gui-layout",
        "ipp.canvas",
        "ipp.asset-dependencies",
        "ipp.lifecycle-publisher",
      ],
    });
    const client = await context.host.openWorld(world.reference);
    const root = createRoot(client as unknown as ReactWorldClient);
    const font = await context.font();
    await root.render(scene(font, "Hello from the shared Host"));
    // Its own root binding; the shared surface selects it only while capturing.
    const binding = await context.host.setRootOutput(
      canvasOutput(world.reference),
      { width: EXTENT[0], height: EXTENT[1], devicePixelRatio: 1 },
    );
    return { world, client, root, binding, font };
  },

  async capture(state, context, args) {
    // Rendering again applies edits of this module in a live session.
    await state.root.render(
      scene(state.font, args.join(" ") || "Hello from the shared Host"),
    );
    await fontLoaded(state.root);
    return {
      images: { hello: await context.capture(state.binding) },
      summary: [`World ${state.world.symbolicId}`],
    };
  },

  async close(state, context) {
    await state.root.unmount();
    await state.client.close();
    await context.host.destroyWorld(state.world.reference);
  },
});
