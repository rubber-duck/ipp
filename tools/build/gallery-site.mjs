/** Package the gallery's release runtime and application-owned assets for static hosts. */
import { cp, mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { dirname, extname, relative, resolve } from "node:path";
import { transform } from "esbuild";
import { artifact, bundleBrowser, workspace } from "./helpers.mjs";

// Development paths stay in the examples. The published entry module lives next
// to index.html, so these URLs work unchanged at /, /ipp/ or a custom subdirectory.
const locations = [
  ["/target/browser-build/render-expanded/", "./runtime/"],
  ["/target/gallery-build/", "./assets/"],
  ["/target/gallery-gui-assets/", "./assets/gui/"],
  ["/target/font-assets/", "./assets/shared/"],
  ["/target/gallery-platformer-assets/", "./assets/platformer/"],
];

export async function buildGallerySite(output) {
  const copy = async (source, destination) => {
    const target = resolve(output, destination);
    await mkdir(dirname(target), { recursive: true });
    await cp(resolve(workspace, source), target);
  };
  await bundleBrowser(
    resolve(workspace, "examples/world-gallery/main.tsx"),
    resolve(output, "gallery.js"),
    "production",
    {
      plugins: [
        {
          name: "gallery-site-locations",
          setup(build) {
            build.onLoad(
              { filter: /[/\\]examples[/\\]world-gallery[/\\].*\.[jt]sx?$/ },
              async ({ path }) => {
                let contents = await readFile(path, "utf8");
                for (const [source, destination] of locations)
                  contents = contents.replaceAll(source, destination);
                return {
                  contents,
                  loader: extname(path).slice(1),
                  resolveDir: dirname(path),
                };
              },
            );
          },
        },
      ],
    },
  );

  const css = await transform(
    await readFile(
      resolve(workspace, "examples/world-gallery/styles.css"),
      "utf8",
    ),
    { loader: "css", minify: true, legalComments: "none" },
  );
  await writeFile(resolve(output, "gallery.css"), css.code);
  const html = (
    await readFile(
      resolve(workspace, "examples/world-gallery/index.html"),
      "utf8",
    )
  )
    .replaceAll("/examples/world-gallery/styles.css", "./gallery.css")
    .replaceAll("/target/gallery-build/world-gallery.js", "./gallery.js")
    .replace(/>\s+</g, "><");
  await writeFile(resolve(output, "index.html"), html);
  await writeFile(resolve(output, ".nojekyll"), "");

  const runtime = "target/browser-build/render-expanded";
  await mkdir(resolve(output, "runtime"), { recursive: true });
  for (const name of await readdir(resolve(workspace, runtime))) {
    if (name.endsWith(".js")) {
      const { code } = await transform(
        await readFile(resolve(workspace, runtime, name), "utf8"),
        {
          loader: "js",
          format: "esm",
          target: "es2023",
          minify: true,
          legalComments: "none",
        },
      );
      await writeFile(resolve(output, "runtime", name), code);
    } else if (name === "runtime.wasm" || name === "SLUG-NOTICE") {
      await copy(`${runtime}/${name}`, `runtime/${name}`);
    }
  }

  for (const name of ["spot-marker.mesh", "sun-marker.mesh"])
    await copy(`target/gallery-build/${name}`, `assets/${name}`);
  await copy(
    "target/font-assets/shure-tech-mono.ippf",
    "assets/shared/shure-tech-mono.ippf",
  );
  for (const name of await readdir(
    resolve(workspace, "target/gallery-gui-assets"),
    { recursive: true, withFileTypes: true },
  )) {
    if (!name.isFile()) continue;
    const source = resolve(name.parentPath, name.name);
    const path = relative(
      resolve(workspace, "target/gallery-gui-assets"),
      source,
    );
    if ([".ippd", ".ippm", ".ippt", ".json"].includes(extname(path)))
      await copy(source, `assets/gui/${path}`);
  }

  const platformer = "target/gallery-platformer-assets";
  const catalog = JSON.parse(
    await readFile(resolve(workspace, platformer, "catalog.json"), "utf8"),
  );
  for (const name of [
    ...Object.keys(catalog),
    "platformer.ipp",
    "manifest.json",
    "route.json",
  ])
    await copy(`${platformer}/${name}`, `assets/platformer/${name}`);

  for (const name of [
    "ShureTechMono-OFL.txt",
    "ShureTechMono-Nerd-Font-README.md",
  ])
    await copy(`licences/fonts/${name}`, `notices/${name}`);
  for (const name of [
    "LICENSE-Platformer.txt",
    "LICENSE-Character-Animations.txt",
  ])
    await copy(
      `examples/world-gallery/worlds/platformer/authoring/${name}`,
      `notices/${name}`,
    );
  for (const name of ["react", "react-dom", "react-reconciler", "scheduler"])
    await copy(`node_modules/${name}/LICENSE`, `notices/${name}.txt`);

  const artifacts = [];
  for (const entry of await readdir(output, {
    recursive: true,
    withFileTypes: true,
  })) {
    if (!entry.isFile()) continue;
    const path = resolve(entry.parentPath, entry.name);
    artifacts.push({
      ...(await artifact(path)),
      path: relative(output, path).replaceAll("\\", "/"),
    });
  }
  artifacts.sort((a, b) => a.path.localeCompare(b.path));
  await writeFile(
    resolve(output, "build-report.json"),
    `${JSON.stringify(
      {
        profile: "render-expanded",
        compression:
          "Uncompressed files; the static host negotiates HTTP compression",
        bytes: artifacts.reduce((sum, entry) => sum + entry.bytes, 0),
        gzipBytes: artifacts.reduce((sum, entry) => sum + entry.gzipBytes, 0),
        artifacts,
      },
      null,
      2,
    )}\n`,
  );
  console.log("Built the standalone gallery site.");
}
