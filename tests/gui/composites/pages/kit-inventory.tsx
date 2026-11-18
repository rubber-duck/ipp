import { createGuiAgent } from "@ipp/client";
import {
  createRoot,
  type ReactWorldClient,
} from "../../../../packages/ipp-react/src/index.js";
import { cybertronSkin } from "@ipp/react/gui-cybertron";
import { glassSkin } from "@ipp/react/gui-glass";
import {
  Preferences,
  createGuiPreferences,
  browserMotionPreference,
} from "../../../../packages/ipp-react/src/gui-kit.js";
import {
  ComponentGallery,
  type GallerySection,
} from "../../fixtures/component-gallery.js";
import {
  canvasOf,
  componentFields,
  openCompositePage,
  symbols,
  type CompositeSetup,
  type PanelSpec,
} from "./composites.js";
export { nativePresentationTransport, workerTransport } from "./composites.js";

const panel = {
  name: "inventory",
  origin: [0, 0],
  extent: [380, 540],
  async build({ client, font, contract, report }) {
    const root = createRoot(client as unknown as ReactWorldClient, {
      onError: report,
    });
    const events: string[] = [];
    const skins = {
      cybertron: cybertronSkin(contract),
      glass: glassSkin(contract),
    };
    const settings = createGuiPreferences(
      { skin: "glass", reducedMotion: "full" },
      skins,
      {
        read: () => ({ skin: "cybertron", reducedMotion: "reduce" }),
        write: () => {},
      },
    );
    const platform = browserMotionPreference(window);
    const draw = (section: GallerySection) =>
      root.render(
        <Preferences
          id="specimen"
          contract={contract}
          font={font}
          fontSize={9}
          settings={settings}
          platformMotion={platform}
        >
          <ComponentGallery
            key={section}
            section={section}
            change={(v) => events.push(v)}
          />
        </Preferences>,
      );
    await draw("selection");
    return { draw, events, settings, close: () => root.unmount() };
  },
} satisfies PanelSpec;
export const CANVAS = canvasOf([panel]);
export async function prepare(setup: CompositeSetup) {
  const page = await openCompositePage(setup, [panel]);
  const instance = page.panel("inventory"),
    client = instance.client;
  const kit = instance.build as Awaited<ReturnType<typeof panel.build>>;
  const agent = createGuiAgent({ gallery: client });
  async function idOf(name: string) {
    let id: bigint | undefined;
    await page.until(
      async () => {
        id = (await symbols(client)).get(name);
        return id !== undefined;
      },
      () => "Missing " + name,
    );
    return id!;
  }
  async function fields(name: string, component: string) {
    const id = await idOf(name),
      snapshot = await client.inspectPage({
        collection: "entities",
        target: id,
        limit: 1,
      });
    return componentFields(client, snapshot, id, component);
  }
  return {
    ...page.steps,
    draw: kit.draw,
    fullMotion: () =>
      kit.settings.set({ skin: "glass", reducedMotion: "full" }),
    followPlatform: () =>
      kit.settings.set({ skin: "glass", reducedMotion: "system" }),
    skin: () => kit.settings.set({ skin: "glass", reducedMotion: "reduce" }),
    async itemBounds(name: string) {
      let box: Readonly<Record<string, unknown>> | undefined;
      await page.until(
        async () => {
          box = await fields(name, "CanvasBounds");
          return Number(box?.width) > 0 && Number(box?.height) > 0;
        },
        () => name + " bounds " + JSON.stringify(box),
      );
      return [
        Number(box!.x),
        Number(box!.y),
        Number(box!.width),
        Number(box!.height),
      ];
    },
    async event(value: string) {
      await page.until(
        () => kit.events.includes(value),
        () => value + " in " + JSON.stringify(kit.events),
      );
    },
    async exists(name: string, present: boolean) {
      await page.until(
        async () => (await symbols(client)).has(name) === present,
        () => name + " present=" + present,
      );
    },
    async field(
      name: string,
      component: string,
      field: string,
      expected: string | number | boolean,
    ) {
      let last: unknown;
      await page.until(
        async () => {
          last = (await fields(name, component))?.[field];
          return last === expected;
        },
        () => name + "." + field + " was " + String(last),
      );
    },
    async identity(name: string) {
      return String(await idOf(name));
    },
    async meter() {
      return (await agent.query({ kind: "meter", label: "Load" })).controls.map(
        (item) => ({ state: item.state, actions: item.actions }),
      );
    },
    async native(name: string) {
      return page.expectNativeBuffer(await idOf(name));
    },
    async externalMotion(reducedMotion: boolean) {
      client.sendCommand({
        type: "GuiPreferencesUpdateCommand",
        reducedMotion,
      });
    },
  };
}
