import {
  outputProducer,
  sameOutputReference,
} from "../../../packages/ipp-client/src/references.js";
import { clientAssetSource } from "../../../packages/ipp-client/src/asset-sources.js";
import type {
  CameraProjectResultEvent,
  CameraViewMotion,
  Client,
  Command,
  GeometryEncoder,
  GeometryPickHit,
  GeometryPickResultEvent,
  HostClientBase,
  OutputReference,
  PickingWorldClient,
  RootBinding,
  ViewQueryTarget,
  ViewViewport,
  WorldPlane,
  WorldReference,
} from "@ipp/client";
import type { DriverConnectOptions } from "../driver.js";
import {
  aliasId,
  CAMERA_VIEWPORT,
  componentFields,
  createEntity,
  createPickingRing,
  PICKING_RING,
  insertComponent,
  ORTHOGRAPHIC_CAMERA,
  successfulBatch,
} from "../camera-fixtures.js";

type Values = Readonly<
  Record<string, number | string | boolean | Uint8Array<ArrayBuffer> | bigint>
>;

/**
 * Small SDK driver shared by native WebSocket and browser worker scenarios.
 * Camera selection is the Host's explicit root output binding; queries and
 * navigation name that exact view instead of a session's active camera.
 */
export class CameraFixture {
  readonly events: GeometryPickResultEvent[] = [];
  readonly world: WorldReference;
  binding: RootBinding | undefined;

  constructor(
    readonly client: PickingWorldClient,
    readonly host: HostClientBase<Client>,
    readonly record: DriverConnectOptions["record"],
    readonly encodeGeometry: GeometryEncoder,
  ) {
    const world = client.worldReference;
    check(world !== undefined, "Camera scenarios require a World reference");
    this.world = world;
  }

  async batch(commands: Command[]) {
    const outcome = await this.client.batch(commands);
    await this.record("camera_batch", { commands, outcome });
    return outcome;
  }

  async create(name: string, components: Readonly<Record<string, Values>>) {
    const entity = { kind: "alias", alias: 1 } as const;
    const outcome = await this.batch([
      createEntity(1, name),
      ...Object.entries(components).map(([name, values]) =>
        insertComponent(this.client, name, entity, values),
      ),
    ]);
    return aliasId(outcome, 1);
  }

  camera(name: string, transform: Values = { z: 6 }, camera: Values = {}) {
    return this.create(name, {
      Transform: transform,
      Camera: { ...ORTHOGRAPHIC_CAMERA, ...camera },
    });
  }

  target(name: string, transform: Values = {}, target: Values = {}) {
    return this.create(name, {
      Transform: transform,
      PickingGeometry: Object.keys(target).length
        ? target
        : {
            geometry: this.encodeGeometry({
              type: "box",
              min: [-0.5, -0.5, -0.5],
              max: [0.5, 0.5, 0.5],
            }),
          },
    });
  }

  delete(entity: bigint): Command {
    return { kind: "delete", entity: { kind: "handle", id: entity } };
  }

  remove(entity: bigint, name: string): Command {
    const component = this.client.components[name];
    if (!component) throw new Error(`Missing generated component ${name}`);
    return {
      kind: "removeComponent",
      entity: { kind: "handle", id: entity },
      component: component.id,
    };
  }

  set(entity: bigint, name: string, values: Values): Command[] {
    const component = this.client.components[name];
    if (!component) throw new Error(`Missing generated component ${name}`);
    return componentFields(this.client, name, values).map((field) => ({
      kind: "setField",
      entity: { kind: "handle", id: entity },
      component: component.id,
      field,
    }));
  }

  /** Bind the current Camera incarnation without selecting it for presentation. */
  output(entity: bigint): Promise<OutputReference> {
    return this.host.bindOutput(this.world, entity, "camera");
  }

  /** Explicitly select a Camera as the World's root view. */
  async select(
    entity: bigint,
    viewport: ViewViewport = CAMERA_VIEWPORT,
  ): Promise<RootBinding> {
    const binding = await this.host.setRootOutput(
      await this.output(entity),
      viewport,
    );
    await this.record("camera_root_binding", binding);
    check(
      outputProducer(binding.output)?.entity === entity &&
        binding.viewport.width === viewport.width &&
        binding.viewport.height === viewport.height,
      "Root binding acknowledges the requested Camera and viewport",
    );
    this.binding = binding;
    return binding;
  }

  async activate(entity: bigint, viewport: ViewViewport = CAMERA_VIEWPORT) {
    await this.select(entity, viewport);
    // Selection is observed through a real query, never a Host reply alone.
    return this.pick(0.5, 0.5, viewport);
  }

  rootBinding() {
    return this.host.getRootOutputBinding(this.world);
  }

  /** Navigate the selected root view, or an explicitly supplied binding. */
  async navigate(motion: CameraViewMotion, binding = this.selected()) {
    await this.client.navigateCamera({ binding, motion });
  }

  selected(): RootBinding {
    check(this.binding !== undefined, "No Camera is selected as root view");
    return this.binding;
  }

  /** Pick through the current root view, which must still select `output`. */
  pick(
    x = 0.5,
    y = 0.5,
    viewport: ViewViewport = CAMERA_VIEWPORT,
    includeViewPlane?: boolean,
    output: OutputReference = this.selected().output,
  ) {
    return this.pickView(
      { kind: "root", output, expectedViewport: viewport },
      x,
      y,
      includeViewPlane,
    );
  }

  async pickView(
    view: ViewQueryTarget,
    x = 0.5,
    y = 0.5,
    includeViewPlane?: boolean,
  ) {
    const result = await this.client.query({
      type: "GeometryPickQuery",
      view,
      x,
      y,
      ...(includeViewPlane === undefined ? {} : { includeViewPlane }),
    });
    if (result.ok && result.hit) {
      check(
        Object.hasOwn(result.hit, "viewPlane") === (includeViewPlane === true),
        "View planes must be returned only when requested",
      );
      if (result.hit.viewPlane) {
        equal(
          result.hit.viewPlane.point,
          result.hit.position,
          "Plane passes through hit",
        );
        close(
          Math.hypot(...result.hit.viewPlane.normal),
          1,
          "Plane normal is unit length",
        );
      }
    } else {
      check(!Object.hasOwn(result, "viewPlane"), "Misses/errors have no plane");
    }
    if (result.ok) {
      const expected = "binding" in view ? view.binding.output : view.output;
      check(
        sameOutputReference(result.view.output, expected),
        "A query never redirects to another output",
      );
    }
    await this.observeEvent(result);
    return result;
  }

  async inspect() {
    const state = await this.client.inspect();
    await this.record("camera_inspection", state);
    return state;
  }

  async project(
    x: number,
    y: number,
    plane: WorldPlane,
    viewport: ViewViewport = CAMERA_VIEWPORT,
  ): Promise<CameraProjectResultEvent> {
    const result = await this.client.query({
      type: "CameraProjectQuery",
      view: {
        kind: "root",
        output: this.selected().output,
        expectedViewport: viewport,
      },
      x,
      y,
      plane,
    });
    await this.record("camera_projection", result);
    check(
      result.session === this.client.session &&
        result.requestId > 0n &&
        result.tick > 0n,
      "Projection keeps session, correlation and frame identity",
    );
    return result;
  }

  async partialCameraEdit(command: Command, name: string, usable = false) {
    const outcome = await this.batch([
      createEntity(99, `partial-${name}`),
      command,
      this.delete(0xffffffffffffffffn),
    ]);
    check(!outcome.ok, `${name} batch must fail`);
    check(
      outcome.error.operation === 1 || outcome.error.operation === 2,
      "Losing the selected root Camera is accepted, so only an invalid camera edit or the final invalid handle fails",
    );
    const created = outcome.aliases.find((entry) => entry.alias === 99);
    check(
      created !== undefined,
      "Failed batches return applied entity aliases",
    );
    check(
      (await this.inspect()).entities.some(
        (entity) => entity.id === created.id,
      ),
      "Creation before camera failure remains applied",
    );
    // Losing the entity or its Camera leaves the selected root view unusable
    // until another Camera is selected. A missing Transform contributes an
    // identity pose, and a rejected Camera write has no effect.
    check(
      (await this.pick()).ok === usable,
      usable
        ? "The partially edited camera remains usable"
        : "The partially edited camera is unusable",
    );
    // Cleanup also applies while the selected root Camera remains unusable.
    await this.batch([this.delete(created.id)]);
    return outcome.error;
  }

  async rejectGeometry(entity: bigint) {
    const outcome = await this.batch([
      createEntity(99, "partial-geometry"),
      ...this.set(entity, "PickingGeometry", {
        geometry: new Uint8Array([1, 2, 3]),
      }),
    ]);
    check(
      !outcome.ok && outcome.error.reason === "InvalidGeometry",
      "Malformed inline geometry fails activation",
    );
    const created = outcome.aliases.find((entry) => entry.alias === 99)!;
    check(
      (await this.inspect()).entities.some((value) => value.id === created.id),
      "Malformed geometry does not roll back preceding entity creation",
    );
    successfulBatch(await this.batch([this.delete(created.id)]));
    successfulBatch(
      await this.batch([
        insertComponent(
          this.client,
          "PickingGeometry",
          { kind: "handle", id: entity },
          {
            geometry: this.encodeGeometry({ type: "sphere", radius: 0.5 }),
          },
        ),
      ]),
    );
    picked(await this.pick(), entity);
  }

  async uploadRing(asset: bigint) {
    const outcome = clientAssetSource(this.client.session, 1, asset);
    await this.client.registerAsset(outcome, createPickingRing());
    await this.record("picking_mesh_upload", outcome);
  }

  async uploadGeometry(asset: bigint) {
    const outcome = clientAssetSource(this.client.session, 6, asset);
    await this.client.registerAsset(
      outcome,
      this.encodeGeometry(PICKING_RING).buffer,
    );
    await this.record("picking_geometry_upload", outcome);
  }

  async waitForGeometry(
    source: string,
    status: "loaded" | "failed" = "loaded",
  ) {
    return this.waitForMesh(source, status, 6);
  }

  async waitForMesh(
    source: string,
    status: "loaded" | "failed" = "loaded",
    kind = 1,
  ) {
    const deadline = performance.now() + 10_000;
    for (;;) {
      const state = await this.inspect();
      const resource = state.resources.find(
        (item) => item.kind === kind && item.source === source,
      );
      if (resource?.status === status) return resource;
      check(
        performance.now() < deadline,
        `Timed out waiting for ${source}: ${status}`,
      );
      await this.client.waitForFrame(state.tick);
    }
  }

  private async observeEvent(result: GeometryPickResultEvent) {
    await this.record("camera_event", result);
    check(
      result.session === this.client.session,
      "Event must identify its live session",
    );
    check(
      result.requestId > 0n,
      "Event must retain a nonzero request identity",
    );
    check(result.tick > 0n, "Event must report its evaluated tick");
    check(
      !this.events.some((previous) => previous.requestId === result.requestId),
      "Every request must have one distinct terminal response",
    );
    this.events.push(result);
  }
}

export const cameraCases = [
  {
    name: "optional view planes follow the evaluated camera at off-center hits",
    run: viewPlanes,
  },
  {
    name: "camera commands retain partial edits and allow explicit recovery",
    run: cameraLifetime,
  },
  {
    name: "camera navigation preserves orbit pivots and projection-scaled pan and zoom",
    run: cameraNavigation,
  },
  {
    name: "geometry boxes use camera projection and nearest world distances",
    run: boxPicking,
  },
  {
    name: "compound geometry preserves holes and recovers after failed activation",
    run: compoundPicking,
  },
] as const;

async function viewPlanes(fixture: CameraFixture) {
  const target = await fixture.target("view-plane-target", {
    sx: 4,
    sy: 4,
    sz: 4,
  });
  for (const projection of [0, 1]) {
    const camera = await fixture.camera(
      `view-plane-${projection}`,
      { z: 6 },
      { projection },
    );
    if (projection === 0) {
      const missing = await fixture.pick(
        0.5,
        0.5,
        CAMERA_VIEWPORT,
        true,
        await fixture.output(camera),
      );
      check(
        !missing.ok && missing.error === "InvalidEntity",
        "Plane requests require an explicitly selected root view",
      );
    }
    await fixture.activate(camera);
    const ordinary = picked(await fixture.pick(0.55, 0.45), target);
    const disabled = picked(
      await fixture.pick(0.55, 0.45, CAMERA_VIEWPORT, false),
      target,
    );
    equal(
      hitGeometry(disabled),
      hitGeometry(ordinary),
      "Explicit false preserves default results",
    );
    const included = picked(
      await fixture.pick(0.55, 0.45, CAMERA_VIEWPORT, true),
      target,
    );
    equal(
      included.viewPlane!.normal,
      [0, 0, -1],
      "Off-center plane uses camera forward rather than pointer ray",
    );
    equal(
      included.position,
      ordinary.position,
      "Plane request preserves intersection",
    );
    const projected = await fixture.project(0.55, 0.45, included.viewPlane!);
    check(
      projected.ok &&
        projected.position !== null &&
        outputProducer(projected.view.output)?.entity === camera,
      "Projection uses the picked camera",
    );
    projected.position.forEach((value, index) =>
      close(
        value,
        included.position[index]!,
        "Same pointer projects to original grab point",
      ),
    );
    const outside = await fixture.project(1.5, -0.5, included.viewPlane!);
    check(
      outside.ok && outside.position !== null,
      "Pointer capture projects outside viewport bounds",
    );
    close(
      outside.position[2],
      included.position[2],
      "Captured projection preserves depth",
    );
    const parallel = await fixture.project(0.5, 0.5, {
      point: [1, 0, 0],
      normal: [1, 0, 0],
    });
    check(
      parallel.ok && parallel.position === null,
      "Parallel rays have no unique intersection",
    );
    const invalidPlane = await fixture.project(0.5, 0.5, {
      point: [0, 0, 0],
      normal: [0, 0, 0],
    });
    check(
      !invalidPlane.ok && invalidPlane.error === "InvalidValue",
      "Zero plane normals fail explicitly",
    );
    const miss = await fixture.pick(0, 0, CAMERA_VIEWPORT, true);
    check(
      miss.ok && miss.hit === null,
      "Requested plane does not turn a miss into a hit",
    );

    successfulBatch(
      await fixture.batch(
        fixture.set(camera, "Transform", {
          x: 6,
          z: 0,
          qy: Math.SQRT1_2,
          qw: Math.SQRT1_2,
          sx: 2,
          sy: 3,
          sz: 4,
        }),
      ),
    );
    const rotated = picked(
      await fixture.pick(0.52, 0.48, CAMERA_VIEWPORT, true),
      target,
    );
    rotated.viewPlane!.normal.forEach((value, index) =>
      close(
        value,
        [-1, 0, 0][index]!,
        "Rotated/scaled camera determines unit plane normal",
      ),
    );
    const rotatedProjection = await fixture.project(
      0.52,
      0.48,
      rotated.viewPlane!,
    );
    check(
      rotatedProjection.ok && rotatedProjection.position !== null,
      "Rotated projection intersects retained plane",
    );
    rotatedProjection.position.forEach((value, index) =>
      close(
        value,
        rotated.position[index]!,
        "Projection follows effective camera rotation and scale",
      ),
    );
  }
  const invalid = await fixture.pick(
    0.5,
    0.5,
    { width: 0, height: 240, devicePixelRatio: 1 },
    true,
  );
  check(
    !invalid.ok && invalid.error === "InvalidViewport",
    "A viewport other than the bound view remains an error",
  );
  await rejects(
    fixture.host.setRootOutput(fixture.selected().output, {
      width: 0,
      height: 240,
      devicePixelRatio: 1,
    }),
    "InvalidValue",
    "An empty root viewport cannot be bound",
  );
}

async function cameraLifetime(fixture: CameraFixture) {
  const first = await fixture.camera("camera-a");
  const second = await fixture.camera("camera-b", { x: 3, z: 6 });
  check(
    (await fixture.rootBinding()) === null,
    "New Worlds must have no implicit root view",
  );
  const missing = await fixture.pick(
    0.5,
    0.5,
    CAMERA_VIEWPORT,
    undefined,
    await fixture.output(first),
  );
  check(
    !missing.ok && missing.error === "InvalidEntity",
    "Queries must not fall back to an unselected camera",
  );
  const incomplete = await fixture.create("incomplete-camera", {
    Transform: {},
  });
  await rejects(
    fixture.select(incomplete),
    "MissingComponent",
    "A camera needs both effective components",
  );
  await rejects(
    fixture.select(0xffffffffffffffffn),
    "MissingComponent",
    "Stale camera handles must fail",
  );
  check(
    (await fixture.rootBinding()) === null,
    "Rejected selections must not bind a root view",
  );
  activated(await fixture.activate(first), first);
  const repeated = fixture.selected();
  activated(await fixture.activate(first), first);
  check(
    sameOutputReference(fixture.selected().output, repeated.output) &&
      fixture.selected().generation.serial > repeated.generation.serial,
    "An equal explicit rebind receives a fresh root generation",
  );
  const replaced = await fixture.pickView({
    kind: "bound",
    binding: repeated,
  });
  check(
    !replaced.ok && replaced.error === "InvalidEntity",
    "A replaced binding generation cannot address the current view",
  );
  activated(
    await fixture.pickView({ kind: "bound", binding: fixture.selected() }),
    first,
  );

  // Host requests are pipelined in order. Every selection retains its own generation.
  const ordered = await Promise.all([
    fixture.select(second),
    fixture.select(first),
    fixture.select(second),
  ]);
  fixture.binding = ordered[2];
  activated(await fixture.pick(), second);
  equal(
    ordered.map((binding) => outputProducer(binding.output)?.entity),
    [second, first, second],
    "Ordered selections must each bind their requested camera",
  );
  ordered.forEach((binding, index) =>
    check(
      index === 0 ||
        binding.generation.serial > ordered[index - 1]!.generation.serial,
      "Ordered selections must acknowledge increasing generations",
    ),
  );
  equal(
    await fixture.rootBinding(),
    ordered[2],
    "The last explicit selection is the root binding",
  );
  const firstOutput = await fixture.output(first);
  const secondOutput = await fixture.output(second);
  const beforeActivation = fixture.pick(
    0.5,
    0.5,
    CAMERA_VIEWPORT,
    undefined,
    secondOutput,
  );
  const firstSelection = fixture.select(first);
  const betweenActivations = fixture.pick(
    0.5,
    0.5,
    CAMERA_VIEWPORT,
    undefined,
    firstOutput,
  );
  const secondSelection = fixture.select(second);
  await Promise.all([firstSelection, secondSelection]);
  fixture.binding = await secondSelection;
  for (const [query, camera] of [
    [await beforeActivation, second],
    [await betweenActivations, first],
  ] as const) {
    check(
      query.ok
        ? outputProducer(query.view.output)?.entity === camera
        : query.error === "InvalidEntity",
      "Interleaved queries use their named root output or fail, never a replacement",
    );
  }
  const retained = await fixture.rootBinding();
  await rejects(
    fixture.select(incomplete),
    "MissingComponent",
    "Invalid selection must fail explicitly",
  );
  equal(
    await fixture.rootBinding(),
    retained,
    "Invalid selection must preserve the root binding",
  );
  activated(await fixture.pick(), second);

  for (const name of ["entity", "Camera", "Transform"]) {
    const camera = await fixture.camera(`partial-camera-${name}`);
    activated(await fixture.activate(camera), camera);
    await fixture.partialCameraEdit(
      name === "entity" ? fixture.delete(camera) : fixture.remove(camera, name),
      name,
      name === "Transform",
    );
    const remaining = (await fixture.inspect()).entities.find(
      (entity) => entity.id === camera,
    );
    check(
      name === "entity"
        ? remaining === undefined
        : !remaining!.components.some(
            (value) => value.component === fixture.client.components[name]!.id,
          ),
      "Camera deletion or component removal remains applied",
    );
    activated(await fixture.activate(first), first);
    if (remaining)
      successfulBatch(await fixture.batch([fixture.delete(camera)]));
  }
  const projection = (await cameraFields(fixture, first)).camera;
  const rejected = await fixture.partialCameraEdit(
    fixture.set(first, "Camera", { near: 200 })[0]!,
    "projection",
    true,
  );
  check(
    rejected.operation === 1 && rejected.reason === "InvalidValue",
    "An out-of-range Camera write is rejected at its operation",
  );
  equal(
    (await cameraFields(fixture, first)).camera,
    projection,
    "A rejected Camera write leaves the component unchanged",
  );
  activated(await fixture.pick(), first);
  const corrected = { ...ORTHOGRAPHIC_CAMERA, near: 0.2 };
  successfulBatch(
    await fixture.batch([
      insertComponent(
        fixture.client,
        "Camera",
        { kind: "handle", id: first },
        corrected,
      ),
    ]),
  );
  close(
    (await cameraFields(fixture, first)).camera.near!,
    corrected.near,
    "The explicit Camera replacement applies",
  );
  const superseded = await fixture.pick();
  check(
    !superseded.ok && superseded.error === "InvalidEntity",
    "A replaced Camera component leaves its former output selection unavailable",
  );
  activated(await fixture.activate(first), first);

  activated(await fixture.activate(second), second);
  successfulBatch(await fixture.batch([fixture.delete(first)]));
  const after = await fixture.inspect();
  check(
    !after.entities.some((entity) => entity.id === first),
    "Switching cameras must permit old-camera deletion",
  );
  activated(await fixture.pick(), second);
  return { session: fixture.client.session, events: fixture.events };
}

async function cameraNavigation(fixture: CameraFixture) {
  const defaults = await fixture.create("navigation-defaults", {
    Transform: { z: 6 },
    Camera: {},
  });
  close(
    (await cameraFields(fixture, defaults)).camera.focus_distance!,
    6,
    "Generated camera creation uses the default focus distance",
  );
  const cleared = await fixture.select(defaults);
  await fixture.host.clearRootOutput(cleared);
  check(
    (await fixture.rootBinding()) === null,
    "Clearing the exact binding withdraws the root view",
  );
  const initial = await fixture.inspect();
  for (const motion of [
    { kind: "rotate", yaw: 0.2, pitch: -0.1 },
    { kind: "pan", x: 0.1, y: 0.1 },
    { kind: "zoom", amount: Math.log(2) },
  ] as const)
    await rejects(
      fixture.navigate(motion, cleared),
      "InvalidEntity",
      "Navigation requires a current root binding",
    );
  equal(
    (await fixture.inspect()).entities,
    initial.entities,
    "Navigation without a selected root view must preserve the scene",
  );
  const missing = await fixture.pick(
    0.5,
    0.5,
    CAMERA_VIEWPORT,
    undefined,
    cleared.output,
  );
  check(
    !missing.ok && missing.error === "InvalidEntity",
    "Navigation must not invent a root view",
  );

  const target = await fixture.target(
    "orbit-pivot",
    {},
    {
      geometry: fixture.encodeGeometry({ type: "sphere", radius: 0.05 }),
    },
  );
  for (const projection of [0, 1]) {
    const camera = await fixture.camera(
      `navigation-${projection}`,
      { z: 6 },
      { projection },
    );
    activated(await fixture.activate(camera), camera);
    await fixture.navigate({
      kind: "rotate",
      yaw: Math.PI / 4,
      pitch: -Math.PI / 8,
    });
    picked(await fixture.pick(), target);
    const rotated = await cameraFields(fixture, camera);
    const forward = rotateVector(rotated.transform, [0, 0, -1]);
    ["x", "y", "z"].forEach((axis, index) =>
      close(
        rotated.transform[axis]! +
          forward[index]! * rotated.camera.focus_distance!,
        0,
        "Orbit must preserve the focus pivot",
      ),
    );
    close(
      rotated.camera.focus_distance!,
      6,
      "Rotation preserves focus distance",
    );

    await fixture.navigate({ kind: "pan", x: 0.15, y: -0.1 });
    picked(await fixture.pick(0.65, 0.4), target);
    const panned = await cameraFields(fixture, camera);
    const height = projection === 0 ? 12 * Math.tan(Math.PI / 8) : 4;
    const right = rotateVector(rotated.transform, [1, 0, 0]);
    const up = rotateVector(rotated.transform, [0, 1, 0]);
    ["x", "y", "z"].forEach((axis, index) =>
      close(
        panned.transform[axis]! - rotated.transform[axis]!,
        -right[index]! * height * (320 / 240) * 0.15 -
          up[index]! * height * 0.1,
        "Pan converts normalized pointer deltas through projection and camera axes",
      ),
    );

    await fixture.navigate({ kind: "zoom", amount: Math.log(2) });
    picked(await fixture.pick(0.575, 0.45), target);
    const zoomed = await cameraFields(fixture, camera);
    if (projection === 0) {
      close(
        zoomed.camera.focus_distance!,
        12,
        "Perspective logarithmic zoom changes focus distance",
      );
      close(
        zoomed.camera.ortho_height!,
        4,
        "Perspective zoom preserves orthographic extent",
      );
      ["x", "y", "z"].forEach((axis, index) =>
        close(
          zoomed.transform[axis]! - panned.transform[axis]!,
          -forward[index]! * 6,
          "Perspective zoom moves the eye away from its pivot",
        ),
      );
    } else {
      close(
        zoomed.camera.ortho_height!,
        8,
        "Orthographic logarithmic zoom changes vertical extent",
      );
      close(
        zoomed.camera.focus_distance!,
        6,
        "Orthographic zoom preserves focus distance",
      );
      equal(
        zoomed.transform,
        panned.transform,
        "Orthographic zoom preserves the eye",
      );
    }

    await rejects(
      fixture.navigate({ kind: "zoom", amount: 100 }),
      "InvalidValue",
      "Unrepresentable navigation is rejected",
    );
    equal(
      await cameraFields(fixture, camera),
      zoomed,
      "Unrepresentable navigation preserves every component",
    );
    await Promise.all([
      fixture.navigate({ kind: "rotate", yaw: 0, pitch: 0 }),
      fixture.navigate({ kind: "pan", x: 0, y: 0 }),
      fixture.navigate({ kind: "zoom", amount: 0 }),
    ]);
    equal(
      await cameraFields(fixture, camera),
      zoomed,
      "Zero navigation deltas preserve state",
    );

    successfulBatch(
      await fixture.batch([
        ...fixture.set(camera, "Transform", {
          x: 0,
          y: 0,
          z: 6,
          qx: 0,
          qy: 0,
          qz: 0,
          qw: 1,
        }),
        ...fixture.set(camera, "Camera", {
          focus_distance: 6,
          ortho_height: 4,
        }),
      ]),
    );
    // Both requests are queued before either completes, in submission order.
    await Promise.all([
      fixture.navigate({ kind: "zoom", amount: Math.log(2) }),
      fixture.navigate({ kind: "pan", x: 0.1, y: 0 }),
    ]);
    const ordered = await cameraFields(fixture, camera);
    close(
      ordered.transform.x!,
      -height * 2 * (320 / 240) * 0.1,
      "Queued pan must use the preceding zoom's projection",
    );
    picked(await fixture.pick(0.6, 0.5), target);

    const other = await fixture.camera(
      `other-${projection}`,
      { x: 3, z: 6 },
      { projection },
    );
    const replaced = fixture.selected();
    activated(await fixture.activate(other), other);
    await rejects(
      fixture.navigate({ kind: "pan", x: 0.1, y: 0 }, replaced),
      "InvalidEntity",
      "A replaced root binding cannot navigate its former camera",
    );
    await fixture.navigate({ kind: "pan", x: 0.1, y: 0 });
    equal(
      await cameraFields(fixture, camera),
      ordered,
      "Navigation must only mutate the selected entity",
    );
    const moved = await cameraFields(fixture, other);
    close(
      moved.transform.x!,
      3 - height * (320 / 240) * 0.1,
      "New selection receives subsequent navigation",
    );

    // Navigation works on the one stored Transform: a client write is the
    // value the next pan starts from.
    successfulBatch(
      await fixture.batch(fixture.set(other, "Transform", { x: 10 })),
    );
    await fixture.navigate({ kind: "pan", x: 0.1, y: 0 });
    const written = await cameraFields(fixture, other);
    close(
      written.transform.x!,
      10 - height * (320 / 240) * 0.1,
      "Navigation must pan from the last written Transform",
    );
  }
  return { session: fixture.client.session, events: fixture.events };
}

async function cameraFields(fixture: CameraFixture, id: bigint) {
  const inspection = await fixture.inspect();
  const entity = inspection.entities.find((entity) => entity.id === id);
  check(entity !== undefined, "Navigation camera must remain live");
  const fields = (name: "Transform" | "Camera") => {
    const component = entity.components.find(
      (value) => value.component === fixture.client.components[name]!.id,
    );
    check(component !== undefined, `Navigation must preserve ${name}`);
    const result: Record<string, number> = {};
    for (const [field, value] of Object.entries(component.fields)) {
      check(
        typeof value === "number",
        `Expected numeric camera field ${field}`,
      );
      result[field] = value;
    }
    return result;
  };
  return { transform: fields("Transform"), camera: fields("Camera") };
}

function rotateVector(
  transform: Record<string, number>,
  vector: readonly number[],
) {
  const q = [transform.qx!, transform.qy!, transform.qz!];
  const cross = (a: readonly number[], b: readonly number[]) => [
    a[1]! * b[2]! - a[2]! * b[1]!,
    a[2]! * b[0]! - a[0]! * b[2]!,
    a[0]! * b[1]! - a[1]! * b[0]!,
  ];
  const t = cross(q, vector).map((value) => 2 * value);
  const second = cross(q, t);
  return vector.map(
    (value, index) => value + transform.qw! * t[index]! + second[index]!,
  );
}

async function boxPicking(fixture: CameraFixture) {
  const camera = await fixture.camera("orthographic-camera");
  activated(await fixture.activate(camera), camera);
  const far = await fixture.target("far-box", { z: -1 });
  const near = await fixture.target("near-nonuniform-box", {
    z: 1,
    sx: 0.5,
    sy: 2,
    sz: 3,
    qz: Math.sin(Math.PI / 8),
    qw: Math.cos(Math.PI / 8),
  });
  const hit = picked(await fixture.pick(), near);
  close(
    hit.distance,
    3.5,
    "Nearest hit uses world distance despite nonuniform scale",
  );
  hit.position.forEach((value, index) =>
    close(value, [0, 0, 2.5][index]!, "Box world position"),
  );
  check(hit.part === 0, "A single primitive has stable part zero");
  const miss = await fixture.pick(0.02, 0.02);
  check(
    miss.ok && miss.hit === null,
    "A screen ray outside every box must miss",
  );
  successfulBatch(await fixture.batch([fixture.delete(near)]));
  picked(await fixture.pick(), far);

  const offset = await fixture.target("translated-box", {
    x: 1.5,
    y: 0.75,
    z: 0,
  });
  const screenX =
    0.5 + 1.5 / ((4 * CAMERA_VIEWPORT.width) / CAMERA_VIEWPORT.height);
  const screenY = 0.5 - 0.75 / 4;
  picked(await fixture.pick(screenX, screenY), offset);
  const wide = { width: 640, height: 240, devicePixelRatio: 1 };
  await fixture.select(camera, wide);
  picked(
    await fixture.pick(0.5 + 1.5 / ((4 * 640) / 240), screenY, wide),
    offset,
  );
  await fixture.select(camera);
  const flipped = await fixture.pick(screenX, 1 - screenY);
  check(
    flipped.ok && flipped.hit === null,
    "Viewport y must increase downwards",
  );
  successfulBatch(
    await fixture.batch(fixture.set(camera, "Camera", { far: 4 })),
  );
  const clipped = await fixture.pick();
  check(
    clipped.ok && clipped.hit === null,
    "Camera clipping interval must exclude distant targets",
  );
  successfulBatch(
    await fixture.batch(
      fixture.set(camera, "Camera", { far: 100, projection: 0 }),
    ),
  );
  picked(await fixture.pick(), far);
  const invalid = await fixture.pick(-0.1, 0.5);
  check(
    !invalid.ok && invalid.error === "InvalidViewport",
    "Out-of-range positions fail explicitly",
  );
  return { session: fixture.client.session, events: fixture.events };
}

export async function compoundPicking(fixture: CameraFixture) {
  const camera = await fixture.camera("compound-camera");
  activated(await fixture.activate(camera), camera);
  const source = clientAssetSource(fixture.client.session, 6, 700n).source;
  const target = await fixture.target("ring-target", {}, { source });
  const pending = await fixture.pick();
  const projection = await fixture.project(0.5, 0.5, {
    point: [0, 0, 0],
    normal: [0, 0, -1],
  });
  check(
    projection.ok && projection.position !== null,
    "Projection is independent of pending geometry",
  );
  check(
    !pending.ok && pending.error === "GeometryUnavailable",
    "Unknown geometry cannot prove a miss",
  );
  const reference = await fixture.waitForGeometry(source, "failed");
  await fixture.uploadGeometry(700n);
  const loaded = await fixture.waitForGeometry(source);
  check(loaded.id === reference.id, "Late geometry retains resource identity");
  const shared = await fixture.target(
    "shared-ring-target",
    { x: 4 },
    { source },
  );
  const state = await fixture.inspect();
  check(
    state.resources.filter((resource) => resource.source === source).length ===
      1,
    "Instances share an immutable definition",
  );
  const hole = await fixture.pick();
  check(
    hole.ok && hole.hit === null,
    "Compound picking preserves the central hole",
  );
  const x = 0.5 + 0.7 / ((4 * CAMERA_VIEWPORT.width) / CAMERA_VIEWPORT.height);
  const hit = picked(await fixture.pick(x, 0.5, CAMERA_VIEWPORT, true), target);
  equal(hit.viewPlane!.normal, [0, 0, -1], "View plane uses camera forward");
  close(hit.distance, 6, "Compound hit distance");
  check(hit.part === 1, "The right rim retains its authored part identity");
  successfulBatch(
    await fixture.batch(fixture.set(target, "Transform", { qy: 1, qw: 0 })),
  );
  check(
    picked(await fixture.pick(x, 0.5), target).part === 3,
    "Rotation moves the left part into the ray",
  );
  successfulBatch(
    await fixture.batch(
      fixture.set(target, "PickingGeometry", {
        source: "",
        geometry: fixture.encodeGeometry({ type: "sphere", radius: 0.5 }),
      }),
    ),
  );
  picked(await fixture.pick(), target);
  await fixture.rejectGeometry(target);
  successfulBatch(
    await fixture.batch([fixture.delete(target), fixture.delete(shared)]),
  );
  return { session: fixture.client.session, events: fixture.events };
}

function activated(result: GeometryPickResultEvent, camera: bigint) {
  check(
    result.ok && outputProducer(result.view.output)?.entity === camera,
    `Query must observe selected camera ${camera}`,
  );
}

/** Hit identity and geometry, independent of the publication each query read. */
function hitGeometry(hit: GeometryPickHit) {
  const { publication: _publication, ...geometry } = hit;
  return geometry;
}

/** Host replies carry the core reason; World-session rejections prefix their code. */
async function rejects(
  operation: Promise<unknown>,
  reason: string,
  message: string,
) {
  const error = await operation.then(
    () => undefined,
    (error: unknown) => error,
  );
  check(
    error instanceof Error &&
      (error.message === reason || error.message.endsWith(`: ${reason}`)),
    `${message}: expected ${reason}, got ${String(error)}`,
  );
}

function picked(result: GeometryPickResultEvent, entity: bigint) {
  check(result.ok && result.hit !== null, `Expected a hit on ${entity}`);
  check(result.hit.entity === entity, `Expected nearest entity ${entity}`);
  check(
    Number.isFinite(result.hit.distance) &&
      result.hit.position.every(Number.isFinite),
    "Hits must contain finite world-space values",
  );
  return result.hit;
}

function check(value: boolean, message: string): asserts value {
  if (!value) throw new Error(message);
}

function close(actual: number, expected: number, message: string) {
  check(
    Math.abs(actual - expected) < 0.0001,
    `${message}: ${actual} != ${expected}`,
  );
}

function equal(actual: unknown, expected: unknown, message: string) {
  const json = (value: unknown) =>
    JSON.stringify(value, (_key, item: unknown) =>
      typeof item === "bigint" ? { $bigint: item.toString() } : item,
    );
  check(json(actual) === json(expected), message);
}
