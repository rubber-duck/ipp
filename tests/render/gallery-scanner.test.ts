/** The real mini-app through worker WASM/WebGL, retained GUI input and completed frames. */
import assert from "node:assert/strict";
import type { EntitySnapshot, Inspection } from "@ipp/client";
import { writeDataUrl } from "./evidence.js";
import { resolve } from "node:path";
import test from "node:test";
import { runBrowserEnvironment } from "../browser/environment.js";
import { openGallery } from "./gallery-driver.js";
import { projectContent } from "./gallery-gui-panel.js";
import { guiApplication, waitForGuiState } from "./gallery-gui-support.js";
import {
  environment,
  waitApp,
  find,
  press,
  expected,
  enterWorkspace,
  openSettings,
  selectSettingsPage,
  selectPresentationPage,
  spacing,
} from "./gallery-scanner-support.js";

test("Gallery scanner logs in with a masked editor and opens the live workspace", {
  timeout: 180_000,
}, async (context) => {
  await runBrowserEnvironment(
    "Scanner login and workspace",
    environment("login"),
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario, {
        initialPage: "gui",
        canvasShare: 1,
      });
      await g.page.waitForFunction(
        () =>
          document.querySelector<HTMLOutputElement>("#status")?.dataset
            .state === "ready",
      );
      const initial = await waitApp(g, (value) => value.ready);
      assert.equal(initial.state.app.phase, "login");
      assert.equal(initial.error, undefined);
      const login = await g.capture("scanner-login-ready");
      assert.equal(login.frame.failedDrawCalls, 0);
      await g.page.screenshot({
        path: resolve(scenario.evidence.directory, "scanner-login-page.png"),
        fullPage: true,
      });
      const password = await find(g, "gui-password");
      expected(password, [342, 349.25, 352, 40]);
      await press(g, password, 0.1);
      await g.page.waitForFunction(
        () =>
          document.querySelectorAll("textarea").length === 1 &&
          document.activeElement === document.querySelector("textarea"),
      );
      await g.page.keyboard.insertText("demo phrase");
      await waitApp(g, (value) => value.state.app.password === "demo phrase");
      await g.capture("scanner-login-masked");
      const reveal = await find(g, "gui-reveal-password");
      await press(g, reveal, 0.1);
      await waitApp(g, (value) => value.state.app.reveal);
      const revealed = await find(g, "gui-password");
      assert.deepEqual(
        revealed.target,
        password.target,
        "Reveal replaced the editor",
      );
      assert.deepEqual(revealed.value, { kind: "text", value: "demo phrase" });
      await g.capture("scanner-login-revealed");
      await press(g, reveal, 0.1);
      await press(g, revealed, 0.1);
      const loginPanel = await g.call<Inspection>("inspectGalleryPanel");
      const card = loginPanel.entities.find(
        (e) => e.metadata.symbolicId === "gui-login-card",
      );
      assert.ok(card);
      const driver = loginPanel.controllers?.find((c) =>
        c.description.drivers.some((d) => d.target === card.id),
      );
      assert.ok(driver);
      const liftSample = g.call<{
        card: EntitySnapshot;
        samples: unknown[];
        captured: { dataUrl: string; frame: { failedDrawCalls: number } };
        app: { state: { app: { phase: string } } };
      }>("captureGalleryLoginLift", "scanner-login-lift", card.id, driver.id);
      await g.page.keyboard.press("Enter");
      const sample = await liftSample;
      const transition = sample.card.components.find(
        (c) => "previous_layer" in c.fields,
      )?.fields;
      const style = sample.card.components.find(
        (c) => "layer" in c.fields && "opacity" in c.fields,
      )?.fields;
      assert.ok(transition && style);
      assert.equal(transition.previous_layer, 1);
      assert.equal(style.layer, 2);
      assert.ok(
        Number(transition.progress) > 0 && Number(transition.progress) <= 1,
      );
      assert.ok(Number(style.opacity) >= 0 && Number(style.opacity) < 1);
      assert.equal(sample.app.state.app.phase, "connecting");
      assert.equal(sample.captured.frame.failedDrawCalls, 0);
      await writeDataUrl(
        resolve(scenario.evidence.directory, "scanner-login-lift.png"),
        sample.captured.dataUrl,
      );
      await scenario.evidence.record("natural-login-lift", {
        card: sample.card,
        samples: sample.samples,
        frame: sample.captured.frame,
      });
      const connecting = await waitApp(
        g,
        (value) => value.state.app.phase === "connecting",
      );
      assert.equal(connecting.state.app.password, "");
      const eligibility = sample.card.components.find(
        (c) => "effective_enabled" in c.fields,
      )?.fields;
      assert.equal(
        eligibility?.effective_enabled,
        false,
        "Fading login subtree still accepts input",
      );
      await waitApp(
        g,
        (value) =>
          value.state.app.phase === "connecting" &&
          value.state.app.progress >= 0.65,
      );
      const loaded = await waitForGuiState(g, (state) =>
        state.controls.some(
          (c) =>
            c.symbol === "gui-login-terminal" &&
            c.scroll &&
            c.value.kind === "scroll" &&
            c.scroll.capacity[1] > 0 &&
            Math.abs(c.value.offset[1] - c.scroll.capacity[1]) < 1,
        ),
      );
      await scenario.evidence.record(
        "loading-terminal-tail",
        loaded.controls.find((c) => c.symbol === "gui-login-terminal"),
      );
      await g.capture("scanner-connecting");
      await g.page.screenshot({
        path: resolve(
          scenario.evidence.directory,
          "scanner-after-loading-page.png",
        ),
        fullPage: true,
      });
      const complete = await waitApp(
        g,
        (value) => value.state.app.phase === "workspace",
      );
      assert.equal(complete.state.app.progress, 1);
      assert.equal(complete.state.app.lines.length, 14);
      assert.equal(complete.state.declarationIssue, undefined);
      const workspace = await g.capture("scanner-workspace-ready");
      assert.equal(workspace.frame.failedDrawCalls, 0);
      // Independent scanner rectangle: its 416-unit paint sits at (65.25,121.25).
      // Sample well inside it so the oracle distinguishes a radar from a white fallback.
      const corners = await projectContent(
        g,
        [
          [115.25, 171.25],
          [431.25, 487.25],
        ],
        0.1,
      );
      const region = await g.call<{
        pixels: string;
        width: number;
        height: number;
      }>("viewerCaptureRegionPixels", "scanner-workspace-ready", [
        corners[0]!.x,
        corners[0]!.y,
        corners[1]!.x,
        corners[1]!.y,
      ]);
      const pixels = Buffer.from(region.pixels, "base64");
      let cyan = 0,
        white = 0,
        dark = 0;
      for (let at = 0; at < pixels.length; at += 4) {
        const r = pixels[at]!,
          green = pixels[at + 1]!,
          b = pixels[at + 2]!;
        if (r < 120 && green > 110 && b > 140) cyan++;
        if (r > 240 && green > 240 && b > 240) white++;
        if (r < 90 && green < 100 && b < 110) dark++;
      }
      await scenario.evidence.record("scanner-radar-pixels", {
        cyan,
        white,
        dark,
        area: region.width * region.height,
        rect: [65.25, 121.25, 416, 416],
      });
      assert.ok(cyan > 100, "radar has no cyan sweep/returns");
      assert.ok(
        white < region.width * region.height * 0.01,
        "radar rendered as white fallback",
      );
      assert.ok(
        dark > region.width * region.height * 0.6,
        "radar background is not clear",
      );
      await g.page.screenshot({
        path: resolve(
          scenario.evidence.directory,
          "scanner-workspace-page.png",
        ),
        fullPage: true,
      });
      await scenario.evidence.record("host-clock-login", {
        initial,
        connecting,
        complete,
        eligibility,
      });
      const pause = await find(g, "gui-scan");
      await press(g, pause, 0.2);
      await waitApp(g, (value) => !value.state.autoscan);
      const settings = await find(g, "gui-settings-open");
      await press(g, settings, await spacing(g));
      await waitApp(g, (value) => value.state.app.settings);
      await g.capture("scanner-settings-display");
      await g.page.screenshot({
        path: resolve(scenario.evidence.directory, "scanner-settings-page.png"),
        fullPage: true,
      });
      for (const label of ["SURFACE", "STYLE", "LAYERS"] as const) {
        await selectPresentationPage(g, label);
        await g.page.screenshot({
          path: resolve(
            scenario.evidence.directory,
            `scanner-settings-${label.toLowerCase()}-page.png`,
          ),
          fullPage: true,
        });
      }
      await selectSettingsPage(g, "projection");
      await g.page.screenshot({
        path: resolve(
          scenario.evidence.directory,
          "scanner-settings-projection-page.png",
        ),
        fullPage: true,
      });
      const tuning = await find(g, "gui-tuning");
      const [scrollPoint] = await projectContent(
        g,
        [
          [
            tuning.bounds[0] + tuning.bounds[2] / 2,
            tuning.bounds[1] + tuning.bounds[3] / 2,
          ],
        ],
        4 * (await spacing(g)),
      );
      await g.page.mouse.move(scrollPoint!.clientX, scrollPoint!.clientY);
      await g.page.mouse.wheel(0, 520);
      const scrolled = await waitForGuiState(g, (state) =>
        state.controls.some(
          (control) =>
            control.symbol === "gui-tuning" &&
            control.value.kind === "scroll" &&
            control.value.offset[1] > 0,
        ),
      );
      await scenario.evidence.record(
        "scanner-settings-projection-scroll",
        scrolled.controls.find((control) => control.symbol === "gui-tuning"),
      );
      await g.capture("scanner-settings-projection-scrolled");
      await g.page.screenshot({
        path: resolve(
          scenario.evidence.directory,
          "scanner-settings-projection-scrolled-page.png",
        ),
        fullPage: true,
      });
      for (let sample = 0; sample < 3; sample++) {
        const label = `scanner-settings-projection-stable-${sample}`;
        const frame = await g.capture(label);
        await g.page.screenshot({
          path: resolve(scenario.evidence.directory, `${label}-page.png`),
          fullPage: true,
        });
        assert.equal(frame.frame.failedDrawCalls, 0);
      }
      await selectSettingsPage(g, "scene");
      await g.page.screenshot({
        path: resolve(
          scenario.evidence.directory,
          "scanner-settings-scene-page.png",
        ),
        fullPage: true,
      });
      assert.deepEqual(g.errors, []);
    },
  );
});

test("Gallery scanner protects pulse, keeps a live log and excludes input under settings", {
  timeout: 180_000,
}, async (context) => {
  await runBrowserEnvironment(
    "Scanner workspace controls",
    environment("workspace"),
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario, {
        initialPage: "gui",
        canvasShare: 1,
      });
      await enterWorkspace(g);
      const initial = await waitApp(
        g,
        (v) => v.state.shieldBlocker !== undefined,
      );
      await scenario.evidence.record("workspace-interlock-ready", initial);
      expected(await find(g, "gui-log-open"), [65.25, 611.75, 72, 32]);
      expected(await find(g, "gui-settings-open"), [149.25, 611.75, 104, 32]);
      const panel = await g.call<Inspection>("inspectGalleryPanel");
      const statusBounds = panel.entities
        .find((e) => e.metadata.symbolicId === "gui-scanner-status")
        ?.components.find(
          (c) =>
            "x" in c.fields && "width" in c.fields && !("kind" in c.fields),
        )?.fields;
      assert.ok(statusBounds);
      assert.ok(
        Number(statusBounds.y) + Number(statusBounds.height) <= 611.75,
        "footer actions overlap the radar status",
      );
      // Pause the sweep and change actual range through the vertical rail.
      await press(g, await find(g, "gui-scan"), 0.2);
      await waitApp(g, (v) => !v.state.autoscan);
      await g.capture("scanner-range-wide");
      const range = await find(g, "gui-scan-range/slider");
      await press(g, range, 0.1);
      await g.page.keyboard.press("Home");
      await waitApp(g, (v) => v.state.app.range === 1);
      await waitForGuiState(g, (v) =>
        v.texts.some(
          (t) =>
            t.symbol === "gui-scanner-status" && t.text.includes("0 CONTACTS"),
        ),
      );
      await g.capture("scanner-range-near");
      const corners = await projectContent(
        g,
        [
          [65.25, 121.25],
          [481.25, 537.25],
        ],
        0.1,
      );
      const rect = [corners[0]!.x, corners[0]!.y, corners[1]!.x, corners[1]!.y];
      const readRadar = async (name: string) => {
        const region = await g.call<{
          pixels: string;
          width: number;
          height: number;
        }>("viewerCaptureRegionPixels", name, rect);
        return Buffer.from(region.pixels, "base64");
      };
      const wide = await readRadar("scanner-range-wide"),
        near = await readRadar("scanner-range-near");
      let changed = 0;
      for (let index = 0; index < wide.length; index += 4) {
        if (
          [0, 1, 2].some(
            (channel) =>
              Math.abs(wide[index + channel]! - near[index + channel]!) > 20,
          )
        )
          changed++;
      }
      assert.ok(changed > 30, "range did not change rendered contacts");
      await g.page.keyboard.press("ArrowUp");
      await g.page.keyboard.press("ArrowUp");
      await waitApp(g, (v) => v.state.app.range === 1.5);
      await waitForGuiState(g, (v) =>
        v.texts.some(
          (t) =>
            t.symbol === "gui-scanner-status" && t.text.includes("2 CONTACTS"),
        ),
      );
      const rangePaint = (
        await g.call<Inspection>("inspectGalleryPanel")
      ).entities
        .find((e) => e.metadata.symbolicId === "gui-radar")
        ?.components.find((c) => c.properties?.range)?.properties?.range;
      assert.ok(rangePaint);
      await scenario.evidence.record("scanner-range-geometry", {
        changed,
        rangePaint,
        state: await guiApplication(g),
      });
      const strength = await find(g, "gui-pulse-strength/slider");
      await press(g, strength, 0.2);
      await g.page.keyboard.press("End");
      await waitApp(g, (v) => v.state.app.strength === 1);
      await press(g, await find(g, "gui-charge"), 0.2);
      const charging = await waitApp(
        g,
        (v) =>
          v.state.app.charge.phase === "charging" &&
          v.state.app.charge.progress > 0 &&
          v.state.app.charge.progress < 1,
      );
      await scenario.evidence.record("host-charge-progress", charging);
      await g.capture("scanner-charge-preparation");
      await g.page.screenshot({
        path: resolve(
          scenario.evidence.directory,
          "scanner-charge-preparation-page.png",
        ),
        fullPage: true,
      });
      await waitApp(g, (v) => v.state.app.charge.phase === "ready");
      // Editing the strength invalidates prepared energy, without firing it.
      await press(g, strength, 0.2);
      await g.page.keyboard.press("Home");
      await waitApp(
        g,
        (v) =>
          v.state.app.charge.phase === "idle" &&
          Math.abs(v.state.app.strength - 0.1) < 1e-5,
      );
      assert.equal((await find(g, "gui-pulse")).enabled, false);
      await g.page.keyboard.press("ArrowRight");
      await waitApp(g, (v) => Math.abs(v.state.app.strength - 0.15) < 1e-5);
      await press(g, await find(g, "gui-charge"), 0.2);
      await waitApp(g, (v) => v.state.app.charge.phase === "ready");
      const pulse = await find(g, "gui-pulse");
      expected(pulse, [818.75, 547.75, 128, 40]);
      const [point] = await projectContent(g, [[882.75, 567.75]], 0.3);
      assert.equal(
        pulse.enabled,
        true,
        "ready protected pulse should be enabled",
      );
      await g.call("observeGalleryGuiInput");
      await g.page.mouse.move(point!.clientX, point!.clientY);
      await g.page.mouse.down();
      await g.page.mouse.up();
      await g.capture("scanner-pulse-blocked");
      assert.equal(
        (await guiApplication(g)).state.pulseSequence,
        initial.state.pulseSequence,
        "armed shield allowed pulse",
      );
      const blocked = await g.call<{ outcomes: { disposition: string }[] }>(
        "finishGalleryGuiInputObservation",
      );
      await scenario.evidence.record("armed-pulse-routing", blocked);
      assert.ok(
        blocked.outcomes.some((outcome) => outcome.disposition === "blocked"),
        "physical shield must block the ray",
      );
      await press(g, await find(g, "gui-pulse-interlock"), 0.2);
      await waitApp(g, (v) => !v.state.shieldArmed);
      await press(g, pulse, 0.3);
      const firing = await waitApp(
        g,
        (v) =>
          v.state.pulseSequence === initial.state.pulseSequence + 1 &&
          v.state.pulse.value > 0.15 &&
          v.state.pulse.value < 0.85,
      );
      assert.equal(firing.state.app.charge.phase, "idle");
      assert.equal(firing.state.app.charge.progress, 0);
      assert.ok(Math.abs(firing.state.pulseStrength - 0.15) < 1e-5);
      await g.capture("scanner-pulse-active");
      await g.page.screenshot({
        path: resolve(scenario.evidence.directory, "scanner-pulse-page.png"),
        fullPage: true,
      });
      await scenario.evidence.record("pulse-effect", firing);
      await waitApp(g, (v) => v.state.pulse.state !== "running");
      await press(g, await find(g, "gui-pulse-interlock"), 0.2);
      await waitApp(g, (v) => v.state.shieldArmed);
      await openSettings(g);
      assert.equal(
        (await guiApplication(g)).state.shieldArmed,
        true,
        "modal opening forgot armed preference",
      );
      assert.ok(
        !(await g.inspect()).entities.some(
          (e) => e.metadata.symbolicId === "gui-input-shield",
        ),
        "physical shield obscures settings",
      );
      await g.capture("scanner-modal-ready");
      await g.page.screenshot({
        path: resolve(
          scenario.evidence.directory,
          "scanner-settings-unobstructed.png",
        ),
        fullPage: true,
      });
      // A modal consumes the pulse point, even while its interlock presentation is absent.
      await g.page.mouse.move(point!.clientX, point!.clientY);
      await g.page.mouse.down();
      await g.page.mouse.up();
      await g.capture("scanner-modal-input-exclusion");
      assert.equal(
        (await guiApplication(g)).state.pulseSequence,
        firing.state.pulseSequence,
      );
      await g.page.keyboard.press("Escape");
      await waitApp(g, (v) => !v.state.app.settings);
      await press(g, await find(g, "gui-log-open"), await spacing(g));
      await waitApp(g, (v) => v.state.app.logOpen);
      const terminal = await waitForGuiState(g, (s) =>
        s.controls.some(
          (c) =>
            c.symbol === "gui-session-terminal" &&
            c.scroll &&
            c.value.kind === "scroll" &&
            Math.abs(c.value.offset[1] - c.scroll.capacity[1]) < 1,
        ),
      );
      const tail = terminal.controls.find(
        (c) => c.symbol === "gui-session-terminal",
      )!;
      assert.ok(tail.scroll!.capacity[1] > 0);
      // CLEAR changes only events/lastCommand; the drawer must update immediately.
      await press(g, await find(g, "gui-clear-log"), 0.2);
      await waitApp(
        g,
        (v) =>
          v.state.events.length === 1 &&
          v.state.events[0]!.includes("LOG CLEARED"),
      );
      await waitForGuiState(g, (s) =>
        s.texts.some((t) => t.text.includes("LOG CLEARED")),
      );
      const shrunk = await waitForGuiState(g, (s) =>
        s.controls.some(
          (c) =>
            c.symbol === "gui-session-terminal" &&
            c.scroll &&
            c.scroll.capacity[1] < tail.scroll!.capacity[1],
        ),
      );
      await scenario.evidence.record("terminal-clamped-after-clear", shrunk);
      // Scroll away using physical wheel input, then append an ordinary scene event.
      const log = await find(g, "gui-session-terminal");
      const [logPoint] = await projectContent(
        g,
        [[log.bounds[0] + 200, log.bounds[1] + 80]],
        0.2,
      );
      await g.page.mouse.move(logPoint!.clientX, logPoint!.clientY);
      await g.page.mouse.wheel(0, -1200);
      const away = await waitForGuiState(g, (s) =>
        s.controls.some(
          (c) =>
            c.symbol === "gui-session-terminal" &&
            c.scroll &&
            c.value.kind === "scroll" &&
            c.value.offset[1] < c.scroll.capacity[1] - 1,
        ),
      );
      const awayOffset = away.controls.find(
        (c) => c.symbol === "gui-session-terminal",
      )!.value;
      await g.call("gallerySceneAction", "setAccent", "amber");
      const appended = await waitForGuiState(g, (s) =>
        s.texts.some((t) => t.text.includes("ACCENT")),
      );
      const appendedLog = appended.controls.find(
        (c) => c.symbol === "gui-session-terminal",
      )!;
      assert.deepEqual(
        appendedLog.value,
        awayOffset,
        "new log line stole manual scroll position",
      );
      await press(g, await find(g, "gui-log-open"), await spacing(g));
      await waitApp(g, (v) => !v.state.app.logOpen);
      const closing = await find(g, "gui-scanner-close");
      const [closePoint] = await projectContent(
        g,
        [
          [
            closing.bounds[0] + closing.bounds[2] / 2,
            closing.bounds[1] + closing.bounds[3] / 2,
          ],
        ],
        0,
      );
      await press(g, await find(g, "gui-charge"), 0.2);
      const beforeClose = await waitApp(
        g,
        (v) =>
          v.state.app.charge.phase === "charging" &&
          v.state.app.charge.progress > 0,
      );
      await scenario.evidence.record("close-during-charge", beforeClose);
      await g.page.mouse.click(closePoint!.clientX, closePoint!.clientY);
      const reset = await waitApp(g, (v) => v.state.app.phase === "login");
      assert.equal(reset.state.app.password, "");
      assert.equal(reset.state.app.progress, 0);
      assert.deepEqual(reset.state.app.charge, { phase: "idle", progress: 0 });
      await enterWorkspace(g);
      const reopened = await guiApplication(g);
      assert.deepEqual(
        reopened.state.app.charge,
        { phase: "idle", progress: 0 },
        "closed charge completed into new session",
      );
      assert.equal(reopened.state.pulse.state, "idle");
      assert.equal((await find(g, "gui-pulse")).enabled, false);
      assert.deepEqual(g.errors, []);
    },
  );
});
