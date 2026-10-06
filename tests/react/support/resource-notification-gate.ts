/** Delay real generated-client notifications without delaying Host loading or acknowledgements. */
import type { AssetResourceSnapshot, Client } from "@ipp/client";
import type { ReactWorldClient } from "@ipp/react";

export class ResourceNotificationGate {
  private held: (() => void)[] = [];
  private predicate: ((resource: AssetResourceSnapshot) => boolean) | undefined;

  wrap<T extends ReactWorldClient>(client: T): T {
    return new Proxy(client, {
      get: (target, property) => {
        if (property === "onResourceChange")
          return (listener: (resource: AssetResourceSnapshot) => void) =>
            target.onResourceChange!((resource) => {
              if (this.predicate?.(resource))
                this.held.push(() => listener(resource));
              else listener(resource);
            });
        const value = Reflect.get(target, property, target);
        return typeof value === "function" ? value.bind(target) : value;
      },
    });
  }

  hold(predicate: (resource: AssetResourceSnapshot) => boolean): void {
    this.predicate = predicate;
  }

  async wait(client: Pick<Client, "waitForFrame">): Promise<void> {
    for (let n = 0; n < 100; n++) {
      await client.waitForFrame();
      if (this.held.length) return;
    }
    throw new Error("No real resource notification reached the gate");
  }

  release(): void {
    this.predicate = undefined;
    for (const deliver of this.held.splice(0)) deliver();
  }
}
