import type { DemoApplication } from "../../../examples/demos/application.js";

/** Observe native font sources and evaluated geometry without client-side metrics. */
export async function applicationFonts(app: DemoApplication) {
  await app.flush();
  const deadline = performance.now() + 30_000;
  for (;;) {
    await app.sceneCanvas!.frame();
    const { shell } = await app.inspectApplication();
    const fonts = shell.resources.filter((resource) => resource.kind === 17);
    const failed = fonts.find((resource) => resource.status === "failed");
    if (failed) throw new Error(`Font failed to load: ${failed.source}`);
    if (fonts.length && fonts.every((resource) => resource.status === "loaded"))
      break;
    if (performance.now() > deadline)
      throw new Error("Application fonts did not load");
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
  // Publish layout and glyphs after resource completion before measuring or capturing.
  await app.sceneCanvas!.frame();
  const state = await app.inspectApplication();
  const components = app.sceneClient!.components;
  const entity = (id: string) => {
    const value = state.shell.entities.find(
      (item) => item.metadata.symbolicId === id,
    );
    if (!value) throw new Error(`Missing application entity ${id}`);
    return value;
  };
  const button = entity("demo-controls-toggle");
  const fields = (id: string, component: number) =>
    entity(id).components.find((item) => item.component === component)!.fields;
  return {
    options: state.options,
    root: state.cameraPath[0]!.parent,
    sceneWorlds: state.sceneWorlds,
    button: button.id,
    source: fields("demo-controls-toggle", components.GuiFont!.id).source,
    width: Number(
      fields("demo-controls-toggle", components.CanvasBounds!.id).width,
    ),
    icon: fields("demo-display/caret", components.CanvasText!.id).source,
  };
}

export type ApplicationFontsState = Awaited<
  ReturnType<typeof applicationFonts>
>;
