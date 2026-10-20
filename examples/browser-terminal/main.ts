import { mountTerminal } from "./application.js";
const button = document.querySelector<HTMLButtonElement>("button")!;
const status = document.querySelector<HTMLElement>("#status")!;
let mounted: Awaited<ReturnType<typeof mountTerminal>> | undefined;
button.addEventListener("click", async () => {
  button.disabled = true;
  try {
    if (mounted) {
      await mounted.close();
      mounted = undefined;
      status.textContent = "Session stopped. Reload to start a new session.";
      return;
    }
    status.textContent = "Starting shell…";
    mounted = await mountTerminal(
      document.querySelector("canvas")!,
      new URL("./", import.meta.url).href,
      (error) => {
        status.textContent = error.message;
      },
    );
    status.textContent =
      "Click the terminal to type. Ctrl+Shift+Escape releases keyboard focus.";
    button.textContent = "Stop session";
    button.disabled = false;
  } catch (error) {
    status.textContent = String(error);
  }
});
