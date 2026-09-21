/** Direct GUI commit tests capture local bindings before applying remote edits. */
import { GuiCommits } from "../src/gui/commits.js";
import type { GuiDescribedRoot } from "../src/gui/description.js";

export class TestGuiCommits extends GuiCommits {
  override apply(
    roots: readonly GuiDescribedRoot[],
    resolveEntity: (identity: number) => bigint | undefined,
  ): Promise<void> {
    this.reconcileLocal(roots);
    return super.apply(roots, resolveEntity);
  }
}
