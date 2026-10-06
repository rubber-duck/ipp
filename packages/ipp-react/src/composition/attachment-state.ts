import {
  canvasOutput,
  outputProducer,
  type AttachmentEffect,
  type AttachmentReceipt,
  type BatchOutcome,
  type CanvasState,
  type CanvasStateUpdateCommand,
  type Client,
  type Command,
  type OutputReference,
  type WorldReference,
} from "@ipp/client";
import type { Ref } from "react";
import type { ReactWorldClient } from "../reconciler/world-client.js";
import {
  ReactWorldCommits,
  ReactWorldBatchRejectedError,
  type ReactWorldRootOptions,
} from "../reconciler/commits.js";
import { ReactWorldContainer, reconciler } from "../reconciler/host-config.js";
import {
  ReactWorldTree,
  type ReactWorldDescription,
} from "../reconciler/tree.js";
import type {
  AttachedWorldDescription,
  AttachedWorldHandle,
  AttachedWorldSlot,
  AttachedWorldCleanupJournal,
  AttachedWorldCleanupRecovery,
  ReactCompositionHost,
} from "./attached-world.js";
import { AttachedWorldCleanupError } from "./attached-world.js";
import { canvasStateUpdate } from "../canvas/world.js";
import { attachmentIdentity as identity } from "./attachment-identity.js";
import {
  controlTracking,
  type ControlTrackingLease,
} from "../reconciler/lifecycle-tracking.js";

function errorValue(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}

function sameWorld(left: WorldReference, right: WorldReference): boolean {
  return left.id === right.id && left.incarnation === right.incarnation;
}

/** Local non-submission or Host ingress rejection: the request had no effect. */
function requestNotSubmitted(error: unknown): boolean {
  return (
    error instanceof Error &&
    "code" in error &&
    (error.code === "IPP_REQUEST_NOT_SENT" ||
      error.code === "IPP_REQUEST_REJECTED")
  );
}

interface OutputTracking {
  readonly entity: bigint;
  readonly component: number;
  lease: ControlTrackingLease;
  failed: boolean;
}

class AttachmentScope {
  readonly active = new Map<AttachedWorldSlot, AttachmentRecord>();
  readonly records = new Set<AttachmentRecord>();
  private readonly departing = new Map<AttachedWorldSlot, AttachmentRecord>();
  private captureGeneration = 0;
  acknowledged = Promise.resolve();
  revision = 0;
  private writer: Client | undefined;
  private writerOpening: Promise<Client> | undefined;
  closing = false;

  constructor(
    readonly group: ReactAttachmentGroup,
    readonly container: ReactWorldContainer,
    readonly client: ReactWorldClient,
  ) {
    container.attachmentHost = group.host;
    container.onPublish = (description, acknowledged) => {
      if (this.acknowledged !== acknowledged) this.revision++;
      this.acknowledged = acknowledged;
      this.capture(description);
      group.schedule();
    };
    container.onFailure = (acknowledged) => {
      this.captureGeneration++;
      this.acknowledged = acknowledged;
      this.revision++;
      group.schedule();
    };
    // A session end deletes nothing: release, as unmount does.
    void client.closed?.then(() => {
      if (!this.closing) this.fence(true);
    });
  }

  private capture(description: ReactWorldDescription): void {
    if (this.closing) return;
    const generation = ++this.captureGeneration;
    const current = () =>
      !this.closing && this.captureGeneration === generation;
    const declarations = description.attachments ?? [];
    const wanted = new Set(declarations.map((item) => item.slot));
    for (const [slot, record] of this.active) {
      if (!wanted.has(slot)) {
        this.active.delete(slot);
        this.depart(record);
        record.cancel();
        if (!current()) return;
      }
    }
    for (const declaration of declarations) {
      let record = this.active.get(declaration.slot);
      if (
        record &&
        identity(record.description.child) !== identity(declaration.child)
      ) {
        if (this.active.get(declaration.slot) === record)
          this.active.delete(declaration.slot);
        this.depart(record);
        record.cancel();
        if (!current()) return;
        record = undefined;
      }
      if (record) {
        record.update(declaration);
        if (!current()) return;
      } else {
        const predecessor = this.departing.get(declaration.slot);
        record = new AttachmentRecord(this, declaration, predecessor);
        this.departing.delete(declaration.slot);
        this.active.set(declaration.slot, record);
        this.records.add(record);
        void record.prepare();
        if (!current()) return;
      }
    }
  }

  private depart(record: AttachmentRecord): void {
    const slot = record.description.slot;
    this.departing.set(slot, record);
  }

  receiptClient(): Promise<Client> {
    const world = this.client.worldReference;
    if (!world)
      return Promise.reject(
        new Error("AttachedWorld requires an exact parent World reference"),
      );
    if (this.writer) return Promise.resolve(this.writer);
    if (this.writerOpening) return this.writerOpening;
    const opening = this.group.host.openWorld(world);
    this.writerOpening = opening;
    void opening.then(
      (writer) => {
        this.writer = writer;
        if (this.writerOpening === opening) this.writerOpening = undefined;
      },
      () => {
        if (this.writerOpening === opening) this.writerOpening = undefined;
      },
    );
    return opening;
  }

  checkpoint(): Promise<void> {
    return Promise.all([
      this.container.commits.checkpoint(),
      ...[...this.active.values()].map((record) => record.checkpoint()),
    ]).then(() => {});
  }

  /**
   * Stop capturing declarations. With `release` (unmount, session end) each
   * active boundary is released and its attachment, child World and child
   * declarations stay; otherwise each is removed with them.
   */
  fence(release = false): void {
    if (this.closing) return;
    this.closing = true;
    this.captureGeneration++;
    if (release) this.container.commits.fence();
    for (const record of this.records)
      if (release) record.release();
      else record.cancel();
    this.active.clear();
  }

  async dispose(retry = false, release = false): Promise<void> {
    this.fence(release);
    const results = await Promise.allSettled(
      [...this.records].map((record) =>
        retry ? record.retryCleanup() : record.closed,
      ),
    );
    const errors = results.flatMap((result) =>
      result.status === "rejected" ? [result.reason] : [],
    );
    if (errors.length)
      throw new AggregateError(errors, "Attached World cleanup is incomplete");
    await this.closeWriter();
  }

  async release(record: AttachmentRecord): Promise<void> {
    this.records.delete(record);
    try {
      if (this.closing) await this.closeWriter();
    } catch (error) {
      this.records.add(record);
      throw error;
    }
    const slot = record.description.slot;
    if (this.departing.get(slot) === record) this.departing.delete(slot);
  }

  async abandon(): Promise<void> {
    this.fence(true);
    await Promise.all(
      [...this.records].map((record) => record.recovery.abandon()),
    );
    await this.closeWriter();
  }

  private async closeWriter(): Promise<void> {
    if (this.records.size) return;
    await this.writerOpening?.catch(() => {});
    const writer = this.writer;
    if (writer && !writer.closure) await writer.close();
  }
}

class AttachmentRecord {
  active = true;
  readonly closed: Promise<void>;
  readonly declarationsReleased: Promise<void>;
  private resolveDeclarations!: () => void;
  private rejectDeclarations!: (error: unknown) => void;
  private resolveClosed!: () => void;
  private rejectClosed!: (error: unknown) => void;
  private readonly cancelled: Promise<void>;
  private resolveCancelled!: () => void;
  private child: Client | undefined;
  private world: WorldReference | undefined;
  private created = false;
  private createStarted = false;
  private scope: AttachmentScope | undefined;
  private ready: AttachedWorldHandle | undefined;
  private callbacksFenced = false;
  private assignedRef: Ref<AttachedWorldHandle> | undefined;
  private releaseRef: (() => void) | undefined;
  private refGeneration = 0;
  private preparing: Promise<void> = Promise.resolve();
  private initializing: Promise<void> | undefined;
  private pending: Promise<void> = Promise.resolve();
  private submittedSignature: string | undefined;
  private generation = 0;
  private anchorRetirement: Promise<void> | undefined;
  private readonly receipts = new Map<bigint, AttachmentReceipt>();
  private readonly retirement = new Map<bigint, Promise<void>>();
  private readonly detached = new Set<bigint>();
  private retirementEpoch = 0;
  private current: AttachmentReceipt | undefined;
  private currentIdentity: string | undefined;
  /** The output last bound for the child's acknowledged output declaration. */
  private boundOutput: { witness: string; output: OutputReference } | undefined;
  /** Lifecycle watch on the bound output component in the child World. */
  private outputTracking: OutputTracking | undefined;
  /** Set once output tracking ended; later reconciles always bind again. */
  private outputTrackingEnded = false;
  /** The created child's canvas state as last created or sent. */
  private canvasState: CanvasState | undefined;
  private writer: Client | undefined;
  private unknown: unknown;
  private preparationError: unknown;
  private cleanup: Promise<void> | undefined;
  private recovering: Promise<void> | undefined;
  private abandoning: Promise<AttachedWorldCleanupJournal> | undefined;
  private cleanupRunning = false;
  /** Released by unmount or session end rather than removed. */
  private released = false;
  private declarationsCleaned = false;
  private completed = false;
  private abandoned = false;
  readonly recovery: AttachedWorldCleanupRecovery;

  constructor(
    readonly parent: AttachmentScope,
    public description: AttachedWorldDescription,
    private predecessor?: AttachmentRecord,
  ) {
    const record = this;
    this.cancelled = new Promise<void>((resolve) => {
      this.resolveCancelled = resolve;
    });
    this.recovery = Object.freeze({
      get journal() {
        return record.journal();
      },
      retry: () => this.retryCleanup(),
      abandon: () => this.abandonCleanup(),
    });
    this.closed = new Promise<void>((resolve, reject) => {
      this.resolveClosed = resolve;
      this.rejectClosed = reject;
    });
    void this.closed.catch((error) => this.parent.group.report(error));
    this.declarationsReleased = new Promise<void>((resolve, reject) => {
      this.resolveDeclarations = resolve;
      this.rejectDeclarations = reject;
    });
    void this.declarationsReleased.catch(() => {});
  }

  update(description: AttachedWorldDescription): void {
    const hiding = !this.description.suspended && description.suspended;
    if (
      this.description.signature !== description.signature ||
      this.description.suspended !== description.suspended
    )
      this.generation++;
    const changed =
      this.description.signature !== description.signature || hiding;
    this.description = description;
    if (changed) {
      this.ready = undefined;
      this.clearRef();
    }
    if (this.description !== description) return;
    this.updateCanvas();
    if (this.description !== description) return;
    if (hiding) this.suspend();
    if (this.assignedRef !== description.attachmentRef) {
      this.clearRef();
      if (this.description !== description) return;
      if (this.ready && this.usable()) this.assignRef(this.ready);
    }
  }

  private suspend(): void {
    this.submittedSignature = undefined;
    this.pending = this.pending
      .then(async () => {
        if (!this.current) return;
        const previous = this.current;
        await this.detach(previous);
        this.current = undefined;
        this.parent.group.retiring(this.retire(previous));
      })
      .catch((error) => this.fail(error));
    this.parent.group.track(this.pending);
  }

  private usable(): boolean {
    return (
      this.active &&
      !this.callbacksFenced &&
      !this.parent.closing &&
      !this.parent.client.closure &&
      !this.child?.closure
    );
  }

  fenceCallbacks(): void {
    this.callbacksFenced = true;
    this.clearRef();
  }

  prepare(): Promise<void> {
    this.preparing = (async () => {
      try {
        const predecessor = this.predecessor;
        this.predecessor = undefined;
        if (predecessor)
          await Promise.race([
            predecessor.declarationsReleased,
            this.cancelled,
          ]);
        if (!this.usable()) return;
        const name = this.description.child.create?.symbolicId;
        while (name !== undefined) {
          const retiring = this.parent.group.creatorRetirements(name);
          if (!retiring.length) break;
          await Promise.race([Promise.all(retiring), this.cancelled]);
          if (!this.usable()) return;
        }
        const work = this.prepareChild();
        const initializing = work.then(
          async () => {
            if (!this.usable() || !this.scope) return;
            reconciler.flushSyncWork();
            this.parent.container.publish();
            this.parent.group.publish();
            await this.scope.checkpoint();
          },
          () => {},
        );
        this.initializing = initializing;
        this.parent.group.track(initializing);
        this.reconcile();
        void initializing
          .finally(() => {
            if (this.initializing === initializing)
              this.initializing = undefined;
          })
          .catch(() => {});
        await work;
      } catch (error) {
        // Only a submitted create with an unknown outcome may own an unnamed
        // World. Known non-submission and failures before creation own nothing.
        if (!this.world && this.createStarted && !requestNotSubmitted(error))
          this.preparationError = error;
        this.fail(error);
        this.cancel();
      }
    })();
    return this.preparing;
  }

  checkpoint(): Promise<void> {
    return Promise.all([this.initializing, this.pending]).then(() => {});
  }

  private async prepareChild(): Promise<void> {
    const specification = this.description.child;
    if ("create" in specification && specification.create) {
      const request = this.description.canvas;
      const canvas: CanvasState | undefined = request && {
        extent: request.extent ?? [1, 1],
        unitsPerMetre: request.unitsPerMetre ?? 1,
      };
      this.createStarted = true;
      const created = await this.parent.group.host.createWorld(
        canvas ? { ...specification.create, canvas } : specification.create,
      );
      this.world = created.reference;
      this.created = true;
      this.canvasState = canvas;
    } else this.world = specification.borrow!;
    if (!this.usable()) return;
    this.child = await this.parent.group.host.openWorld(this.world);
    if (!this.usable()) return;
    if (this.child.closure) throw this.child.closure.reason;
    const container = new ReactWorldContainer(
      new ReactWorldTree(this.child),
      new ReactWorldCommits(this.child, this.parent.group.options),
    );
    this.scope = this.parent.group.add(container, this.child);
    this.updateCanvas();
    void this.child.closed.then(({ reason }) => {
      if (this.active) {
        this.fail(reason);
        this.cancel();
      }
    });
    this.description.slot.update(container);
  }

  reconcile(): void {
    const scope = this.scope;
    const initializing = this.initializing;
    const signature = `${this.generation}:${this.parent.revision}:${scope?.revision}`;
    if (
      !this.usable() ||
      this.description.suspended ||
      this.unknown ||
      this.anchorRetirement ||
      (!initializing &&
        (!scope || this.description.portal !== scope.container)) ||
      this.submittedSignature === signature
    )
      return;
    const description = this.description;
    this.submittedSignature = signature;
    const parentAck = this.parent.acknowledged;
    const childAck = scope?.acknowledged;
    const generation = this.generation;
    const current = () =>
      this.usable() &&
      !this.description.suspended &&
      generation === this.generation;
    let targetsCurrent: (() => boolean) | undefined;
    const desired = () =>
      current() &&
      !this.unknown &&
      !this.anchorRetirement &&
      (!targetsCurrent || targetsCurrent());
    this.pending = this.pending
      .catch(() => {})
      .then(async () => {
        let dependenciesSettled = false;
        try {
          await Promise.all([parentAck, childAck, initializing]);
          dependenciesSettled = true;
          if (
            !this.scope ||
            this.description.portal !== this.scope.container ||
            !desired()
          )
            return;
          const anchor = this.parent.container.commits.resolveEntity(
            description.anchor,
          );
          const output = await this.output(description);
          targetsCurrent = () => {
            try {
              if (
                this.parent.container.commits.resolveEntity(
                  description.anchor,
                ) !== anchor
              )
                return false;
              if (
                description.attachment.mode === "surface-camera" &&
                !("world" in description.attachment.output)
              )
                return (
                  this.scope!.container.commits.resolveEntity(
                    description.attachment.output.entity,
                  ) === (output && outputProducer(output)?.entity)
                );
              return true;
            } catch {
              return false;
            }
          };
          if (!desired()) return;
          this.writer = await this.parent.receiptClient();
          if (!desired()) return;
          if (this.current && this.current.anchor !== anchor) {
            const previous = this.current;
            await this.detach(previous);
            this.current = undefined;
            const retirement = this.retire(previous);
            this.anchorRetirement = retirement;
            this.parent.group.retiring(retirement);
            void retirement.then(
              () => {
                this.anchorRetirement = undefined;
                this.submittedSignature = undefined;
                this.parent.group.schedule();
              },
              (error) => this.fail(error),
            );
            return;
          }
          const component = this.writer.components.WorldAttachment;
          if (
            !component ||
            !this.writer.manifest?.components.includes(component.id)
          )
            throw new Error("The parent World does not select WorldAttachment");
          const selection = identity({
            anchor,
            child: this.world,
            mode: description.attachment.mode,
            output,
          });
          let receipt = this.current;
          if (!receipt || this.currentIdentity !== selection) {
            this.clearRef();
            this.ready = undefined;
            const operation: Command = {
              kind: "insertComponent",
              entity: { kind: "handle", id: anchor },
              component: component.id,
              fields: [
                {
                  offset: component.fields.child!.offset,
                  value: { kind: "world", value: this.world! },
                },
                {
                  offset: component.fields.mode!.offset,
                  value: {
                    kind: "u32",
                    value:
                      description.attachment.mode === "spatial"
                        ? 0
                        : description.attachment.mode === "surface-canvas"
                          ? 1
                          : 2,
                  },
                },
                {
                  offset: component.fields.output!.offset,
                  value: {
                    kind: "output",
                    value:
                      description.attachment.mode === "surface-camera"
                        ? output!
                        : null,
                  },
                },
              ],
            };
            if (!desired()) return;
            const outcome = await this.submit([operation]);
            const written = outcome.effects.find(
              (effect): effect is AttachmentEffect =>
                effect.operation === 0 && effect.kind === "written",
            );
            if (!written) {
              this.unknown = new Error(
                "Attachment acknowledgement omitted its receipt",
              );
              throw this.unknown;
            }
            this.current = written.receipt;
            this.currentIdentity = selection;
            receipt = written.receipt;
          }
          if (!desired()) return;
          if (this.ready) return;
          const record = this;
          const accepted = receipt;
          this.ready = {
            get world() {
              record.checkHandle(accepted, description, generation);
              return record.world!;
            },
            get output() {
              record.checkHandle(accepted, description, generation);
              return output;
            },
            closed: this.closed,
          };
          this.clearRef();
          const ready = this.ready;
          this.assignRef(ready);
          if (desired() && this.ready === ready)
            this.notify(() => this.description.onReady?.(ready));
        } catch (error) {
          // The originating scope reports rejected declaration batches. An
          // attachment waiting for that acknowledgement has no new failure.
          if (
            !dependenciesSettled &&
            error instanceof ReactWorldBatchRejectedError
          )
            return;
          if (
            current() &&
            (dependenciesSettled ||
              (parentAck === this.parent.acknowledged &&
                (childAck === undefined || childAck === scope?.acknowledged)))
          )
            this.fail(error);
          else this.parent.group.report(error);
        }
      });
    this.parent.group.track(this.pending);
  }

  /**
   * Send the canvas values that differ from the created child's state as one
   * Canvas state update, in the child session's order. Omitted values keep
   * the World's current value.
   */
  private updateCanvas(): void {
    const request = this.description.canvas;
    const state = this.canvasState;
    const child = this.child as
      | (Client & { sendCommand?(command: CanvasStateUpdateCommand): void })
      | undefined;
    if (!request || !state || !child || !this.usable()) return;
    const update = canvasStateUpdate(request, state);
    if (!update) return;
    try {
      if (!child.sendCommand)
        throw new Error("The child World client cannot send System commands");
      child.sendCommand(update.command);
      this.canvasState = update.state;
    } catch (error) {
      this.fail(error);
    }
  }

  private checkHandle(
    receipt: AttachmentReceipt,
    description: AttachedWorldDescription,
    generation: number,
  ): void {
    if (
      !this.usable() ||
      this.description.suspended ||
      this.generation !== generation ||
      this.current !== receipt ||
      this.description.signature !== description.signature ||
      this.parent.container.commits.resolveEntity(description.anchor) !==
        receipt.anchor
    )
      throw new Error("Attached World handle is no longer live");
  }

  /**
   * The presented output. SurfaceCanvas presents the child World's canvas,
   * which the attachment does not name; SurfaceCamera binds its camera.
   */
  private async output(
    description: AttachedWorldDescription,
  ): Promise<OutputReference | undefined> {
    if (description.attachment.mode === "spatial") return undefined;
    if (description.attachment.mode === "surface-canvas")
      return canvasOutput(this.world!);
    const selector = description.attachment.output;
    const kind = "camera";
    if ("world" in selector) {
      if (!sameWorld(selector.world, this.world!) || selector.kind !== kind)
        throw new Error(
          "Attached output must belong to the exact child World and mode",
        );
      this.releaseOutputTracking();
      return this.parent.group.host.resolveOutput(selector);
    }
    // Binding resolves the output's current incarnation. A reused binding
    // needs the same acknowledged output declaration in the child root and a
    // lifecycle watch on the output component showing the bound incarnation
    // is still current, so child commits that leave both alone need no Host
    // round trip. Another writer's replacement or removal of the component
    // changes the watched incarnation and schedules a rebind. When tracking is unavailable or has ended, every
    // reconcile binds again.
    const commits = this.scope!.container.commits;
    const entity = commits.resolveEntity(selector.entity);
    const witness = commits.outputWitness(selector.entity);
    const component = this.child!.components.Camera?.id;
    const bound = this.boundOutput;
    if (
      witness !== undefined &&
      bound?.witness === witness &&
      bound.output.kind === kind &&
      outputProducer(bound.output)?.entity === entity &&
      this.outputCurrent(bound.output)
    )
      return bound.output;
    this.boundOutput = undefined;
    const output = await this.parent.group.host.bindOutput(
      this.world!,
      entity,
      kind,
    );
    if (witness !== undefined && component !== undefined)
      await this.trackOutput(entity, component);
    else this.releaseOutputTracking();
    if (witness !== undefined && this.outputCurrent(output))
      this.boundOutput = { witness, output };
    return output;
  }

  /** Whether the output watch shows `output`'s incarnation is still current. */
  private outputCurrent(output: OutputReference): boolean {
    const tracking = this.outputTracking;
    const producer = outputProducer(output);
    if (
      !tracking ||
      tracking.failed ||
      !producer ||
      tracking.entity !== producer.entity
    )
      return false;
    const lifetime = tracking.lease.lifetime();
    return (
      !!lifetime &&
      lifetime.entityLive &&
      lifetime.incarnation === producer.incarnation
    );
  }

  /**
   * Watch the child World's output component through the shared lifecycle
   * tracking. A changed incarnation schedules a rebind; an ended watch is
   * reported and leaves this attachment binding on every reconcile.
   */
  private async trackOutput(entity: bigint, component: number): Promise<void> {
    const child = this.child!;
    const current = this.outputTracking;
    if (current?.entity === entity && current.component === component) {
      if (!current.failed) await current.lease.ready.catch(() => {});
      return;
    }
    this.releaseOutputTracking();
    if (
      this.outputTrackingEnded ||
      !child.watchLifecycle ||
      (child.manifest &&
        !child.manifest.systems.includes("ipp.lifecycle-publisher"))
    )
      return;
    const tracking: OutputTracking = {
      entity,
      component,
      failed: false,
      lease: undefined!,
    };
    const stale = () => {
      if (this.outputTracking !== tracking || !this.usable()) return;
      this.boundOutput = undefined;
      this.submittedSignature = undefined;
      this.parent.group.schedule();
    };
    tracking.lease = controlTracking(child).acquire(entity, component, {
      changed: () => {
        const bound = this.boundOutput;
        if (bound && !this.outputCurrent(bound.output)) stale();
      },
      failed: (error) => {
        if (this.outputTracking !== tracking || tracking.failed) return;
        tracking.failed = true;
        this.outputTrackingEnded = true;
        this.parent.group.report(
          new Error(
            "Attached output tracking ended; the output binds on every child commit",
            { cause: error },
          ),
        );
        // Only a reused binding can be stale; a watch that never started
        // left nothing reused, and the reconcile that asked binds anyway.
        if (this.boundOutput) stale();
      },
    });
    this.outputTracking = tracking;
    try {
      await tracking.lease.ready;
    } catch (error) {
      if (this.outputTracking === tracking && !tracking.failed) {
        tracking.failed = true;
        this.outputTrackingEnded = true;
        this.parent.group.report(
          new Error(
            "Attached output tracking failed; the output binds on every child commit",
            { cause: error },
          ),
        );
      }
    }
  }

  private releaseOutputTracking(): void {
    const tracking = this.outputTracking;
    if (!tracking) return;
    this.outputTracking = undefined;
    this.boundOutput = undefined;
    void tracking.lease
      .release()
      .catch((error: unknown) => this.parent.group.report(error));
  }

  private remember(outcome: BatchOutcome): void {
    for (const effect of outcome.effects) {
      // Adoption effects carry no attachment receipt.
      if (effect.kind === "adopted") continue;
      this.receipts.set(effect.receipt.id, effect.receipt);
      if (effect.kind === "written") {
        const previous = this.current;
        this.current = effect.receipt;
        this.currentIdentity = undefined;
        this.ready = undefined;
        this.clearRef();
        if (previous && previous.id !== effect.receipt.id) {
          this.detached.add(previous.id);
          this.parent.group.retiring(this.retire(previous));
        }
      } else {
        this.detached.add(effect.receipt.id);
        if (this.current?.id === effect.receipt.id) {
          this.current = undefined;
          this.currentIdentity = undefined;
        }
      }
    }
  }

  private async submit(
    operations: Command[],
    createsAttachment = true,
  ): Promise<BatchOutcome> {
    let outcome: BatchOutcome;
    try {
      outcome = await this.writer!.batch(operations);
    } catch (error) {
      // A batch applies whole at its final page: a refused or rejected batch
      // applied nothing, and any other failure leaves its outcome unknown.
      if (createsAttachment && !requestNotSubmitted(error))
        this.unknown = error;
      throw error;
    }
    this.remember(outcome);
    if (!outcome.ok) throw new ReactWorldBatchRejectedError(outcome);
    return outcome;
  }

  private async parentDestroyed(): Promise<boolean> {
    const parent = this.parent.client.worldReference!;
    return !(await this.parent.group.host.listWorlds()).some(
      (world) => world.id === parent.id,
    );
  }

  private async detach(receipt: AttachmentReceipt): Promise<void> {
    if (!this.receipts.has(receipt.id) || this.detached.has(receipt.id)) return;
    if (
      (this.parent.client.closure || this.writer?.closure) &&
      (await this.parentDestroyed())
    ) {
      this.detached.add(receipt.id);
      return;
    }
    try {
      await this.submit(
        [{ kind: "detachWorldAttachment", receipt: receipt.id }],
        false,
      );
      if (!this.detached.has(receipt.id))
        throw new Error(
          "Conditional detach acknowledgement omitted its effect",
        );
    } catch (error) {
      if (!(await this.parentDestroyed())) throw error;
      this.detached.add(receipt.id);
    }
  }

  private retire(receipt: AttachmentReceipt): Promise<void> {
    const pending = this.retirement.get(receipt.id);
    if (pending) return pending;
    if (!this.receipts.has(receipt.id)) return Promise.resolve();
    const work = this.waitForRetirement(receipt);
    this.retirement.set(receipt.id, work);
    void work.then(
      () => this.retirement.delete(receipt.id),
      () => this.retirement.delete(receipt.id),
    );
    return work;
  }

  private async waitForRetirement(receipt: AttachmentReceipt): Promise<void> {
    const epoch = this.retirementEpoch;
    const current = () => {
      if (epoch !== this.retirementEpoch)
        throw new Error("Retirement observation paused for cleanup recovery");
    };
    try {
      const destroyed =
        (this.parent.client.closure || this.writer?.closure) &&
        (await this.parentDestroyed());
      if (!destroyed) {
        for (;;) {
          if (this.abandoned) return;
          current();
          const state = await this.writer!.attachmentRetirement(receipt.id);
          if (this.abandoned) return;
          current();
          if (state === "retired") break;
          await new Promise<void>((resolve) => setTimeout(resolve, 16));
        }
        await this.writer!.releaseAttachmentReceipt(receipt.id);
      }
    } catch (error) {
      if (this.abandoned) return;
      current();
      if (!(await this.parentDestroyed())) throw error;
    }
    this.receipts.delete(receipt.id);
    this.detached.delete(receipt.id);
  }

  private clearRef(): void {
    this.refGeneration++;
    const release = this.releaseRef;
    this.releaseRef = undefined;
    this.assignedRef = undefined;
    try {
      release?.();
    } catch (error) {
      this.parent.group.report(error);
    }
  }

  private assignRef(value: AttachedWorldHandle): void {
    const ref = this.description.attachmentRef;
    const generation = ++this.refGeneration;
    this.assignedRef = ref;
    let cleanup: (() => void) | undefined;
    if (typeof ref === "function") {
      const release = ref(value);
      cleanup =
        typeof release === "function"
          ? release
          : () => {
              ref(null);
            };
    } else if (ref) {
      ref.current = value;
      cleanup = () => {
        ref.current = null;
      };
    }
    if (
      this.refGeneration === generation &&
      this.usable() &&
      this.ready === value &&
      this.description.attachmentRef === ref
    )
      this.releaseRef = cleanup;
    else {
      try {
        cleanup?.();
      } catch (error) {
        this.parent.group.report(error);
      }
    }
  }

  private notify(callback: () => void): void {
    if (!this.usable()) return;
    try {
      callback();
    } catch (error) {
      this.parent.group.report(error);
    }
  }

  private fail(error: unknown): void {
    const failure = errorValue(error);
    this.clearRef();
    if (!this.usable()) {
      this.parent.group.report(failure);
      return;
    }
    if (this.description.onError)
      this.notify(() => this.description.onError!(failure));
    else this.description.slot.update(this.scope?.container, failure);
  }

  cancel(): void {
    if (!this.active) return;
    this.active = false;
    const name = this.description.child.create?.symbolicId;
    if (this.createStarted && name !== undefined)
      this.parent.group.retireCreator(name, this);
    this.resolveCancelled();
    this.clearRef();
    this.releaseOutputTracking();
    this.scope?.fence();
    this.description.slot.update(undefined);
    this.cleanup = this.performCleanup();
    void this.cleanup.then(this.resolveClosed, this.rejectClosed);
  }

  /**
   * Unmount or session end: fence refs and callbacks and release this
   * boundary's sessions without World changes. Unlike removal, the
   * attachment, the child World (even one this boundary created) and the
   * child declarations stay.
   */
  release(): void {
    if (!this.active) return;
    this.active = false;
    this.released = true;
    this.resolveCancelled();
    this.clearRef();
    this.releaseOutputTracking();
    this.scope?.fence(true);
    this.description.slot.update(undefined);
    this.cleanup = this.performRelease();
    void this.cleanup.then(this.resolveClosed, this.rejectClosed);
  }

  retryCleanup(): Promise<void> {
    if (this.active || this.abandoned || this.abandoning)
      return Promise.reject(new Error("Cleanup recovery is not available"));
    if (this.recovering) return this.recovering;
    if (this.cleanupRunning) return this.cleanup!;
    const work = this.released ? this.performRelease() : this.performCleanup();
    this.recovering = work;
    void work
      .finally(() => {
        this.recovering = undefined;
      })
      .catch(() => {});
    return work;
  }

  private journal(): AttachedWorldCleanupJournal {
    return Object.freeze({
      parent: this.parent.client.worldReference!,
      receiptSession: this.writer?.session,
      child: this.world,
      childSession: this.child?.session,
      creatorOwned: this.created,
      receipts: Object.freeze([...this.receipts.values()]),
      unknownAttachmentOutcome: this.unknown,
      preparationFailure: this.preparationError,
    });
  }

  private cleanupError(errors: readonly unknown[]): AttachedWorldCleanupError {
    return new AttachedWorldCleanupError(errors, this.journal(), this.recovery);
  }

  /**
   * Settle submitted work, then release the child declarations'
   * subscriptions and close the child session.
   */
  private async performRelease(): Promise<void> {
    this.cleanupRunning = true;
    try {
      await this.preparing;
      await this.pending;
      if (!this.completed) {
        const errors: unknown[] = [];
        if (this.scope) {
          const scope = this.scope;
          for (const work of [
            () => scope.dispose(false, true),
            () => scope.container.commits.dispose(),
          ]) {
            try {
              await work();
            } catch (error) {
              errors.push(error);
            }
          }
          if (!errors.length) this.parent.group.remove(scope);
        }
        if (this.child && !this.child.closure) {
          try {
            await this.child.close();
          } catch (error) {
            errors.push(error);
          }
        }
        if (errors.length) throw this.cleanupError(errors);
        this.completed = true;
        this.resolveDeclarations();
      }
      await this.parent.release(this);
    } catch (error) {
      const failure =
        error instanceof AttachedWorldCleanupError
          ? error
          : this.cleanupError([error]);
      this.rejectDeclarations(failure);
      throw failure;
    } finally {
      this.cleanupRunning = false;
    }
  }

  private async performCleanup(): Promise<void> {
    this.cleanupRunning = true;
    try {
      await this.preparing;
      await this.pending;
      if (this.retirementEpoch > 0)
        await Promise.allSettled([...this.retirement.values()]);
      if (this.completed) {
        await this.parent.release(this);
        return;
      }
      const errors: unknown[] = [];
      for (const receipt of [...this.receipts.values()]) {
        try {
          await this.detach(receipt);
        } catch (error) {
          errors.push(error);
        }
      }
      if (errors.length) throw this.cleanupError(errors);
      const nestedErrors: unknown[] = [];
      if (this.scope) {
        try {
          await this.scope.dispose(true, false);
        } catch (error) {
          nestedErrors.push(error);
        }
        this.parent.group.remove(this.scope);
      }
      if (!this.declarationsCleaned) {
        const declarationErrors: unknown[] = [];
        if (this.scope) {
          try {
            await this.scope.container.commits.dispose(true);
          } catch (error) {
            declarationErrors.push(error);
          }
        }
        if (this.child) {
          try {
            await this.child.close();
          } catch (error) {
            declarationErrors.push(error);
          }
        }
        if (declarationErrors.length) {
          try {
            if (
              this.world &&
              !(await this.parent.group.host.listWorlds()).some(
                (world) => world.id === this.world!.id,
              )
            )
              declarationErrors.length = 0;
          } catch {}
        }
        if (!declarationErrors.length) this.declarationsCleaned = true;
        errors.push(...declarationErrors);
      }
      errors.push(...nestedErrors);
      if (this.declarationsCleaned && !nestedErrors.length)
        this.resolveDeclarations();
      else this.rejectDeclarations(this.cleanupError(errors));
      if (this.unknown) {
        try {
          if (await this.parentDestroyed()) this.unknown = undefined;
          else errors.push(this.unknown);
        } catch {
          errors.push(this.unknown);
        }
      }
      if (this.preparationError) errors.push(this.preparationError);
      const retirements = await Promise.allSettled(
        errors.length
          ? []
          : [...this.receipts.values()]
              .filter((receipt) => this.detached.has(receipt.id))
              .map((receipt) => this.retire(receipt)),
      );
      errors.push(
        ...retirements.flatMap((result) =>
          result.status === "rejected" ? [result.reason] : [],
        ),
      );
      if (!errors.length && this.receipts.size)
        errors.push(
          new Error("Attachment cleanup still holds undetached receipts"),
        );
      if (!errors.length && this.created && this.world) {
        try {
          await this.parent.group.host.destroyWorld(this.world);
        } catch (error) {
          try {
            if (
              (await this.parent.group.host.listWorlds()).some(
                (world) => world.id === this.world!.id,
              )
            )
              errors.push(error);
          } catch {
            errors.push(error);
          }
        }
      }
      if (errors.length) throw this.cleanupError(errors);
      this.completed = true;
      this.parent.group.releaseCreator(this);
      await this.parent.release(this);
    } catch (error) {
      this.retirementEpoch++;
      const failure =
        error instanceof AttachedWorldCleanupError
          ? error
          : this.cleanupError([error]);
      if (!this.declarationsCleaned) this.rejectDeclarations(failure);
      throw failure;
    } finally {
      this.cleanupRunning = false;
    }
  }

  private abandonCleanup(): Promise<AttachedWorldCleanupJournal> {
    if (this.abandoning) return this.abandoning;
    if (this.active || this.cleanupRunning || this.recovering)
      return Promise.reject(new Error("Cleanup recovery is in progress"));
    const journal = this.journal();
    this.abandoned = true;
    const work = (async () => {
      const errors: unknown[] = [];
      try {
        await this.scope?.abandon();
      } catch (error) {
        errors.push(error);
      }
      if (this.child && !this.child.closure) {
        try {
          await this.child.close();
        } catch (error) {
          errors.push(error);
        }
      }
      for (const receipt of [...this.receipts.values()]) {
        try {
          if (!this.writer?.closure)
            await this.writer!.releaseAttachmentReceipt(receipt.id);
          this.receipts.delete(receipt.id);
          this.detached.delete(receipt.id);
        } catch (error) {
          errors.push(error);
        }
      }
      if (errors.length) throw this.cleanupError(errors);
      try {
        await this.parent.release(this);
        this.parent.group.releaseCreator(this);
      } catch (error) {
        throw this.cleanupError([error]);
      }
      return journal;
    })();
    this.abandoning = work;
    void work
      .finally(() => {
        this.abandoning = undefined;
      })
      .catch(() => {});
    return work;
  }
}

export class ReactAttachmentGroup {
  private readonly scopes = new Set<AttachmentScope>();
  private readonly work = new Set<Promise<void>>();
  private readonly retiringCreators = new Map<string, Set<AttachmentRecord>>();
  private readonly root: AttachmentScope;
  private scheduled = false;
  revision = 0;

  constructor(
    readonly host: ReactCompositionHost,
    container: ReactWorldContainer,
    client: ReactWorldClient,
    readonly options: ReactWorldRootOptions,
  ) {
    this.root = this.add(container, client);
    container.capturePortals = () => {
      for (const scope of this.scopes)
        if (scope !== this.root && !scope.closing) scope.container.capture();
    };
  }

  add(
    container: ReactWorldContainer,
    client: ReactWorldClient,
  ): AttachmentScope {
    const scope = new AttachmentScope(this, container, client);
    this.scopes.add(scope);
    this.revision++;
    return scope;
  }

  remove(scope: AttachmentScope): void {
    this.scopes.delete(scope);
  }

  retireCreator(name: string, record: AttachmentRecord): void {
    let records = this.retiringCreators.get(name);
    if (!records) {
      records = new Set();
      this.retiringCreators.set(name, records);
    }
    records.add(record);
  }

  creatorRetirements(name: string): readonly Promise<void>[] {
    return [...(this.retiringCreators.get(name) ?? [])].map(
      (record) => record.closed,
    );
  }

  releaseCreator(record: AttachmentRecord): void {
    const name = record.description.child.create?.symbolicId;
    if (name === undefined) return;
    const records = this.retiringCreators.get(name);
    records?.delete(record);
    if (!records?.size) this.retiringCreators.delete(name);
  }

  uncaught(error: unknown): void {
    for (const scope of this.scopes) {
      for (const record of scope.active.values()) record.fenceCallbacks();
      if (scope !== this.root && !scope.closing)
        scope.container.uncaught(error);
    }
  }

  report(error: unknown): void {
    this.root.container.commits.report(error);
  }

  track(work: Promise<void>): void {
    this.work.add(work);
    this.revision++;
    void work
      .finally(() => {
        this.work.delete(work);
        this.revision++;
      })
      .catch((error) => this.report(error));
  }

  retiring(work: Promise<void>): void {
    void work.catch((error) => this.report(error));
  }

  schedule(): void {
    if (this.scheduled) return;
    this.scheduled = true;
    queueMicrotask(() => {
      this.scheduled = false;
      this.publish();
    });
  }

  publish(): void {
    for (const scope of this.scopes)
      if (scope !== this.root && !scope.closing) scope.container.publish();
    for (const scope of this.scopes)
      for (const record of scope.active.values()) record.reconcile();
  }

  async settled(): Promise<void> {
    await Promise.all([
      ...this.work,
      ...[...this.scopes].map((scope) => scope.checkpoint()),
    ]);
  }

  /** Unmount: release every boundary without World changes. */
  fence(): void {
    this.root.fence(true);
  }

  async dispose(retry = false): Promise<void> {
    await this.root.dispose(retry, true);
  }

  async abandon(): Promise<void> {
    await this.root.abandon();
  }
}
