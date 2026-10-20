/** Blender-exported bridge mounts and spatial environment texture inputs. */
export interface CockpitRuntime {
  readonly reference: { readonly width: number; readonly height: number };
  readonly bridge: {
    readonly entity: string;
    readonly carouselMount: string;
    readonly paneExtent: readonly [number, number];
    readonly paneElevation: number;
  };
  readonly surfaces: Readonly<
    Record<string, { readonly width: number; readonly height: number }>
  >;
  readonly lighting: {
    readonly bodies: readonly {
      readonly entity: string;
      readonly baseTexture: string;
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
  for (const body of runtime.lighting.bodies) textureSource(body.baseTexture);
  for (const body of runtime.exterior.bodies) textureSource(body.baseTexture);
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
