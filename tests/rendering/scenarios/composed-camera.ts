/** Exact composed Camera operations shared by native and worker clients. */
import type {
  CameraAttachmentReference,
  Client,
  Command,
  GeometryEncoder,
  EntityRef,
  HostClientBase,
  PickingWorldClient,
  ViewQueryTarget,
} from "@ipp/client";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../../fixtures/commands.js";
import {
  ATTACHMENTS,
  CAMERA,
  CANVAS,
  SURFACE,
  selectSystems,
} from "../../fixtures/system-selections.js";
import { check } from "../../harness/page/checks.js";

function near(actual: number, expected: number): void {
  check(
    Math.abs(actual - expected) < 1e-5,
    `${actual} differs from ${expected}`,
  );
}

export async function exerciseComposedCamera(
  host: HostClientBase<Client>,
  encodeGeometry: GeometryEncoder,
): Promise<unknown> {
  const shell = (
    await host.createWorld({
      selectedSystems: selectSystems(CANVAS, SURFACE, ATTACHMENTS),
      canvas: { extent: [600, 400], unitsPerMetre: 200 },
      temporary: true,
    })
  ).reference;
  const root = (await host.openWorld(shell)) as PickingWorldClient;
  // Worlds have independent tick origins. Age the requester before creating
  // the Camera producer so a producer tick cannot masquerade as its reply tick.
  let rootFrame = await root.waitForFrame();
  for (let frame = 0; frame < 4; frame++)
    rootFrame = await root.waitForFrame(rootFrame.tick);
  const scene = (
    await host.createWorld({
      selectedSystems: selectSystems(CAMERA),
      temporary: true,
    })
  ).reference;
  const child = (await host.openWorld(scene)) as PickingWorldClient;
  try {
    const olderRoot = await root.inspect();
    const youngerChild = await child.inspect();
    check(
      olderRoot.tick > youngerChild.tick,
      "Fixture did not establish independent requester and producer tick origins",
    );
    const created = successfulBatch(
      await child.batch([
        createEntity(1, "camera"),
        insertComponent(
          child,
          "Camera",
          { kind: "alias", alias: 1 },
          { projection: 1, ortho_height: 4 },
        ),
        insertComponent(
          child,
          "Transform",
          { kind: "alias", alias: 1 },
          { z: 5 },
        ),
        createEntity(2, "object"),
        insertComponent(
          child,
          "PickingGeometry",
          { kind: "alias", alias: 2 },
          {
            geometry: encodeGeometry({
              type: "box",
              min: [-1, -1, -0.2],
              max: [1, 1, 0.2],
            }),
          },
        ),
      ]),
    );
    const output = await host.bindOutput(scene, aliasId(created, 1), "camera");
    const attachment = root.components.WorldAttachment!;
    const attach = (entity: EntityRef): Command => ({
      kind: "insertComponent",
      entity,
      component: attachment.id,
      fields: [
        {
          offset: attachment.fields.child!.offset,
          value: { kind: "world", value: scene },
        },
        {
          offset: attachment.fields.output!.offset,
          value: { kind: "output", value: output },
        },
        {
          offset: attachment.fields.mode!.offset,
          value: { kind: "u32", value: 2 },
        },
      ],
    });
    const mounted = successfulBatch(
      await root.batch([
        createEntity(1, "scene"),
        insertComponent(
          root,
          "FlatSurface",
          { kind: "alias", alias: 1 },
          { width: 3, height: 2 },
        ),
        attach({ kind: "alias", alias: 1 }),
      ]),
    );
    const anchor = aliasId(mounted, 1);
    const effect = mounted.effects.find((effect) => effect.kind === "written");
    check(
      effect?.kind === "written",
      "Attachment did not publish an exact receipt",
    );
    const { parent, incarnation, revision } = effect.receipt;
    const path: CameraAttachmentReference[] = [
      { parent, anchor, incarnation, revision },
    ];
    let binding = await host.setRootOutput(
      { kind: "canvas", world: shell },
      { width: 600, height: 400, devicePixelRatio: 1 },
    );
    const view = (): ViewQueryTarget => ({
      kind: "camera",
      root: { kind: "bound", binding },
      path,
    });
    const pick = await root.query({
      type: "GeometryPickQuery",
      view: view(),
      x: 0.5,
      y: 0.5,
      includeViewPlane: true,
    });
    check(
      pick.ok && pick.hit?.viewPlane,
      "Child camera did not return a scene drag plane",
    );
    check(
      pick.hit.entity === aliasId(created, 2),
      "Child pick hit the wrong entity",
    );
    check(
      pick.hit.world.id === scene.id &&
        pick.hit.world.incarnation === scene.incarnation,
      "Child pick lost World identity",
    );
    check(
      pick.hit.path.length === 1 &&
        pick.hit.path[0]!.world.id === shell.id &&
        pick.hit.path[0]!.anchor === anchor,
      "Child hit lost the root-to-Camera path",
    );
    near(pick.hit.position[2], 0.2);
    const plane = pick.hit.viewPlane;
    const project = await root.query({
      type: "CameraProjectQuery",
      view: view(),
      x: 0.75,
      y: 0.5,
      plane,
    });
    check(project.ok && project.position, "Child plane projection failed");
    near(project.position[0], 1.5); // Surface aspect 3/2, orthographic height 4.
    const beforeNavigation = await root.inspect();
    await root.navigateCamera({
      binding,
      path,
      motion: { kind: "pan", x: 0.1, y: 0 },
    });
    const afterNavigation = await root.inspect();
    check(
      afterNavigation.tick >= beforeNavigation.tick,
      "Camera navigation invalidated monotonic requester-session responses",
    );
    const moved = await root.query({
      type: "CameraProjectQuery",
      view: view(),
      x: 0.5,
      y: 0.5,
      plane,
    });
    check(
      moved.ok && moved.position,
      "Child navigation did not preserve composed projection",
    );
    near(moved.position[0], -0.6);
    await root.navigateCamera({
      binding,
      path,
      motion: { kind: "zoom", amount: Math.log(2) },
    });
    const zoomed = await root.query({
      type: "CameraProjectQuery",
      view: view(),
      x: 0.75,
      y: 0.5,
      plane,
    });
    check(zoomed.ok && zoomed.position, "Child zoom failed");
    near(zoomed.position[0], 2.4);
    await assertRejected(
      () =>
        root.navigateCamera({ binding, motion: { kind: "zoom", amount: 1 } }),
      "Canvas-root navigation inferred a child",
    );
    const oldBinding = binding;
    binding = await host.setRootOutput(binding.output, {
      width: 1200,
      height: 800,
      devicePixelRatio: 2,
    });
    await assertRejected(
      () =>
        root.navigateCamera({
          binding: oldBinding,
          path,
          motion: { kind: "pan", x: 1, y: 0 },
        }),
      "Old root binding navigated the child",
    );
    const scaled = await root.query({
      type: "CameraProjectQuery",
      view: view(),
      x: 0.75,
      y: 0.5,
      plane,
    });
    check(
      scaled.ok && scaled.position,
      "DPI resize invalidated current child view",
    );
    near(scaled.position[0], 2.4);
    // Equal-value producer writes publish a new identity and fence the old path.
    successfulBatch(await root.batch([attach({ kind: "handle", id: anchor })]));
    await assertRejected(
      () =>
        root.navigateCamera({
          binding,
          path,
          motion: { kind: "pan", x: 1, y: 0 },
        }),
      "Replaced attachment accepted its old path",
    );
    const stale = await root.query({
      type: "GeometryPickQuery",
      view: view(),
      x: 0.5,
      y: 0.5,
    });
    check(!stale.ok, "Replaced attachment accepted an old pick path");
    return { pick, project, moved, zoomed, scaled, stale };
  } finally {
    await root.close();
    await child.close();
    await host.destroyWorld(shell);
    await host.destroyWorld(scene);
  }
}

async function assertRejected(
  action: () => Promise<unknown>,
  message: string,
): Promise<void> {
  let rejected = false;
  try {
    await action();
  } catch {
    rejected = true;
  }
  check(rejected, message);
}
