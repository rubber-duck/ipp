import { useEffect, useRef, useState, type ReactNode } from "react";
import type { CameraView } from "./shared/camera.js";
import {
  GALLERY_SCENES,
  galleryScene,
  type GalleryScene,
} from "./scene-catalog.js";

export function ScenePicker({
  page,
  disabled,
  navigate,
}: {
  readonly page: CameraView;
  readonly disabled: boolean;
  readonly navigate: (page: CameraView) => Promise<void>;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState(false);
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const scenes = GALLERY_SCENES.filter(
    (scene) =>
      !normalizedQuery ||
      `${scene.label} ${scene.description}`
        .toLocaleLowerCase()
        .includes(normalizedQuery),
  );

  const openPicker = () => {
    dialog.current?.showModal();
    setOpen(true);
    requestAnimationFrame(() => search.current?.focus());
  };
  const closePicker = () => dialog.current?.close();

  return (
    <>
      <button
        ref={trigger}
        id="scene-picker-trigger"
        className="scene-picker-trigger"
        type="button"
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-controls="scene-picker"
        aria-disabled={disabled}
        onClick={() => {
          if (!disabled) openPicker();
        }}
      >
        <span>
          <small>Scene</small>
          <strong>{galleryScene(page).shortLabel}</strong>
        </span>
        <span className="picker-chevron" aria-hidden="true" />
      </button>
      <dialog
        ref={dialog}
        id="scene-picker"
        className="scene-picker"
        aria-labelledby="scene-picker-title"
        onClick={(event) => {
          const bounds = event.currentTarget.getBoundingClientRect();
          if (
            event.target === event.currentTarget &&
            (event.clientX < bounds.left ||
              event.clientX > bounds.right ||
              event.clientY < bounds.top ||
              event.clientY > bounds.bottom)
          )
            closePicker();
        }}
        onClose={() => {
          setOpen(false);
          setQuery("");
          trigger.current?.focus();
        }}
      >
        <header className="dialog-heading">
          <div>
            <p className="eyebrow">World gallery</p>
            <h2 id="scene-picker-title">Choose a scene</h2>
          </div>
          <button
            className="icon-button"
            type="button"
            aria-label="Close scene picker"
            onClick={closePicker}
          >
            ×
          </button>
        </header>
        <label className="scene-search" htmlFor="scene-search">
          <span className="visually-hidden">Search scenes</span>
          <input
            ref={search}
            id="scene-search"
            type="search"
            placeholder="Search scenes"
            autoComplete="off"
            value={query}
            onChange={(event) => setQuery(event.currentTarget.value)}
          />
        </label>
        <div className="scene-options" aria-label="Scenes">
          {scenes.map((scene) => (
            <SceneOption
              key={scene.id}
              scene={scene}
              current={scene.id === page}
              disabled={disabled}
              select={() => {
                closePicker();
                void navigate(scene.id);
              }}
            />
          ))}
        </div>
        {scenes.length === 0 && (
          <p className="empty-search" role="status">
            No scenes match “{query}”.
          </p>
        )}
      </dialog>
    </>
  );
}

function SceneOption({
  scene,
  current,
  disabled,
  select,
}: {
  readonly scene: GalleryScene;
  readonly current: boolean;
  readonly disabled: boolean;
  readonly select: () => void;
}) {
  return (
    <button
      id={`world-${scene.id}`}
      className="scene-option"
      type="button"
      aria-current={current ? "page" : undefined}
      disabled={disabled}
      onClick={select}
    >
      <span className="scene-option-copy">
        <strong>{scene.label}</strong>
        <span>{scene.description}</span>
      </span>
      {current && <small>Current</small>}
    </button>
  );
}

export function ResponsiveControls({
  page,
  children,
}: {
  readonly page: CameraView;
  readonly children: ReactNode;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const [mobile, setMobile] = useState(
    () => window.matchMedia("(max-width: 960px)").matches,
  );
  const [mobileOpen, setMobileOpen] = useState(false);
  const [desktopOpen, setDesktopOpen] = useState(true);
  const open = mobile ? mobileOpen : desktopOpen;

  useEffect(() => {
    const media = window.matchMedia("(max-width: 960px)");
    const change = () => setMobile(media.matches);
    media.addEventListener("change", change);
    return () => media.removeEventListener("change", change);
  }, []);

  useEffect(() => {
    const element = dialog.current;
    if (!element) return;
    if (element.open) element.close();
    if (open) {
      if (mobile) element.showModal();
      else element.show();
    }
  }, [mobile, open]);

  const setOpen = (next: boolean) => {
    if (mobile) setMobileOpen(next);
    else setDesktopOpen(next);
  };

  return (
    <>
      <button
        ref={trigger}
        id="controls-toggle"
        className="controls-toggle"
        type="button"
        aria-haspopup={mobile ? "dialog" : undefined}
        aria-expanded={open}
        aria-controls="world-controls"
        onClick={() => setOpen(!open)}
      >
        <span className="controls-glyph" aria-hidden="true" />
        <span>{open ? "Hide controls" : "Controls"}</span>
      </button>
      <dialog
        ref={dialog}
        id="world-controls"
        className="control-panel"
        aria-labelledby="controls-title"
        onCancel={() => setMobileOpen(false)}
        onClick={(event) => {
          const bounds = event.currentTarget.getBoundingClientRect();
          if (
            mobile &&
            event.target === event.currentTarget &&
            (event.clientX < bounds.left ||
              event.clientX > bounds.right ||
              event.clientY < bounds.top ||
              event.clientY > bounds.bottom)
          )
            dialog.current?.close();
        }}
        onClose={() => {
          if (dialog.current?.open) return;
          setOpen(false);
          if (mobile) trigger.current?.focus();
        }}
      >
        <header className="controls-heading">
          <div>
            <p className="eyebrow">{galleryScene(page).shortLabel}</p>
            <h2 id="controls-title">Scene controls</h2>
          </div>
          <button
            id="controls-close"
            className="icon-button"
            type="button"
            aria-label="Close scene controls"
            onClick={() => dialog.current?.close()}
          >
            ×
          </button>
        </header>
        <div className="control-panel-body">{children}</div>
      </dialog>
    </>
  );
}
