/** Context-owned, nonreused completion names. Polling never waits for the GPU. */
export function createWebGlCompletionFences(
  gl: WebGL2RenderingContext,
  options: { id(): number; live(): void; check(): void },
) {
  const fences = new Map<number, WebGLSync>();

  function remove(id: number, lost = false): void {
    const fence = fences.get(id);
    fences.delete(id);
    if (fence && !lost && !gl.isContextLost()) gl.deleteSync(fence);
  }

  return {
    create(): number {
      options.live();
      if (fences.size >= 4096)
        throw new Error("Completion fence capacity exhausted");
      const fence = gl.fenceSync(gl.SYNC_GPU_COMMANDS_COMPLETE, 0);
      let retained = false;
      try {
        gl.flush();
        options.check();
        if (!fence) throw new Error("Completion fence allocation failed");
        const id = options.id();
        fences.set(id, fence);
        retained = true;
        return id;
      } finally {
        if (!retained) gl.deleteSync(fence);
      }
    },
    poll(id: number): number {
      options.live();
      const fence = fences.get(id);
      if (!fence) throw new Error("Completion fence retired");
      const result = gl.clientWaitSync(fence, 0, 0);
      if (result === gl.TIMEOUT_EXPIRED) return 1;
      if (result === gl.ALREADY_SIGNALED || result === gl.CONDITION_SATISFIED)
        return 2;
      options.check();
      throw new Error("Completion fence wait failed");
    },
    remove,
    reset(lost: boolean): void {
      for (const id of fences.keys()) remove(id, lost);
    },
  };
}
