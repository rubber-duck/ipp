/** Last-resort ownership of processes spawned by test environments.
 *
 * Environments stop their processes on success, failure, timeout and
 * cancellation. These hooks cover the paths that skip that cleanup: the test
 * process exiting early, or a signal whose default action would terminate it
 * before any finally block runs. A process killed with SIGKILL cannot run
 * hooks; the pipeline's process-group cleanup covers that path. */
import type { ChildProcess } from "node:child_process";

const INTERRUPTS = ["SIGINT", "SIGTERM", "SIGHUP"] as const;

const owned = new Set<ChildProcess>();
let installed = false;

function killOwned(): void {
  for (const child of owned)
    if (child.exitCode === null && child.signalCode === null)
      child.kill("SIGKILL");
  owned.clear();
}

function interrupted(signal: NodeJS.Signals): void {
  // Another handler owns this signal and cancels its scenarios gracefully;
  // the exit hook stays as the fallback.
  if (process.listenerCount(signal) > 1) return;
  killOwned();
  // Restore the default action so the interrupted process still terminates.
  process.off(signal, interrupted);
  process.kill(process.pid, signal);
}

/** Stop `child` if this process exits or is interrupted before cleanup. */
export function ownProcess(child: ChildProcess): void {
  if (!installed) {
    installed = true;
    process.once("exit", killOwned);
    for (const signal of INTERRUPTS) process.on(signal, interrupted);
  }
  owned.add(child);
  child.once("exit", () => owned.delete(child));
}
