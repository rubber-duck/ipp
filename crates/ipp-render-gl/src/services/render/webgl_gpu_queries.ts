/** Instrumentation-only timer bindings. Rust owns polling cadence and scope identity. */
export function createWebGlGpuQueries(
  gl: WebGL2RenderingContext,
  calls?: { query<T>(operation: () => T): T; queryExtensionCall(): void },
) {
  const context = gl;
  type TimerExtension = {
    TIME_ELAPSED_EXT: number;
    TIMESTAMP_EXT: number;
    GPU_DISJOINT_EXT: number;
    QUERY_COUNTER_BITS_EXT: number;
    queryCounterEXT(query: WebGLQuery, target: number): void;
  };
  let extension: TimerExtension | null = null;
  let timestampBits = 0;
  let capability = 0;
  let next = 0;
  let active = false;
  const queries = new Map<number, WebGLQuery>();
  const discover = () => {
    extension = context.getExtension(
      "EXT_disjoint_timer_query_webgl2",
    ) as TimerExtension | null;
    timestampBits = 0;
    capability = 0;
    if (!extension || context.isContextLost()) return;
    const elapsedBits = Number(
      context.getQuery(
        extension.TIME_ELAPSED_EXT,
        extension.QUERY_COUNTER_BITS_EXT,
      ),
    );
    timestampBits = Number(
      context.getQuery(
        extension.TIMESTAMP_EXT,
        extension.QUERY_COUNTER_BITS_EXT,
      ),
    );
    // JS query values are Numbers: do not advertise timestamp counters wider than exact integers.
    capability =
      timestampBits > 0 && timestampBits <= 53 ? 2 : elapsedBits > 0 ? 1 : 0;
  };
  discover();
  const reset = () => {
    if (!context.isContextLost()) {
      if (active && extension) context.endQuery(extension.TIME_ELAPSED_EXT);
      for (const query of queries.values()) context.deleteQuery(query);
    }
    queries.clear();
    active = false;
  };
  const api = {
    reset,
    restore() {
      reset();
      discover();
    },
    imports: {
      gpu_query_capability: () => capability,
      gpu_query_timestamp_bits: () => timestampBits,
      gpu_query_create: () => {
        if (!capability || context.isContextLost() || next >= 0xffff_ffff)
          return 0;
        const query = context.createQuery();
        if (!query) return 0;
        const id = ++next;
        queries.set(id, query);
        return id;
      },
      gpu_query_delete: (id: number) => {
        const query = queries.get(id);
        if (query && !context.isContextLost()) context.deleteQuery(query);
        queries.delete(id);
      },
      gpu_query_timestamp: (id: number) => {
        const query = queries.get(id);
        if (query && extension && !context.isContextLost()) {
          calls?.queryExtensionCall();
          extension.queryCounterEXT(query, extension.TIMESTAMP_EXT);
        }
      },
      gpu_query_begin: (id: number) => {
        const query = queries.get(id);
        if (query && extension && !active && !context.isContextLost()) {
          context.beginQuery(extension.TIME_ELAPSED_EXT, query);
          active = true;
        }
      },
      gpu_query_end: () => {
        if (active && extension && !context.isContextLost())
          context.endQuery(extension.TIME_ELAPSED_EXT);
        active = false;
      },
      gpu_query_available: (id: number) => {
        const query = queries.get(id);
        return query &&
          !context.isContextLost() &&
          context.getQueryParameter(query, context.QUERY_RESULT_AVAILABLE)
          ? 1
          : 0;
      },
      gpu_query_result: (id: number) => {
        const query = queries.get(id);
        if (!query || context.isContextLost()) return Number.NaN;
        const value = Number(
          context.getQueryParameter(query, context.QUERY_RESULT),
        );
        return Number.isSafeInteger(value) && value >= 0 ? value : Number.NaN;
      },
      gpu_query_disjoint: () =>
        extension &&
        !context.isContextLost() &&
        context.getParameter(extension.GPU_DISJOINT_EXT)
          ? 1
          : 0,
      gpu_query_context_lost: () => (context.isContextLost() ? 1 : 0),
    },
  };
  if (!calls) return api;
  const imports = Object.fromEntries(
    Object.entries(api.imports).map(([name, operation]) => [
      name,
      (...args: number[]) =>
        calls.query(() => Reflect.apply(operation, undefined, args)),
    ]),
  ) as typeof api.imports;
  return {
    ...api,
    reset: () => calls.query(api.reset),
    restore: () => calls.query(api.restore),
    imports,
  };
}
