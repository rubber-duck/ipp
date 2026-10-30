/** Shared form behavior for a native page document or a browser shadow root. */
export function installPageBehavior(root, signal) {
  const status = root.querySelector("output");
  root.addEventListener(
    "input",
    (event) => {
      const element = event.composedPath()[0];
      if (
        status &&
        element instanceof HTMLInputElement &&
        element.type === "range"
      )
        status.textContent = `Power: ${element.value}%`;
    },
    { signal },
  );
  root.addEventListener(
    "submit",
    (event) => {
      event.preventDefault();
      if (status && event.target instanceof HTMLFormElement) {
        const data = new FormData(event.target);
        status.style.background = "#16804b";
        status.textContent = `Saved: ${[...data.values()].join(" · ") || "ready"}`;
      }
    },
    { signal },
  );
  root.addEventListener(
    "reset",
    () => {
      if (status) status.textContent = "Form reset. Ready to try again.";
    },
    { signal },
  );
}
