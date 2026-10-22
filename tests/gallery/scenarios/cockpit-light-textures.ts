/** Inspect the exact immutable texture bytes consumed by the real renderer. */
import assert from "node:assert/strict";
import type {
  CockpitRuntime,
  CockpitLightTile,
} from "../../../examples/demos/scenes/cockpit/runtime.js";

export async function assertCockpitLightTextures(
  runtime: CockpitRuntime,
  read: (source: string) => Promise<Uint8Array>,
) {
  const bodies = [
    ...runtime.lighting.bodies,
    ...runtime.exterior.bodies.filter((body) => body.role === "bridge"),
  ];
  const textures = new Map<string, Uint8Array>();
  let pairs = 0;
  for (const body of bodies) {
    const tiles = body.tiles!;
    for (const tile of tiles) {
      assert.deepEqual(tile.dimensions, [1024, 1024]);
      assert.equal(
        (tile.textureRect[2] - tile.textureRect[0]) * 1024,
        tile.sourcePixels[2] - tile.sourcePixels[0],
        "split preserves the source's horizontal pixel density",
      );
      assert.equal(
        (tile.textureRect[3] - tile.textureRect[1]) * 1024,
        tile.sourcePixels[3],
        "split preserves the source's vertical pixel density",
      );
      for (const role of ["baseTexture", "contributionTexture"] as const) {
        const bytes = await read(tile[role]);
        const header = new DataView(
          bytes.buffer,
          bytes.byteOffset,
          bytes.byteLength,
        );
        assert.equal(new TextDecoder().decode(bytes.subarray(0, 4)), "IPPT");
        assert.equal(header.getUint32(4, true), 3);
        assert.equal(header.getUint32(8, true), 1024);
        assert.equal(header.getUint32(12, true), 1024);
        assert.equal(bytes.length, 16 + 1024 * 1024 * 4);
        const pixels = bytes.subarray(16);
        if (role === "contributionTexture") {
          let emitting = 0;
          for (let i = 0; i < pixels.length; i += 4) {
            assert.equal(pixels[i], pixels[i + 1]);
            assert.equal(pixels[i], pixels[i + 2]);
            assert.equal(
              pixels[i],
              pixels[i + 3],
              "linear alpha carries the scalar contribution",
            );
            if (pixels[i]! > 100) emitting++;
          }
          assert.ok(
            emitting > 20 && emitting < 1024 * 1024 * 0.1,
            "contribution is confined to fixtures",
          );
        }
        textures.set(tile[role], pixels);
      }
      pairs++;
    }
    // Both sides of a split retain the same four source columns in their
    // guard bands. Hardware filtering can cross the split without a seam.
    for (let index = 1; index < tiles.length; index++) {
      const left = tiles[index - 1]!;
      const right = tiles[index]!;
      for (const role of ["baseTexture", "contributionTexture"] as const) {
        const a = textures.get(left[role])!;
        const b = textures.get(right[role])!;
        for (let y = 2; y < left.textureRect[3] * 1024; y++) {
          const x = left.textureRect[2] * 1024 - 2;
          assert.deepEqual(
            a.subarray((y * 1024 + x) * 4, (y * 1024 + x + 4) * 4),
            b.subarray(y * 1024 * 4, (y * 1024 + 4) * 4),
          );
        }
      }
    }
  }
  assert.equal(pairs, 11);
  // Landmarks measured on the unpowered source photos, independently of the
  // rasterized fixture outlines: lens centers, then neighboring non-light parts.
  for (const [name, lamps, casing] of [
    [
      "cockpit-console-shell",
      [
        [1084, 228],
        [770, 265],
        [1084, 304],
      ],
      [
        [1084, 205],
        [1084, 260],
        [1084, 360],
      ],
    ],
    [
      "cockpit-bridge-upper-left",
      [
        [45, 162],
        [395, 760],
        [840, 670],
      ],
      [
        [400, 600],
        [650, 605],
      ],
    ],
  ] as const) {
    const body = bodies.find((body) => body.entity === name);
    assert.ok(body, name);
    for (const [points, emitting] of [
      [lamps, true],
      [casing, false],
    ] as const) {
      for (const [x, y] of points) {
        const tile: CockpitLightTile = body.tiles!.find(
          (tile) => x >= tile.sourcePixels[0] && x < tile.sourcePixels[2],
        )!;
        const offset = ((y + 2) * 1024 + x - tile.sourcePixels[0] + 2) * 4;
        const base: Uint8Array = textures.get(tile.baseTexture)!;
        const weight = textures.get(tile.contributionTexture)![offset + 3]!;
        assert.equal(base[offset + 3], 255);
        if (emitting) {
          assert.ok(
            weight > 180,
            `${name} contribution coincides with lens ${x},${y}`,
          );
          assert.ok(
            base[offset]! > 25 && base[offset]! < 225,
            `${name} keeps its visible unpowered diffuser`,
          );
        } else
          assert.ok(
            weight < 4,
            `${name} casing/dial face ${x},${y} stays unlit`,
          );
      }
    }
  }
  return { squareTexturePairs: pairs };
}
