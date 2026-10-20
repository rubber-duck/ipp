import type { CockpitBrowserDriver } from "../scenarios/cockpit-browser.js";
import type { openApplication } from "./browser-application.js";
import { decodePng } from "../../harness/images.js";

export function cockpitBrowserDriver(
  app: Awaited<ReturnType<typeof openApplication>>,
): CockpitBrowserDriver {
  const press = async (id: string) => {
    const point = await app.cockpitCoordinates(3, `cockpit-browser-${id}`);
    await app.page.mouse.click(point.x, point.y);
    await app.settle();
  };
  const pagePoint = async (id: string, fraction = 0.5) => {
    const point = await app.page.evaluate(
      ({ id, fraction }) => {
        const root = document.querySelector<HTMLElement>(
          "canvas > [drawable]",
        )!;
        const element = root.shadowRoot!.querySelector<HTMLElement>(`#${id}`)!;
        const bounds = element.getBoundingClientRect(),
          page = root.getBoundingClientRect();
        return {
          x: (bounds.x - page.x + bounds.width * fraction) / page.width,
          y: (bounds.y - page.y + bounds.height / 2) / page.height,
        };
      },
      { id, fraction },
    );
    return app.cockpitCoordinates(3, "cockpit-browser-page", point.x, point.y);
  };
  const observeInput = async (kind: "pointerup" | "wheel") =>
    app.page.evaluate((kind) => {
      const content = document.querySelector<HTMLElement>(
        "canvas > [drawable]",
      )!;
      const state = window as Window & {
        htmlInputs?: Record<"pointerup" | "wheel", number>;
        htmlObserved?: WeakSet<Element>;
      };
      state.htmlInputs ??= { pointerup: 0, wheel: 0 };
      state.htmlObserved ??= new WeakSet();
      if (!state.htmlObserved.has(content)) {
        state.htmlObserved.add(content);
        for (const event of ["pointerup", "wheel"] as const)
          content.addEventListener(event, () => {
            state.htmlInputs![event]++;
          });
      }
      return state.htmlInputs[kind];
    }, kind);
  const delivered = async (kind: "pointerup" | "wheel", count: number) => {
    await app.page.waitForFunction(
      ({ kind, count }) =>
        ((
          window as Window & {
            htmlInputs?: Record<"pointerup" | "wheel", number>;
          }
        ).htmlInputs?.[kind] ?? 0) > count,
      { kind, count },
    );
    await app.settle();
  };
  let left = false;
  return {
    press,
    async pagePress(id, fraction) {
      const count = await observeInput("pointerup");
      const point = await pagePoint(id, fraction);
      await app.page.mouse.click(point.x, point.y);
      await delivered("pointerup", count);
    },
    async paste(text) {
      await app.page
        .context()
        .grantPermissions(["clipboard-read", "clipboard-write"]);
      await app.page.evaluate(
        (text) => navigator.clipboard.writeText(text),
        text,
      );
      await app.page.keyboard.press("ControlOrMeta+V");
      await app.settle();
    },
    async type(text) {
      await app.page.keyboard.insertText(text);
    },
    async key(key) {
      await app.page.keyboard.press(key);
    },
    async wheel(id, delta) {
      const count = await observeInput("wheel");
      const point = await pagePoint(id);
      await app.page.mouse.move(point.x, point.y);
      await app.page.mouse.wheel(0, delta);
      await delivered("wheel", count);
    },
    async dom() {
      return app.page.evaluate(() => {
        const root = document.querySelector("canvas > [drawable]")!.shadowRoot!;
        const fields = Object.fromEntries(
          [
            ...root.querySelectorAll<
              HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement
            >("input,select,textarea"),
          ].map((node) => [
            node.id,
            node instanceof HTMLInputElement &&
            ["checkbox", "radio"].includes(node.type)
              ? node.checked
              : node.value,
          ]),
        );
        return {
          fields,
          output: root.querySelector("output")?.textContent ?? "",
          focus: root.activeElement?.id ?? "",
          scroll: root.querySelector("main")!.scrollTop,
        };
      });
    },
    async address(value) {
      const current = await app.call<{ address: string }>(
        "cockpitBrowserState",
      );
      const focused = await app.page.evaluate((address) => {
        const active = document.activeElement;
        return (
          active instanceof HTMLTextAreaElement &&
          active.dataset.ippNativeText === "true" &&
          active.value === address
        );
      }, current.address);
      // Keep an already focused address editor: another asynchronous pointer
      // placement must not race the keyboard selection we are about to make.
      if (!focused) await press("address/input");
      await app.page.waitForFunction(
        (address) =>
          document.activeElement instanceof HTMLTextAreaElement &&
          document.activeElement.dataset.ippNativeText === "true" &&
          document.activeElement.value === address,
        current.address,
        { timeout: 10_000 },
      );
      await app.page.keyboard.press("ControlOrMeta+A");
      await app.page.keyboard.insertText(value);
      const deadline = performance.now() + 20_000;
      while (
        (await app.call<{ address: string }>("cockpitBrowserState")).address !==
        value
      ) {
        if (performance.now() > deadline)
          throw new Error("Address edit did not reach the native text field");
        await app.page.waitForTimeout(50);
      }
      await app.page.keyboard.press("Enter");
      await app.settle();
    },
    async state() {
      if (left)
        return {
          source: "",
          status: "",
          address: "",
          domSources: await app.page.locator("canvas > [drawable]").count(),
        };
      return app.call("cockpitBrowserState");
    },
    async skin(value) {
      await app.action("skin", value);
    },
    async frame(label) {
      // Source registration is asynchronous; wait for completed HTML pixels.
      let counts = { blue: 0, green: 0, orange: 0, light: 0 };
      for (let attempt = 0; attempt < 30; attempt++) {
        const capture = await app.capture(label);
        const image = decodePng(
          Buffer.from(capture.dataUrl.split(",")[1]!, "base64"),
        );
        const center = await app.cockpitCoordinates(3, "cockpit-browser-page");
        const scale = image.width / app.page.viewportSize()!.width;
        counts = { blue: 0, green: 0, orange: 0, light: 0 };
        // Interior of the projected page, excluding cockpit lamps and side panes.
        for (
          let y = Math.max(0, Math.floor((center.y - 170) * scale));
          y < Math.min(image.height, (center.y + 170) * scale);
          y++
        )
          for (
            let x = Math.max(0, Math.floor((center.x - 195) * scale));
            x < Math.min(image.width, (center.x + 195) * scale);
            x++
          ) {
            const i = (y * image.width + x) * 4;
            const r = image.pixels[i]!,
              g = image.pixels[i + 1]!,
              b = image.pixels[i + 2]!;
            if (b > 130 && b > r * 1.4 && b > g * 1.1) counts.blue++;
            if (g > 65 && g > r * 1.4 && g > b * 1.15) counts.green++;
            if (r > 150 && r > g * 1.3 && g > b * 1.25) counts.orange++;
            if (r > 190 && g > 190 && b > 175) counts.light++;
          }
        const expected =
          label.includes("submitted") || label.includes("mission")
            ? counts.green
            : label.includes("crew")
              ? counts.orange
              : label.includes("welcome") || label.includes("back")
                ? counts.blue
                : 101;
        if (counts.light > 1000 && expected > 100) break;
        await app.page.waitForTimeout(100);
      }
      return counts;
    },
    async resize() {
      await app.page.setViewportSize({ width: 1000, height: 700 });
      await app.settle();
    },
    async leave() {
      await app.navigate("shapes");
      left = true;
    },
  };
}
