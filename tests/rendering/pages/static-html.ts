import type {
  Client,
  HostClientBase,
  PresentationView,
  HtmlSource,
} from "@ipp/client";
import { registerHtml } from "../../../packages/ipp-client/src/html-content.js";
import { canvasOutput } from "../../../packages/ipp-client/src/references.js";
import { presentationTesting } from "../../../packages/ipp-client/src/testing.js";
import {
  createEntity,
  insertComponent,
  componentFields,
  aliasId,
  successfulBatch,
} from "../../fixtures/commands.js";
import { CANVAS, selectSystems } from "../../fixtures/system-selections.js";

let host: HostClientBase<Client>;
let client: Client;
let view: PresentationView;
let canvas: HTMLCanvasElement;
let source: HtmlSource;
let entity: bigint;
const errors: string[] = [];

function content(green: boolean) {
  const root = document.createElement("div");
  root.style.cssText = `position:relative;background:${green ? "rgba(0,255,0,.5)" : "rgba(255,0,0,.5)"}`;
  const marker = document.createElement("div");
  marker.style.cssText = `position:absolute;left:0;top:0;width:24px;height:16px;background:${green ? "#ff0000" : "#00ff00"}`;
  root.append(marker);
  const detail = document.createElement("div");
  detail.style.cssText =
    "position:absolute;left:48px;top:0;width:40px;height:16px;background:repeating-linear-gradient(to right,#000 0 .5px,#fff .5px 1px)";
  root.append(detail);
  return root;
}

export async function create(configuration: {
  generated: string;
  worker: string;
  wasm: string;
  zoom: number;
}) {
  canvas = document.createElement("canvas");
  canvas.width = Math.round(96 * devicePixelRatio);
  canvas.height = Math.round(64 * devicePixelRatio);
  canvas.style.width = "96px";
  canvas.style.height = "64px";
  canvas.style.zoom = String(configuration.zoom);
  document.body.append(canvas);
  const module = await import(configuration.generated);
  host = await module.IppHostClient.connectWorker(
    configuration.worker,
    configuration.wasm,
    { canvas: canvas.transferControlToOffscreen(), htmlCanvas: canvas },
  );
  const world = await host.createWorld({
    selectedSystems: selectSystems(CANVAS),
    canvas: { extent: [96, 64], unitsPerMetre: 1 },
  });
  client = await host.openWorld(world.reference);
  source = await registerHtml(host, content(false), (error) =>
    errors.push(error.message),
  );
  const outcome = successfulBatch(
    await client.batch([
      createEntity(1, "background"),
      insertComponent(
        client,
        "CanvasBox",
        { kind: "alias", alias: 1 },
        { width: 96, height: 64 },
      ),
      insertComponent(
        client,
        "CanvasStyle",
        { kind: "alias", alias: 1 },
        { red: 0, green: 0, blue: 1 },
      ),
      createEntity(2, "html"),
      insertComponent(
        client,
        "CanvasHtml",
        { kind: "alias", alias: 2 },
        { source: source.source, width: 96, height: 64 },
      ),
    ]),
  );
  entity = aliasId(outcome, 2);
  const binding = await host.setRootOutput(canvasOutput(world.reference), {
    width: canvas.width,
    height: canvas.height,
    devicePixelRatio,
  });
  view = await host.presentation.select(
    await host.presentation.surface(),
    binding,
  );
}

async function fields(values: Record<string, string | number>) {
  successfulBatch(
    await client.batch(
      componentFields(client, "CanvasHtml", values).map((field) => ({
        kind: "setField",
        entity: { kind: "handle", id: entity },
        component: client.components.CanvasHtml!.id,
        field,
      })),
    ),
  );
}

export async function replace() {
  const previous = source;
  source = await registerHtml(host, content(true), (error) =>
    errors.push(error.message),
  );
  previous.release();
  await fields({ source: source.source });
}

export async function resize() {
  await fields({ width: 63, height: 31 });
}

export async function recover() {
  const testing = presentationTesting(host);
  testing.loseContext();
  testing.restoreContext();
  const before = view.surface.context;
  const deadline = performance.now() + 10_000;
  for (;;) {
    const surface = await host.presentation.surface().catch(() => undefined);
    if (surface && surface.context !== before) {
      view = await host.presentation.select(surface, view.binding);
      return;
    }
    if (performance.now() > deadline)
      throw new Error("HTML context recovery timed out");
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
}

export function withdraw() {
  source.release();
}

export async function capture() {
  const frame = await host.presentation.capture(view);
  return {
    width: canvas.width,
    height: canvas.height,
    pixelRatio: devicePixelRatio,
    pixels: Array.from(new Uint8Array(frame.pixels)),
    errors: [...errors],
  };
}

export async function close() {
  source?.release();
  if (client) {
    const world = client.worldReference!;
    await client.close();
    await host.destroyWorld(world);
  }
  await host?.close();
  canvas?.remove();
  return document.querySelectorAll("canvas > [drawable]").length;
}
