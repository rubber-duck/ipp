/** Scene gestures consume correlated runtime GUI routing, independently of the platform. */
import type {
  GuiPhysicalInput,
  GuiInputRoutingOutcome,
  PresentationViewport,
} from "@ipp/client";
import type { GallerySceneMount } from "./scene.js";

interface Gesture {
  readonly pointer: bigint;
  readonly button: "primary" | "auxiliary";
  readonly origin: readonly [number, number];
  point: readonly [number, number];
  dragged: boolean;
}

export class ChartSceneInput {
  private gesture: Gesture | undefined;

  constructor(
    private readonly scene: GallerySceneMount,
    private readonly signal: AbortSignal,
    private readonly viewport: () => PresentationViewport,
  ) {}

  get capturing(): boolean {
    return !!this.gesture;
  }
  cancel() {
    this.gesture = undefined;
  }

  private async cancelNavigation() {
    this.cancel();
    await this.scene.action("endNavigation");
    if (!this.signal.aborted) await this.scene.action("hover", null);
  }

  private async move(point: readonly [number, number]) {
    const gesture = this.gesture;
    if (!gesture || this.signal.aborted) return;
    const viewport = this.viewport();
    const width = viewport.width / viewport.devicePixelRatio,
      height = viewport.height / viewport.devicePixelRatio;
    const dx = point[0] - gesture.point[0],
      dy = point[1] - gesture.point[1];
    gesture.point = point;
    gesture.dragged ||=
      Math.hypot(
        (point[0] - gesture.origin[0]) * width,
        (point[1] - gesture.origin[1]) * height,
      ) >= 5;
    if (!gesture.dragged || (dx === 0 && dy === 0)) return;
    await this.scene.action(
      "navigate",
      gesture.button === "auxiliary"
        ? { kind: "pan", x: dx, y: dy }
        : {
            kind: "rotate",
            yaw: ((-dx * width) / height) * Math.PI,
            pitch: -dy * Math.PI,
          },
    );
  }

  async handle(
    input: GuiPhysicalInput,
    outcome: GuiInputRoutingOutcome,
  ): Promise<void> {
    if (this.signal.aborted) return;
    if (outcome.error || outcome.rejected || outcome.cancelled) {
      await this.cancelNavigation();
      return;
    }
    const admitted =
      outcome.disposition === "miss" || outcome.disposition === "unhandled";
    if (input.kind === "pointerDown") {
      if (
        outcome.disposition !== "miss" ||
        this.gesture ||
        input.button === "secondary"
      )
        return;
      this.gesture = {
        pointer: input.pointer,
        button: input.button === "auxiliary" ? "auxiliary" : "primary",
        origin: input.point,
        point: input.point,
        dragged: false,
      };
      await this.scene.action("beginNavigation");
    } else if (input.kind === "pointerMove") {
      if (this.gesture?.pointer === input.pointer) await this.move(input.point);
      else
        await this.scene.action(
          "hover",
          admitted ? { x: input.point[0], y: input.point[1] } : null,
        );
    } else if (
      input.kind === "pointerUp" &&
      this.gesture?.pointer === input.pointer
    ) {
      const gesture = this.gesture;
      await this.move(input.point);
      this.gesture = undefined;
      if (this.signal.aborted) return;
      await this.scene.action("endNavigation");
      if (gesture.button === "primary" && !gesture.dragged && admitted)
        await this.scene.action("select", {
          x: input.point[0],
          y: input.point[1],
        });
      await this.scene.action(
        "hover",
        admitted ? { x: input.point[0], y: input.point[1] } : null,
      );
    } else if (input.kind === "pointerCancel" || input.kind === "blur") {
      await this.cancelNavigation();
    } else if (
      input.kind === "wheel" &&
      (admitted ||
        outcome.remaining?.every((value, axis) => value === input.delta[axis]))
    ) {
      await this.scene.action("navigate", {
        kind: "zoom",
        amount: Math.max(-1, Math.min(1, input.delta[1] * 0.005)),
      });
    }
  }
}
