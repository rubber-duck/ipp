/** Shared scene intent and observations for native and browser resource pressure. */
export const pressureSources = [
  "ipp://texture/checkerboard?width=2048&height=2048&cellsX=4&cellsY=4",
  "ipp://texture/checkerboard?width=2048&height=2048&cellsX=8&cellsY=4",
] as const;

export interface ResourceObservation {
  readonly source: string;
  readonly status: string;
  readonly error?: string;
}

export interface ResourcePressureObservation {
  readonly session: string;
  readonly entities: number;
  readonly resources: readonly ResourceObservation[];
  readonly declarations: readonly {
    readonly entity: string;
    readonly baseSource: unknown;
    readonly effectiveSource: unknown;
  }[];
}

export interface ResourcePressureDriver {
  declare(sources: readonly string[]): Promise<void>;
  observe(): Promise<ResourcePressureObservation>;
  nextFrame(): Promise<void>;
  events(): readonly ResourceObservation[];
}

/** Combined input and residency exceed the former shared 16 MiB quota. */
export async function resourcePressurePreservesSession(
  driver: ResourcePressureDriver,
) {
  const initial = await driver.observe();
  await driver.declare(pressureSources);
  const committed = await driver.observe();
  requireCondition(
    committed.session === initial.session,
    "World commit replaced the session",
  );
  requireCondition(
    committed.entities === 2,
    "Both pending declarations must commit",
  );
  requireCondition(
    pressureSources.every((source) =>
      committed.declarations.some(
        (declaration) =>
          declaration.baseSource === source &&
          declaration.effectiveSource === source,
      ),
    ),
    "Both source references must be authored and effective",
  );

  const settled = await waitFor(driver, (state) =>
    pressureSources.every((source) =>
      state.resources.some(
        (resource) =>
          resource.source === source &&
          ["loaded", "failed"].includes(resource.status),
      ),
    ),
  );
  requireCondition(
    pressureSources.every((source) =>
      settled.resources.some(
        (resource) =>
          resource.source === source && resource.status === "loaded",
      ),
    ),
    "Both assets must load beyond the former aggregate quota",
  );
  requireCondition(
    pressureSources.every((source) =>
      driver
        .events()
        .some(
          (resource) =>
            resource.source === source && resource.status === "loaded",
        ),
    ),
    "Each completed resource must publish its loaded event",
  );

  const small = "ipp://texture/checkerboard?width=2&height=2&cellsX=2&cellsY=2";
  await driver.declare([small]);
  const recovered = await waitFor(driver, (state) =>
    state.resources.some(
      (resource) => resource.source === small && resource.status === "loaded",
    ),
  );
  requireCondition(
    recovered.session === initial.session,
    "Asset growth must preserve the session",
  );
  requireCondition(
    recovered.entities === 3,
    "Later work must commit without losing existing declarations",
  );
  for (const state of [settled, recovered]) {
    requireCondition(
      committed.declarations.every((original) =>
        state.declarations.some(
          (declaration) =>
            declaration.entity === original.entity &&
            declaration.baseSource === original.baseSource &&
            declaration.effectiveSource === original.effectiveSource,
        ),
      ),
      "Asset growth must preserve each entity's base and effective source reference",
    );
  }
  const manySources = Array.from(
    { length: 300 },
    (_, index) =>
      `ipp://texture/checkerboard?width=${index + 1}&height=1&cellsX=1&cellsY=1`,
  );
  for (let offset = 0; offset < manySources.length; offset += 128)
    await driver.declare(manySources.slice(offset, offset + 128));
  const expanded = await waitFor(driver, (state) =>
    manySources.every((source) =>
      state.resources.some(
        (resource) =>
          resource.source === source && resource.status === "loaded",
      ),
    ),
  );
  requireCondition(
    expanded.entities === 303 && expanded.session === initial.session,
    "More than 256 asset declarations must load and remain observable in the same session",
  );
  requireCondition(
    manySources.every((source) =>
      driver
        .events()
        .some(
          (resource) =>
            resource.source === source && resource.status === "loaded",
        ),
    ),
    "Each asset beyond the former count quota must publish its loaded event",
  );
  return { committed, settled, recovered, expanded, events: driver.events() };
}

async function waitFor(
  driver: ResourcePressureDriver,
  ready: (state: ResourcePressureObservation) => boolean,
) {
  for (let frame = 0; frame < 600; frame++) {
    const state = await driver.observe();
    if (ready(state)) return state;
    await driver.nextFrame();
  }
  throw new Error(
    "Resource pressure scenario did not settle within 600 observed frames",
  );
}

function requireCondition(value: boolean, message: string): asserts value {
  if (!value) throw new Error(message);
}
