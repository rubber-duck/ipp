import type {
  GuiFileAdapter,
  GuiFileDescriptor,
} from "../../gui-kit/file-input.js";

export interface BrowserGuiFile extends GuiFileDescriptor {
  readonly file: File;
}
export interface BrowserGuiFileOptions {
  /** Explicit DOM drop/paste zone. The embedding application owns its placement. */
  readonly surface?: HTMLElement;
  /** A real native file input for platforms requiring a synchronous trusted activation. */
  readonly picker?: HTMLInputElement;
}

/** Chooser handles, arbitrary File bytes and any derived object URLs remain application-owned. */
export function browserGuiFileAdapter({
  surface,
  picker,
}: BrowserGuiFileOptions = {}): GuiFileAdapter<BrowserGuiFile> {
  if (picker && picker.type !== "file")
    throw new TypeError("Expected an input of type file");
  const descriptors = (files: FileList | readonly File[] | null) =>
    Array.from(files ?? [], (file) => ({
      id: crypto.randomUUID(),
      name: file.name,
      size: file.size,
      type: file.type,
      file,
    }));
  return {
    capabilities: {
      choose: !!picker || typeof document !== "undefined",
      drop: !!surface,
      paste: !!surface,
    },
    subscribe(request, receive) {
      const signal = request.signal;
      const accept = (list: FileList | null) => {
        if (!signal.aborted && list?.length) receive(descriptors(list));
      };
      surface?.addEventListener(
        "dragover",
        (event) => {
          if (event.dataTransfer?.types.includes("Files"))
            event.preventDefault();
        },
        { signal },
      );
      surface?.addEventListener(
        "drop",
        (event) => {
          if (!event.dataTransfer?.files.length) return;
          event.preventDefault();
          accept(event.dataTransfer.files);
        },
        { signal },
      );
      surface?.addEventListener(
        "paste",
        (event) => {
          if (!event.clipboardData?.files.length) return;
          event.preventDefault();
          accept(event.clipboardData.files);
        },
        { signal },
      );
      if (!picker) return;
      const oldAccept = picker.accept,
        oldMultiple = picker.multiple;
      picker.accept = request.accept.join(",");
      picker.multiple = request.multiple;
      let armed = false;
      picker.addEventListener(
        "click",
        () => {
          armed = true;
        },
        { signal },
      );
      picker.addEventListener(
        "cancel",
        () => {
          armed = false;
        },
        { signal },
      );
      picker.addEventListener(
        "change",
        () => {
          if (armed) accept(picker.files);
          armed = false;
          picker.value = "";
        },
        { signal },
      );
      return () => {
        picker.accept = oldAccept;
        picker.multiple = oldMultiple;
        picker.value = "";
      };
    },
    async choose(request) {
      if (request.signal.aborted) return null;
      const input = picker ?? document.createElement("input");
      if (!picker) {
        input.type = "file";
        input.accept = request.accept.join(",");
        input.multiple = request.multiple;
      }
      // Browser policies can require a direct trusted gesture. A supplied picker
      // remains directly operable even when a remote Canvas press lacks activation.
      if (navigator.userActivation && !navigator.userActivation.isActive)
        throw new Error(
          "Activate the browser file picker directly to choose files",
        );
      if (picker) {
        picker.click();
        return null;
      }
      return await new Promise((resolve, reject) => {
        const finish = (files: readonly BrowserGuiFile[] | null) => {
          request.signal.removeEventListener("abort", abort);
          input.onchange = null;
          input.oncancel = null;
          input.remove();
          resolve(files);
        };
        const abort = () => finish(null);
        input.onchange = () => finish(descriptors(input.files));
        input.oncancel = () => finish(null);
        request.signal.addEventListener("abort", abort, { once: true });
        input.hidden = true;
        document.body.append(input);
        try {
          if (typeof input.showPicker === "function") input.showPicker();
          else input.click();
        } catch (error) {
          request.signal.removeEventListener("abort", abort);
          input.remove();
          reject(error);
        }
      });
    },
  };
}
