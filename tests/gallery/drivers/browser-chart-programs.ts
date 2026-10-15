import type { Page, Worker } from "playwright";
import type { ProgramObservation } from "../scenarios/gallery-chart-visibility.js";

/** Diagnostic GL setup observations, separate from ordinary frame timing. */
export function observeChartPrograms(page: Page) {
  const workers: Worker[] = [];
  const pending: Promise<unknown>[] = [];
  page.on("worker", (worker) => {
    workers.push(worker);
    pending.push(
      worker.evaluate(() => {
        const prototype = globalThis.WebGL2RenderingContext?.prototype;
        if (!prototype) return;
        const observation: ProgramObservation = {
          programs: 0,
          calls: [],
          dropped: 0,
        };
        Object.assign(globalThis, { ippChartProgramObservation: observation });
        const create = prototype.createProgram;
        prototype.createProgram = function () {
          observation.programs++;
          return create.call(this);
        };
        for (const method of [
          "compileShader",
          "linkProgram",
          "getShaderParameter",
          "getProgramParameter",
        ] as const) {
          const original = prototype[method];
          Object.defineProperty(prototype, method, {
            value: function (this: WebGL2RenderingContext, ...args: unknown[]) {
              const start = performance.now();
              try {
                return Reflect.apply(original, this, args);
              } finally {
                if (observation.calls.length < 8192)
                  observation.calls.push({
                    method,
                    milliseconds: performance.now() - start,
                  });
                else observation.dropped++;
              }
            },
            configurable: true,
            writable: true,
          });
        }
      }),
    );
  });
  return async (): Promise<ProgramObservation> => {
    await Promise.all(pending);
    const observations = await Promise.all(
      workers.map((worker) =>
        worker.evaluate(
          () =>
            (
              globalThis as typeof globalThis & {
                ippChartProgramObservation?: ProgramObservation;
              }
            ).ippChartProgramObservation ?? null,
        ),
      ),
    );
    const supported = observations.filter((value) => value !== null);
    if (!supported.length)
      throw new Error("No rendering worker installed the GL program probe");
    return {
      programs: supported.reduce((sum, value) => sum + value.programs, 0),
      calls: supported.flatMap((value) => value.calls),
      dropped: supported.reduce((sum, value) => sum + value.dropped, 0),
    };
  };
}
