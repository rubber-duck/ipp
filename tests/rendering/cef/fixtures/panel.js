function setPhase(phase) {
  document.querySelector("#blue").style.background = phase
    ? "#d946ef"
    : "#2563eb";
  document.querySelector("#status").textContent = phase
    ? "Click received · texture updated"
    : "Ready for input";
  const ctx = document.querySelector("canvas").getContext("2d");
  ctx.fillStyle = "#0f172a";
  ctx.fillRect(0, 0, 250, 120);
  const values = phase ? [60, 30, 80, 50, 95, 70] : [20, 45, 35, 65, 55, 85];
  ctx.fillStyle = phase ? "#d946ef" : "#38bdf8";
  values.forEach((height, i) =>
    ctx.fillRect(10 + i * 39, 110 - height, 24, height),
  );
}
setPhase(0);
