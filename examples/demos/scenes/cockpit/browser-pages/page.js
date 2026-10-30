import { installPageBehavior } from "./behavior.js";
if (document.readyState === "loading")
  document.addEventListener(
    "DOMContentLoaded",
    () => installPageBehavior(document),
    { once: true },
  );
else installPageBehavior(document);
