/** Optional Linux process CPU and DRM engine observations, outside timing loops. */
import { execFileSync } from "node:child_process";
import { readFile, readdir } from "node:fs/promises";

let ticksPerSecond: number | undefined;

export async function nativeUsage(pid: number) {
  const sampledAt = performance.now();
  if (process.platform !== "linux")
    return { sampledAt, unavailable: "Linux procfs required" };
  try {
    ticksPerSecond ??= Number(
      execFileSync("getconf", ["CLK_TCK"], { encoding: "utf8" }).trim(),
    );
    const stat = await readFile(`/proc/${pid}/stat`, "utf8");
    const fields = stat
      .slice(stat.lastIndexOf(")") + 2)
      .trim()
      .split(/\s+/);
    const cpuTicks = BigInt(fields[11]!) + BigInt(fields[12]!);
    const clients = new Map<
      string,
      {
        driver: string;
        device: string;
        client: string;
        enginesNs: Record<string, string>;
        raw: string;
      }
    >();
    for (const fd of await readdir(`/proc/${pid}/fdinfo`)) {
      const raw = await readFile(`/proc/${pid}/fdinfo/${fd}`, "utf8").catch(
        () => "",
      );
      const values = Object.fromEntries(
        raw
          .split("\n")
          .filter((line) => line.includes(":"))
          .map((line) => {
            const at = line.indexOf(":");
            return [line.slice(0, at), line.slice(at + 1).trim()];
          }),
      );
      const driver = values["drm-driver"],
        client = values["drm-client-id"],
        device = values["drm-pdev"] ?? "unspecified";
      if (!driver || !client) continue;
      const enginesNs: Record<string, string> = {};
      for (const [key, value] of Object.entries(values)) {
        const match = /^(\d+) ns$/.exec(value);
        if (key.startsWith("drm-engine-") && match)
          enginesNs[key.slice("drm-engine-".length)] = match[1]!;
      }
      // Duplicated descriptors for the same DRM client must not be added twice.
      clients.set(`${device}/${client}`, {
        driver,
        device,
        client,
        enginesNs,
        raw,
      });
    }
    return {
      sampledAt,
      pid,
      cpuTicks: String(cpuTicks),
      ticksPerSecond,
      drmClients: [...clients.values()],
      scope:
        "Host process CPU (all threads); driver-reported busy time per DRM client and engine. These scopes are distinct.",
    };
  } catch (error) {
    return { sampledAt, unavailable: String(error) };
  }
}
