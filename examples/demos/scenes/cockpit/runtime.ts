/** Blender-exported bridge mounts and spatial environment texture inputs. */
export interface CockpitLightTile {
  readonly baseTexture: string;
  readonly contributionTexture: string;
  /** Region in the original mesh UV domain; base and contribution share it. */
  readonly bounds: readonly [number, number, number, number];
  /** Interior texel boundaries inside the padded square texture. */
  readonly textureRect: readonly [number, number, number, number];
  readonly dimensions: readonly [number, number];
  readonly sourcePixels: readonly [number, number, number, number];
}

export interface CockpitRuntime {
  readonly reference: { readonly width: number; readonly height: number };
  readonly bridge: {
    readonly entity: string;
    readonly carouselMount: string;
    readonly paneExtent: readonly [number, number];
    readonly paneElevation: number;
    readonly panoramaSplit: number;
  };
  readonly surfaces: Readonly<
    Record<string, { readonly width: number; readonly height: number }>
  >;
  readonly lighting: {
    readonly bodies: readonly {
      readonly entity: string;
      readonly tiles: readonly CockpitLightTile[];
    }[];
    readonly zones: readonly {
      readonly name: string;
      /** Normalized top-left UV rectangle, in the exported texture domain. */
      readonly rect: readonly [number, number, number, number];
      readonly group: "cabin" | "dashboard" | "buttons" | "dials";
    }[];
    readonly carrierEntityNames: readonly string[];
  };
  readonly exterior: {
    readonly planetMap: {
      readonly dimensions: readonly [number, number];
      readonly surface: string;
      readonly clouds: string;
      readonly cities: string;
    };
    readonly bodies: readonly {
      readonly entity: string;
      readonly baseTexture: string;
      readonly quadrant?: number;
      readonly bounds?: readonly [number, number, number, number];
      readonly tiles?: readonly CockpitLightTile[];
      readonly role:
        | "planet"
        | "atmosphere"
        | "stars"
        | "bridge"
        | "trim"
        | "starlight";
    }[];
    readonly textures: {
      readonly cities: string;
    };
    readonly carrierEntityNames: readonly string[];
  };
}

export function textureSource(source: string): string {
  const match = /^\/assets\/([A-Za-z0-9_-]+(?:\/[A-Za-z0-9_-]+)*)$/.exec(
    source,
  );
  if (!match)
    throw new Error(`Invalid exported cockpit texture source: ${source}`);
  return `https://cockpit.ipp.invalid/${match[1]}`;
}

export function validateRuntime(runtime: CockpitRuntime): CockpitRuntime {
  if (
    ![runtime.reference?.width, runtime.reference?.height].every(
      (value) => Number.isFinite(value) && value > 0,
    )
  )
    throw new Error("Cockpit export has no reference extent");
  if (
    !runtime.surfaces ||
    !runtime.bridge?.carouselMount ||
    !runtime.bridge.entity ||
    !Number.isFinite(runtime.bridge.paneElevation) ||
    runtime.bridge.paneExtent?.length !== 2 ||
    !runtime.bridge.paneExtent.every(
      (value) => Number.isFinite(value) && value > 0,
    ) ||
    !runtime.lighting?.bodies.length ||
    !runtime.exterior?.bodies.length
  )
    throw new Error(
      "Cockpit export has no runtime placement or lighting metadata",
    );
  for (const [name, surface] of Object.entries(runtime.surfaces))
    if (
      ![surface.width, surface.height].every(
        (value) => Number.isFinite(value) && value > 0,
      )
    )
      throw new Error(`Invalid cockpit surface extent: ${name}`);
  for (const body of [...runtime.lighting.bodies, ...runtime.exterior.bodies]) {
    if ("baseTexture" in body) textureSource(body.baseTexture);
    if (!("role" in body) || body.role === "bridge") {
      if (body.tiles?.length !== ("role" in body ? 2 : 3))
        throw new Error(
          `Cockpit body has no registered lighting tiles: ${body.entity}`,
        );
      let right = 0;
      for (const tile of body.tiles) {
        textureSource(tile.baseTexture);
        textureSource(tile.contributionTexture);
        if (
          tile.dimensions.length !== 2 ||
          !tile.dimensions.every(
            (value) => Number.isInteger(value) && value > 0,
          ) ||
          tile.dimensions[0] !== tile.dimensions[1] ||
          tile.bounds[0] !== right ||
          tile.bounds[1] !== 0 ||
          tile.bounds[3] !== 1 ||
          tile.bounds[2] <= right ||
          tile.bounds[2] > 1 ||
          !tile.textureRect.every(
            (value) => Number.isFinite(value) && value > 0 && value < 1,
          ) ||
          tile.textureRect[0] >= tile.textureRect[2] ||
          tile.textureRect[1] >= tile.textureRect[3]
        )
          throw new Error(`Invalid square lighting tile: ${body.entity}`);
        right = tile.bounds[2];
      }
      if (right !== 1)
        throw new Error(`Incomplete lighting tiles: ${body.entity}`);
    }
  }
  const quadrants = runtime.exterior.bodies.filter(
    (body) => body.role === "bridge",
  );
  if (
    quadrants.length !== 4 ||
    new Set(quadrants.map((body) => body.quadrant)).size !== 4 ||
    !Number.isFinite(runtime.bridge.panoramaSplit) ||
    runtime.bridge.panoramaSplit <= 0 ||
    runtime.bridge.panoramaSplit >= 1 ||
    quadrants.some(
      (body) =>
        !Number.isInteger(body.quadrant) ||
        body.quadrant! < 0 ||
        body.quadrant! > 3 ||
        body.bounds?.length !== 4 ||
        !body.bounds.every(
          (value) => Number.isFinite(value) && value >= 0 && value <= 1,
        ),
    )
  )
    throw new Error(
      "Cockpit export must contain four registered panorama quadrants",
    );
  if (
    runtime.exterior.planetMap?.dimensions?.length !== 2 ||
    !runtime.exterior.planetMap.dimensions.every(
      (value) => Number.isInteger(value) && value > 0,
    )
  )
    throw new Error("Cockpit export has no Earth map dimensions");
  for (const source of Object.values(runtime.exterior.textures))
    textureSource(source);
  for (const zone of runtime.lighting.zones)
    if (
      !zone.rect.every(
        (value) => Number.isFinite(value) && value >= 0 && value <= 1,
      ) ||
      zone.rect[0] > zone.rect[2] ||
      zone.rect[1] > zone.rect[3] ||
      !["cabin", "dashboard", "buttons", "dials"].includes(zone.group)
    )
      throw new Error(`Invalid cockpit light zone: ${zone.name}`);
  return runtime;
}
