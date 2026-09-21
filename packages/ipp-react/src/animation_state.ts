import type {
  AnimationControllerDescription,
  AnimationPlaybackControl,
} from "@ipp/client";
import type { ReactWorldClient } from "./contract.js";
import type { ReactAssetRegistry } from "./asset_state.js";
import type { AnimationDescription } from "./animation.js";
import { isAssetReference } from "./assets.js";
import { animationMutation, animationSignature } from "./animation_tree.js";

type Entry = {
  declaration: AnimationDescription;
  id?: bigint;
  signature?: string;
  driverSignature?: string;
  pending: AnimationPlaybackControl[];
};

/** All mutations share the root's acknowledgement queue. */
export class ReactAnimationRegistry {
  private readonly session: bigint;
  private readonly entries = new Map<number, Entry>();
  private desired = new Map<number, AnimationDescription>();
  private readonly unsubscribe?: (() => void) | undefined;
  private closed = false;

  constructor(
    private readonly client: ReactWorldClient,
    private readonly enqueue: (work: () => Promise<void>) => Promise<void>,
    private readonly report: (error: unknown) => unknown,
  ) {
    this.session = client.session;
    this.unsubscribe = client.onPlaybackEvent?.((event) => {
      if (this.closed || client.session !== this.session) return;
      for (const [identity, entry] of this.entries) {
        if (entry.id !== event.controller.id) continue;
        const declaration = this.desired.get(identity);
        if (!declaration?.mailbox.mounted) return;
        try {
          declaration.onPlaybackEvent?.(event);
        } catch (error) {
          this.report(error);
        }
      }
    });
  }

  setDesired(descriptions: readonly AnimationDescription[]) {
    this.desired = new Map(
      descriptions.map((description) => [description.identity, description]),
    );
  }

  private live(): boolean {
    return !this.closed && this.client.session === this.session;
  }

  async removeExcept(identities: ReadonlySet<number>): Promise<void> {
    for (const [identity, entry] of this.entries) {
      if (identities.has(identity)) continue;
      entry.declaration.mailbox.dispatch = () =>
        Promise.reject(new Error("Animation is unmounted"));
      entry.declaration.mailbox.pending = [];
      if (entry.id !== undefined && this.client.session === this.session)
        await this.client.deleteAnimationController!(entry.id);
      this.entries.delete(identity);
    }
  }

  async apply(
    descriptions: readonly AnimationDescription[],
    assets: ReactAssetRegistry,
    target: (identity: number) => bigint | undefined,
  ): Promise<void> {
    if (!this.live()) return;
    if (
      descriptions.length &&
      (!this.client.createAnimationController ||
        !this.client.updateAnimationController ||
        !this.client.deleteAnimationController ||
        !this.client.controlAnimationController ||
        !this.client.onPlaybackEvent)
    )
      throw new Error("Animation requires an animation-capable client");
    for (const declaration of descriptions) {
      if (!this.live()) return;
      if (!this.desired.has(declaration.identity)) continue;
      let entry = this.entries.get(declaration.identity);
      if (!entry) {
        entry = {
          declaration,
          pending: declaration.autoPlay ? [{ action: "play" }] : [],
        };
        this.entries.set(declaration.identity, entry);
        const owned = entry;
        declaration.mailbox.dispatch = (control) => {
          if (!this.live() || !this.desired.has(declaration.identity))
            return Promise.reject(new Error("Animation is unmounted"));
          return this.enqueue(async () => {
            if (!this.live() || !this.desired.has(declaration.identity)) return;
            if (owned.id === undefined) owned.pending.push(control);
            else
              await this.client.controlAnimationController!(owned.id, control);
          });
        };
        entry.pending.push(...declaration.mailbox.pending.splice(0));
      }
      entry.declaration = declaration;
      const drivers: AnimationControllerDescription["drivers"][number][] = [];
      for (const binding of declaration.bindings) {
        const resource = isAssetReference(binding.source)
          ? assets.get(binding.source.assetId)
          : undefined;
        // Do not combine a new clip's track hints with its previous source.
        if (
          isAssetReference(binding.source) &&
          (resource?.status !== "loaded" || !resource.current)
        )
          break;
        const entity =
          typeof binding.target === "bigint"
            ? binding.target
            : target(binding.target.entity);
        if (entity === undefined) break;
        drivers.push({
          ...binding,
          source:
            typeof binding.source === "string"
              ? binding.source
              : resource!.current!.source,
          variant: resource?.current?.variant ?? binding.variant ?? 0,
          target: entity,
        });
      }
      if (drivers.length !== declaration.bindings.length) continue;
      const description = {
        drivers,
        speed: declaration.speed,
        looping: declaration.looping,
      };
      const signature = animationSignature(description);
      const driverSignature = animationSignature(description.drivers);
      if (entry.id === undefined) {
        entry.id = await this.client.createAnimationController!(description);
      } else {
        const transition = declaration.transition;
        const mutation = animationMutation(
          entry.signature,
          entry.driverSignature,
          signature,
          driverSignature,
          transition !== undefined,
        );
        if (mutation === "transition") {
          if (!transition) throw new Error("Missing Animation transition");
          if (!this.client.transitionAnimationController)
            throw new Error(
              "Animation transitions require a transition-capable client",
            );
          await this.client.transitionAnimationController(entry.id, {
            description,
            ...transition,
          });
        } else if (mutation === "update") {
          await this.client.updateAnimationController!(entry.id, description);
        }
      }
      entry.signature = signature;
      entry.driverSignature = driverSignature;
      // Creation acknowledgement may arrive after local unmount. Retain the ID
      // for queued cleanup, but never start obsolete playback.
      if (!this.live() || !this.desired.has(declaration.identity)) continue;
      while (
        entry.pending.length &&
        this.live() &&
        this.desired.has(declaration.identity)
      ) {
        await this.client.controlAnimationController!(
          entry.id,
          entry.pending[0]!,
        );
        entry.pending.shift();
      }
    }
  }

  close() {
    this.closed = true;
    this.unsubscribe?.();
  }

  async dispose() {
    this.close();
    await this.removeExcept(new Set());
  }
}
