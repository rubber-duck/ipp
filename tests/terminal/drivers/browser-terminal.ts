import type { Page } from "playwright";
import type {
  Client,
  RowsTable,
  HostClientBase,
  GuiWorldClient,
  PresentationView,
} from "@ipp/client";
import type { mountTerminal } from "../../../examples/browser-terminal/application.js";

type Application = Awaited<ReturnType<typeof mountTerminal>>;
declare global {
  interface Window {
    terminal: Application;
    terminalErrors: string[];
    terminalGlyphs: Map<number, string>;
  }
}

export class BrowserTerminalDriver {
  readonly errors: string[] = [];
  constructor(readonly page: Page) {
    page.on("pageerror", (error) => this.errors.push(error.message));
  }
  async start(base: string) {
    await this.page.evaluate(async (base) => {
      const canvas = document.createElement("canvas");
      canvas.width = 720;
      canvas.height = 384;
      canvas.tabIndex = 0;
      document.body.style.margin = "0";
      document.body.append(canvas);
      const application = await import(`${base}application.js`);
      window.terminalErrors = [];
      window.terminal = await application.mountTerminal(
        canvas,
        base,
        (error: Error) => window.terminalErrors.push(error.message),
      );
      const font = (await (
        await fetch(`${base}terminal-font.json`)
      ).json()) as { glyphs: Record<string, number> };
      window.terminalGlyphs = new Map(
        Object.entries(font.glyphs).map(([character, id]) => [id, character]),
      );
    }, base);
  }
  async focus() {
    await this.page.locator("canvas").click({ position: { x: 24, y: 24 } });
    await this.page.waitForFunction(
      () => document.activeElement?.tagName === "TEXTAREA",
    );
  }
  type(text: string) {
    return this.page.keyboard.type(text);
  }
  key(key: string) {
    return this.page.keyboard.press(key);
  }
  async paste(text: string) {
    await this.page.evaluate((text) => {
      const data = new DataTransfer();
      data.setData("text/plain", text);
      document.activeElement!.dispatchEvent(
        new ClipboardEvent("paste", {
          clipboardData: data,
          bubbles: true,
          cancelable: true,
        }),
      );
    }, text);
  }
  async compose(text: string) {
    await this.page.evaluate((text) => {
      const input = document.activeElement!;
      input.dispatchEvent(
        new CompositionEvent("compositionstart", { bubbles: true, data: "" }),
      );
      input.dispatchEvent(
        new CompositionEvent("compositionupdate", {
          bubbles: true,
          data: text,
        }),
      );
      input.dispatchEvent(
        new InputEvent("beforeinput", {
          bubbles: true,
          cancelable: true,
          inputType: "insertCompositionText",
          data: text,
          isComposing: true,
        }),
      );
      input.dispatchEvent(
        new CompositionEvent("compositionend", { bubbles: true, data: text }),
      );
    }, text);
  }
  async text() {
    return this.page.evaluate(async () => {
      const client: Client = window.terminal.client;
      const snapshot = await client.inspect();
      return snapshot.entities
        .flatMap((entity) =>
          entity.components
            .filter((c) => c.component === client.components.CanvasGlyphRun!.id)
            .map((c) =>
              [...(c.fields.glyphs as RowsTable).rows.values()]
                .map(
                  (g) => window.terminalGlyphs.get(Number(g.glyph_id)) ?? "?",
                )
                .join(""),
            ),
        )
        .join("\n");
    });
  }
  async capture() {
    return this.page.evaluate(async () => {
      const host: HostClientBase<GuiWorldClient> = window.terminal.host;
      const view: PresentationView = window.terminal.view;
      const frame = await host.presentation.capture(view);
      const bytes = new Uint8Array(frame.pixels);
      let red = 0,
        bright = 0,
        background = 0;
      for (let at = 0; at < bytes.length; at += 4) {
        if (bytes[at]! > bytes[at + 1]! + 50 && bytes[at]! > 160) red++;
        if (bytes[at]! > 160 && bytes[at + 1]! > 160 && bytes[at + 2]! > 160)
          bright++;
        if (bytes[at]! < 60 && bytes[at + 1]! < 60 && bytes[at + 2]! < 75)
          background++;
      }
      return { red, bright, background, sequence: frame.sequence };
    });
  }
  async resize(width: number, height: number) {
    await this.page.evaluate(
      ([w, h]) => window.terminal.resize(w!, h!),
      [width, height],
    );
  }
  async removeTarget() {
    await this.page.evaluate(async () => {
      const { client, target } = window.terminal;
      const result = await client.batch([
        { kind: "deleteSubtree", root: { kind: "handle", id: target } },
      ]);
      if (!result.ok) throw new Error(String(result.error.reason));
    });
  }
  async checkErrors() {
    return [
      ...this.errors,
      ...(await this.page.evaluate(() => window.terminalErrors)),
    ];
  }
  async unfocused() {
    await this.page.waitForFunction(
      () => document.activeElement?.tagName !== "TEXTAREA",
    );
  }
  async close() {
    await this.page.evaluate(() => window.terminal.close());
  }
}
