import { ReactWorldCommits } from "../../packages/ipp-react/src/commits.js";

export function observeRootCutoff(entity: string) {
  const prototype = ReactWorldCommits.prototype;
  const original = prototype.checkpoint;
  let accept!: (value: { acknowledged: Promise<void> }) => void;
  const sealed = new Promise<{ acknowledged: Promise<void> }>((resolve) => {
    accept = resolve;
  });
  function checkpoint(this: ReactWorldCommits): Promise<void> {
    const acknowledged = original.call(this);
    try {
      this.resolveEntity(entity);
    } catch {
      return acknowledged;
    }
    if (!(this instanceof ReactWorldCommits))
      throw new Error("Cutoff observer did not share the reconciler module");
    restore();
    accept({ acknowledged });
    return acknowledged;
  }
  function restore() {
    if (prototype.checkpoint === checkpoint) prototype.checkpoint = original;
  }
  prototype.checkpoint = checkpoint;
  return { sealed, restore };
}
