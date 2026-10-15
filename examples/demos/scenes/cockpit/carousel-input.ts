/** Native capture qualifies grabs; fixed-camera rays keep a moving pane from chasing the pointer. */
import type {
  Client,
  GuiPhysicalInput,
  GuiInputRoutingOutcome,
  PickingWorldClient,
  ViewQueryTarget,
} from "@ipp/client";
import type { GallerySceneInput } from "../../shared/scene.js";
import { cameraForward, cameraRayAngle, type Pose } from "./placement.js";

const WHEEL_LOGICAL_UNITS_PER_PANE = 160;

interface Grab {
  readonly pointer: bigint;
  readonly start: readonly [number, number];
  point: readonly [number, number];
  readonly focus: number;
  admitted: boolean;
  released: boolean;
  angle: number;
  basis: readonly (readonly number[])[];
}

export class CockpitCarouselInput implements GallerySceneInput {
  private grab: Grab | undefined;
  private moving: Promise<void> | undefined;
  private wheelActive = false;
  private wheelIntent: number | undefined;
  private wheelSequence = 0;
  private gestureSequence = 0;

  constructor(
    private readonly controls: {
      signal: AbortSignal;
      camera: Pose;
      client: PickingWorldClient;
      view(): ViewQueryTarget;
      panes(): readonly { client: Client; grip: bigint }[];
      ownsWheelHit?(outcome: GuiInputRoutingOutcome): boolean;
      wheel(focus: number, current: () => boolean): Promise<void>;
      focus(): number;
      step(): number;
      update(
        focus: number,
        dragging: boolean,
        current?: () => boolean,
      ): Promise<void>;
    },
  ) {}

  get capturing() {
    return !!this.grab;
  }

  cancelWheel() {
    this.wheelActive = false;
    ++this.wheelSequence;
    this.wheelIntent = undefined;
  }

  cancel() {
    ++this.gestureSequence;
    this.grab = undefined;
    this.cancelWheel();
  }

  private async project(point: readonly [number, number]) {
    const { camera, client } = this.controls;
    const normal = cameraForward(camera);
    const result = await client.query({
      type: "CameraProjectQuery",
      view: this.controls.view(),
      x: point[0],
      y: point[1],
      plane: {
        point: [
          camera.x + normal[0],
          camera.y + normal[1],
          camera.z + normal[2],
        ],
        normal: [...normal],
      },
    });
    if (!result.ok)
      throw new Error(`Carousel projection failed: ${result.error}`);
    if (!result.position)
      throw new Error("Carousel ray does not intersect the camera plane");
    return result.position;
  }

  private angle(point: readonly [number, number], grab: Grab) {
    const [origin, horizontal, vertical] = grab.basis;
    const position = origin!.map(
      (value, index) =>
        value +
        (horizontal![index]! - value) * point[0] +
        (vertical![index]! - value) * point[1],
    );
    return cameraRayAngle(position, this.controls.camera);
  }

  private move() {
    if (this.moving) return this.moving;
    this.moving = (async () => {
      let previous: readonly [number, number] | undefined;
      for (;;) {
        const grab = this.grab;
        if (
          !grab?.admitted ||
          this.controls.signal.aborted ||
          grab.point === previous
        )
          return;
        const point = grab.point;
        previous = point;
        const angle = this.angle(point, grab);
        if (this.grab !== grab || this.controls.signal.aborted) return;
        await this.controls.update(
          grab.focus - (angle - grab.angle) / this.controls.step(),
          true,
          () => this.grab === grab && !this.controls.signal.aborted,
        );
      }
    })().finally(() => {
      this.moving = undefined;
    });
    return this.moving;
  }

  async handle(input: GuiPhysicalInput, outcome: GuiInputRoutingOutcome) {
    if (this.controls.signal.aborted) return;
    if (input.kind === "pointerCancel" && this.grab?.pointer !== input.pointer)
      return;
    if (
      outcome.error ||
      outcome.rejected ||
      outcome.cancelled ||
      input.kind === "blur" ||
      input.kind === "pointerCancel"
    ) {
      const active = this.grab;
      const wheelActive = this.wheelActive;
      this.cancel();
      if (active?.admitted || wheelActive)
        await this.controls.update(this.controls.focus(), false);
      return;
    }
    if (input.kind === "wheel") {
      // Native controls and scroll chains get the wheel first. Only an exact
      // application-owned chrome hit may use the movement left over afterward.
      if (this.grab || !this.controls.ownsWheelHit?.(outcome)) return;
      const remaining =
        outcome.remaining ??
        (outcome.disposition === "unhandled" ? input.delta : undefined);
      if (!remaining?.every(Number.isFinite)) return;
      const [horizontal, vertical] = remaining;
      const delta =
        Math.abs(horizontal) >= Math.abs(vertical) ? horizontal : vertical;
      if (!delta) return;
      // Accumulate before awaiting any rendering or prior held-command drain.
      // The latest wheel target owns admission; superseded work cannot republish.
      const focus = Math.max(
        0,
        Math.min(
          2,
          (this.wheelIntent ?? this.controls.focus()) +
            delta / WHEEL_LOGICAL_UNITS_PER_PANE,
        ),
      );
      this.wheelActive = true;
      this.wheelIntent = focus;
      const sequence = ++this.wheelSequence;
      try {
        await this.controls.wheel(
          focus,
          () =>
            sequence === this.wheelSequence &&
            !this.grab &&
            !this.controls.signal.aborted,
        );
      } finally {
        if (sequence === this.wheelSequence) this.wheelIntent = undefined;
      }
      return;
    }
    if (input.kind === "pointerDown") {
      const focus = this.wheelIntent ?? this.controls.focus();
      const wheelActive = this.wheelActive;
      const gesture = ++this.gestureSequence;
      this.cancelWheel();
      if (wheelActive) {
        await this.controls.update(
          focus,
          false,
          () =>
            gesture === this.gestureSequence && !this.controls.signal.aborted,
        );
        if (gesture !== this.gestureSequence || this.controls.signal.aborted)
          return;
      }
      if (
        this.grab ||
        input.button === "secondary" ||
        input.button === "auxiliary" ||
        outcome.disposition !== "routed"
      )
        return;
      const grab: Grab = {
        pointer: input.pointer,
        start: input.point,
        point: input.point,
        focus,
        admitted: false,
        released: false,
        angle: 0,
        basis: [],
      };
      this.grab = grab;
      const hits = await Promise.all(
        this.controls.panes().map(async ({ client, grip }) => {
          const state = await client.inspectPage({ collection: "guiPointers" });
          return state.guiPointers?.some(
            (record) =>
              record.pointer === input.pointer &&
              record.target.entity === grip &&
              record.state.captured,
          );
        }),
      );
      if (this.grab !== grab || this.controls.signal.aborted) return;
      if (!hits.some(Boolean)) {
        this.cancel();
        return;
      }
      // Camera/view are fixed for this gesture. Native projection supplies an
      // affine plane basis once; later moves need no query or transport delay.
      grab.basis = await Promise.all([
        this.project([0, 0]),
        this.project([1, 0]),
        this.project([0, 1]),
      ]);
      if (this.grab !== grab || this.controls.signal.aborted) return;
      grab.angle = this.angle(grab.start, grab);
      grab.admitted = true;
      await this.controls.update(
        grab.focus,
        true,
        () => this.grab === grab && !this.controls.signal.aborted,
      );
      if (this.grab !== grab || this.controls.signal.aborted) return;
      await this.move();
      if (grab.released && this.grab === grab) {
        this.cancel();
        await this.controls.update(Math.round(this.controls.focus()), false);
      }
    } else if (
      (input.kind === "pointerMove" || input.kind === "pointerUp") &&
      this.grab?.pointer === input.pointer
    ) {
      const grab = this.grab;
      grab.point = input.point;
      if (input.kind === "pointerUp") grab.released = true;
      await this.move();
      if (grab.admitted && grab.released && this.grab === grab) {
        this.cancel();
        await this.controls.update(Math.round(this.controls.focus()), false);
      }
    }
  }
}
