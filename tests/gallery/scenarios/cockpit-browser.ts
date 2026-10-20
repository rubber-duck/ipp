import assert from "node:assert/strict";

export interface CockpitBrowserDriver {
  press(id: string): Promise<void>;
  pagePress(id: string, fraction?: number): Promise<void>;
  type(text: string): Promise<void>;
  paste(text: string): Promise<void>;
  key(key: string): Promise<void>;
  wheel(id: string, delta: number): Promise<void>;
  dom(): Promise<{
    fields: Record<string, string | boolean>;
    output: string;
    focus: string;
    scroll: number;
  }>;
  address(value: string): Promise<void>;
  state(): Promise<{
    source: string;
    status: string;
    address: string;
    domSources: number;
  }>;
  skin(value: "glass" | "cybertron"): Promise<void>;
  frame(
    label: string,
  ): Promise<{ blue: number; green: number; orange: number; light: number }>;
  resize(): Promise<void>;
  leave(): Promise<void>;
}

async function waitFor(
  driver: CockpitBrowserDriver,
  predicate: (
    state: Awaited<ReturnType<CockpitBrowserDriver["state"]>>,
  ) => boolean,
) {
  const deadline = performance.now() + 20_000;
  while (performance.now() < deadline) {
    const state = await driver.state();
    assert.ok(
      !/failed|expired|timed out|unavailable|Error:/.test(state.status),
      state.status,
    );
    if (predicate(state)) return state;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  assert.fail(
    `Browser state did not settle: ${JSON.stringify(await driver.state())}`,
  );
}

export async function cockpitBrowserScenario(driver: CockpitBrowserDriver) {
  await waitFor(driver, (state) => !!state.source);
  const welcome = await driver.frame("browser-welcome");
  assert.ok(
    welcome.blue > 100 && welcome.light > 1000,
    JSON.stringify(welcome),
  );
  await driver.pagePress("callsign");
  assert.equal((await driver.dom()).focus, "callsign");
  await driver.type("Vega");
  await driver.key("Home");
  await driver.key("Shift+End");
  await driver.type("Orion");
  await driver.key("Tab");
  assert.equal((await driver.dom()).focus, "deck");
  await driver.key("ArrowUp");
  await driver.pagePress("message");
  assert.equal(
    (await driver.dom()).focus,
    "message",
    JSON.stringify(await driver.dom()),
  );
  await driver.paste("Ready for launch");
  await driver.key("Enter");
  await driver.type("All clear.");
  await driver.pagePress("updates");
  await driver.pagePress("send");
  const form = await driver.dom();
  assert.deepEqual(form.fields, {
    callsign: "Orion",
    deck: "4",
    message: "Ready for launch\nAll clear.",
    updates: true,
  });
  assert.match(form.output, /Saved: Orion · 4 · Ready for launch/);
  const submitted = await driver.frame("browser-form-submitted");
  assert.ok(
    submitted.green > welcome.green + 100,
    "live DOM feedback reaches the rendered Surface",
  );
  await driver.key("Control+Shift+Escape");
  await driver.pagePress("mission-link");
  await waitFor(
    driver,
    (state) => !!state.source && state.address.endsWith("mission.html"),
  );
  const mission = await driver.frame("browser-mission");
  assert.ok(
    mission.green > 100 && mission.light > 1000,
    JSON.stringify(mission),
  );
  assert.ok(
    mission.green > welcome.green + 100,
    "real HTML navigation changes rendered green pixels",
  );
  await driver.pagePress("power", 0.8);
  assert.ok(Number((await driver.dom()).fields.power) >= 70);
  await driver.key("ArrowLeft");
  await driver.pagePress("destination");
  await driver.key("End");
  await driver.pagePress("rescue");
  assert.equal((await driver.dom()).fields.rescue, true);
  await driver.pagePress("save-mission");
  assert.match((await driver.dom()).output, /Vega · rescue/);
  await driver.wheel("destination", 600);
  assert.ok((await driver.dom()).scroll > 0, "wheel scrolls the page");
  await driver.frame("browser-scrolled");
  await driver.press("back");
  await waitFor(
    driver,
    (state) => !!state.source && state.address.endsWith("welcome.html"),
  );
  const back = await driver.frame("browser-back");
  assert.ok(back.blue > mission.blue + 100, "Back restores the blue HTML page");
  await driver.press("forward");
  await waitFor(
    driver,
    (state) => !!state.source && state.address.endsWith("mission.html"),
  );
  await driver.press("back");
  await waitFor(
    driver,
    (state) => !!state.source && state.address.endsWith("welcome.html"),
  );
  await driver.skin("cybertron");
  const current = await driver.state();
  await driver.address(current.address.replace("welcome.html", "crew.html"));
  await waitFor(
    driver,
    (state) => !!state.source && state.address.endsWith("crew.html"),
  );
  const crew = await driver.frame("browser-crew-cybertron");
  assert.ok(crew.orange > 100 && crew.light > 1000, JSON.stringify(crew));
  await driver.press("forward");
  assert.ok(
    (await driver.state()).address.endsWith("crew.html"),
    "new navigation discards forward history",
  );
  await driver.pagePress("crew-search");
  await driver.type("Mira");
  assert.equal((await driver.dom()).fields["crew-search"], "Mira");
  await driver.resize();
  await driver.frame("browser-resized");
  assert.equal(
    (await driver.state()).domSources,
    1,
    "old pages release their DOM sources",
  );
  await driver.address("https://www.google.com/");
  assert.match((await driver.state()).status, /bundled/);
  await driver.address("welcome.html");
  await waitFor(
    driver,
    (state) => !!state.source && state.address.endsWith("welcome.html"),
  );
  await driver.leave();
  assert.equal(
    (await driver.state()).domSources,
    0,
    "leaving cockpit releases all page sources",
  );
  return { welcome, submitted, mission, back, crew, form };
}
