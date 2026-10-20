import type { GuiClientInput, GuiTarget } from "./gui-types.js";

interface GeometryCanvas extends HTMLCanvasElement {
  updateElementGeometry(
    element: Element,
    options: { canvasTransform: DOMMatrix },
  ): void;
  clearElementGeometry(element: Element): void;
}

const INPUTS = Symbol.for("ipp.html-inputs");
type InputCanvas = HTMLCanvasElement & { [INPUTS]?: Set<HtmlInputBinding> };

/** Find a DOM editor only for the exact target authorized by the input context. */
export function htmlInputFor(
  canvas: HTMLCanvasElement,
  target?: GuiTarget,
): HtmlInputBinding | undefined {
  if (!target) return;
  return [...((canvas as InputCanvas)[INPUTS] ?? [])].find(
    ({ target: candidate }) =>
      candidate.entity === target.entity &&
      candidate.component === target.component &&
      candidate.incarnation === target.incarnation &&
      candidate.world.id === target.world.id &&
      candidate.world.incarnation === target.world.incarnation,
  );
}

const FOCUSABLE =
  "a[href],button,input,textarea,select,[tabindex],[contenteditable]";
const editable = (
  element: Element,
): element is HTMLInputElement | HTMLTextAreaElement =>
  element instanceof HTMLTextAreaElement ||
  (element instanceof HTMLInputElement &&
    ["text", "search", "url", "tel", "email", "password"].includes(
      element.type,
    ));

/** Browser-local input for one exact GUI target. Core owns routing and focus;
 * the live DOM owns native editing, clipboard, IME and form values. */
export class HtmlInputBinding {
  private readonly cancel = new AbortController();
  private readonly root: ShadowRoot | HTMLElement;
  private selected: HTMLElement;
  private active = false;
  private stopped = false;
  private escape: (() => void) | undefined;
  private hover: Element | undefined;
  private readonly pressed = new Map<
    bigint,
    { element: Element; anchor?: number }
  >();

  constructor(
    private readonly canvas: HTMLCanvasElement,
    readonly content: HTMLElement,
    readonly target: GuiTarget,
  ) {
    this.root = content.shadowRoot ?? content;
    this.selected = content;
    content.tabIndex = -1;
    content.style.pointerEvents = "none";
    const inputs = ((canvas as InputCanvas)[INPUTS] ??= new Set());
    inputs.add(this);
    const listen = (name: string, callback: (event: Event) => void) =>
      content.addEventListener(name, callback, { signal: this.cancel.signal });
    // Redispatched DOM events must never enter the canvas router a second time.
    for (const name of [
      "pointerdown",
      "pointermove",
      "pointerup",
      "pointercancel",
      "mousedown",
      "mouseup",
      "wheel",
      "contextmenu",
      "keyup",
    ])
      listen(name, (event) => event.stopPropagation());
    listen("keydown", (event) => {
      event.stopPropagation();
      const key = event as KeyboardEvent;
      if (!this.active || this.stopped) {
        event.preventDefault();
        return;
      }
      if (key.key === "Escape" && key.ctrlKey && key.shiftKey) {
        event.preventDefault();
        this.escape?.();
      } else if (key.key === "Tab") {
        event.preventDefault();
        const items = [
          ...this.root.querySelectorAll<HTMLElement>(FOCUSABLE),
        ].filter(
          (element) =>
            element.tabIndex >= 0 &&
            !element.matches(":disabled,[inert]") &&
            element.getClientRects().length > 0,
        );
        const index = items.indexOf(this.selected);
        const next =
          items[
            (index + (key.shiftKey ? -1 : 1) + items.length) % items.length
          ];
        if (next) this.focusElement(next);
      }
    });
    listen("beforeinput", (event) => {
      if (!this.active || this.stopped) event.preventDefault();
    });
    listen("focusin", (event) => {
      const element = event.composedPath()[0];
      if (element instanceof HTMLElement) this.selected = element;
    });
  }

  contains(target: EventTarget | null): boolean {
    return (
      target instanceof Node &&
      (this.content.contains(target) || this.root.contains(target))
    );
  }

  activate(escape: () => void): void {
    if (this.stopped) return;
    this.escape = escape;
    this.active = true;
    this.selected.focus({ preventScroll: true });
  }

  deactivate(): void {
    this.active = false;
    this.escape = undefined;
    for (const [pointer, { element }] of this.pressed)
      element.dispatchEvent(
        new PointerEvent("pointercancel", {
          bubbles: true,
          composed: true,
          pointerId: Number(pointer),
        }),
      );
    this.pressed.clear();
    this.selected.blur();
  }

  private focusElement(element: HTMLElement): void {
    this.selected = element;
    if (this.active) element.focus({ preventScroll: true });
  }

  /** Use Chromium's actual DOM hit testing, including clips, transforms and
   * shadow trees. Geometry exists only during this synchronous query; it never
   * bypasses IPP's curved Surface routing or installs a flat DOM overlay. */
  private at<T>(
    point: readonly [number, number],
    action: (element: Element, x: number, y: number) => T,
  ): T | undefined {
    const canvas = this.canvas as GeometryCanvas;
    const rect = canvas.getBoundingClientRect();
    const zoom =
      this.content.getBoundingClientRect().width / this.content.offsetWidth;
    // Place the queried content point at the canvas centre, within the viewport.
    const x = rect.left + rect.width / 2,
      y = rect.top + rect.height / 2;
    this.content.style.pointerEvents = "auto";
    canvas.updateElementGeometry(this.content, {
      canvasTransform: new DOMMatrix([
        1,
        0,
        0,
        1,
        rect.width / (2 * zoom) - point[0],
        rect.height / (2 * zoom) - point[1],
      ]),
    });
    try {
      let element =
        this.content.shadowRoot?.elementFromPoint(x, y) ??
        document.elementFromPoint(x, y);
      while (element?.shadowRoot) {
        const child = element.shadowRoot.elementFromPoint(x, y);
        if (!child || child === element) break;
        element = child;
      }
      return action(
        element && this.contains(element) ? element : this.content,
        x,
        y,
      );
    } finally {
      canvas.clearElementGeometry(this.content);
      this.content.style.pointerEvents = "none";
    }
  }

  private caret(
    element: HTMLInputElement | HTMLTextAreaElement,
    x: number,
    y: number,
  ): number {
    // Chromium returns the control's UTF-16 caret offset, including wrapping,
    // multiline layout, bidi and scrolling; do not approximate text geometry.
    const position = document.caretPositionFromPoint(
      x,
      y,
      this.root instanceof ShadowRoot ? { shadowRoots: [this.root] } : {},
    );
    return position?.offsetNode === element
      ? position.offset
      : element.value.length;
  }

  input(input: GuiClientInput): void {
    if (this.stopped) return;
    if (input.kind === "pointer") {
      const held = this.pressed.get(input.pointer);
      if (input.phase === "cancel") {
        held?.element.dispatchEvent(
          new PointerEvent("pointercancel", {
            bubbles: true,
            composed: true,
            pointerId: Number(input.pointer),
          }),
        );
        this.pressed.delete(input.pointer);
        return;
      }
      this.at(input.point, (hit, x, y) => {
        const element = held?.element ?? hit;
        const init = {
          bubbles: true,
          composed: true,
          cancelable: true,
          clientX: x,
          clientY: y,
          pointerId: Number(input.pointer),
          pointerType: "mouse",
          button:
            input.button === "secondary"
              ? 2
              : input.button === "auxiliary"
                ? 1
                : 0,
          buttons: input.phase === "down" || held ? 1 : 0,
        };
        if (this.hover !== hit) {
          this.hover?.dispatchEvent(new PointerEvent("pointerout", init));
          hit.dispatchEvent(new PointerEvent("pointerover", init));
          this.hover = hit;
        }
        const allowed = element.dispatchEvent(
          new PointerEvent(`pointer${input.phase}`, init),
        );
        const mouseAllowed =
          allowed &&
          element.dispatchEvent(new MouseEvent(`mouse${input.phase}`, init));
        if (input.phase === "down") {
          this.pressed.set(input.pointer, { element });
          if (!mouseAllowed) return;
          const focus = element.closest<HTMLElement>(FOCUSABLE);
          if (focus && !focus.matches(":disabled")) this.focusElement(focus);
          if (editable(element) && element.selectionStart !== null) {
            const caret = this.caret(element, x, y);
            element.setSelectionRange(caret, caret);
            this.pressed.set(input.pointer, { element, anchor: caret });
          }
        } else if (
          input.phase === "move" &&
          held?.anchor !== undefined &&
          editable(element)
        ) {
          const caret = this.caret(element, x, y);
          element.setSelectionRange(
            Math.min(held.anchor, caret),
            Math.max(held.anchor, caret),
            caret < held.anchor ? "backward" : "forward",
          );
        } else if (input.phase === "up") {
          this.pressed.delete(input.pointer);
          if (held && held.element === hit && mouseAllowed) {
            if (input.button === "secondary")
              hit.dispatchEvent(new MouseEvent("contextmenu", init));
            else if (input.button === "primary" && hit instanceof HTMLElement)
              hit.click();
            if (hit instanceof HTMLOptionElement && !hit.disabled) {
              const select = hit.closest("select");
              if (select && !select.disabled) {
                select.value = hit.value;
                select.dispatchEvent(
                  new Event("input", { bubbles: true, composed: true }),
                );
                select.dispatchEvent(new Event("change", { bubbles: true }));
                this.focusElement(select);
              }
            }
          }
        }
        if (
          mouseAllowed &&
          (input.phase === "down" || (input.phase === "move" && held)) &&
          element instanceof HTMLInputElement &&
          element.type === "range" &&
          !element.disabled
        ) {
          const rect = element.getBoundingClientRect();
          const min = Number(element.min || 0),
            max = Number(element.max || 100),
            step = Number(element.step || 1);
          const value =
            min +
            Math.max(0, Math.min(1, (x - rect.left) / rect.width)) *
              (max - min);
          element.value = String(
            step > 0 ? min + Math.round((value - min) / step) * step : value,
          );
          element.dispatchEvent(
            new Event("input", { bubbles: true, composed: true }),
          );
        }
        if (
          input.phase === "up" &&
          element instanceof HTMLInputElement &&
          element.type === "range"
        )
          element.dispatchEvent(new Event("change", { bubbles: true }));
      });
      if (input.phase === "up") this.pressed.delete(input.pointer);
    } else if (input.kind === "wheel") {
      this.at(input.point, (element, x, y) => {
        const delta = input.delta.map((value) => value * 400);
        if (
          !element.dispatchEvent(
            new WheelEvent("wheel", {
              bubbles: true,
              composed: true,
              cancelable: true,
              clientX: x,
              clientY: y,
              deltaX: delta[0]!,
              deltaY: delta[1]!,
            }),
          )
        )
          return;
        for (
          let node: Element | null = element;
          node;
          node =
            node.parentElement ??
            (node.getRootNode() === this.root ? this.content : null)
        ) {
          const before = [node.scrollLeft, node.scrollTop];
          const style = getComputedStyle(node);
          if (/auto|scroll/.test(style.overflowX)) node.scrollLeft += delta[0]!;
          if (/auto|scroll/.test(style.overflowY)) node.scrollTop += delta[1]!;
          delta[0]! -= node.scrollLeft - before[0]!;
          delta[1]! -= node.scrollTop - before[1]!;
          if (node === this.content || delta.every((value) => value === 0))
            break;
        }
      });
    } else if (this.active && input.kind === "text") {
      // Committed text from non-DOM ingress uses the same browser editing engine.
      document.execCommand("insertText", false, input.text);
    } else if (this.active && input.kind === "key") {
      this.selected.dispatchEvent(
        new KeyboardEvent(input.pressed ? "keydown" : "keyup", {
          bubbles: true,
          composed: true,
          cancelable: true,
          key: input.key,
          code: input.code,
          repeat: input.repeat,
          shiftKey: !!(input.modifiers & 1),
          ctrlKey: !!(input.modifiers & 2),
          altKey: !!(input.modifiers & 4),
          metaKey: !!(input.modifiers & 8),
        }),
      );
    }
  }

  release(): void {
    if (this.stopped) return;
    this.stopped = true;
    this.deactivate();
    this.cancel.abort();
    (this.canvas as InputCanvas)[INPUTS]?.delete(this);
  }
}
