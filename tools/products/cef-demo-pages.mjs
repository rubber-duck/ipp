/** Bundle native local-page scripts without relying on file:// module origins. */
import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { resolve, join } from "node:path";
import { build } from "esbuild";

const source = resolve("examples/demos/scenes/cockpit/browser-pages");
const output = process.env.IPP_BUILD_OUTPUT ?? resolve("target/cef-demo-pages");
await mkdir(output, { recursive: true });
for (const name of await readdir(source)) {
  if (!name.endsWith(".html")) continue;
  const html = (await readFile(join(source, name), "utf8")).replace(
    'type="module" src="page.js"',
    'defer src="page.js"',
  );
  await writeFile(join(output, name), html);
}
await build({
  entryPoints: [join(source, "page.js")],
  outfile: join(output, "page.js"),
  bundle: true,
  platform: "browser",
  format: "iife",
  target: "chrome154",
});
