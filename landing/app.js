const hero = document.querySelector(".hero");
const journey = document.querySelector(".hero-journey");
const orb = document.querySelector(".orb-composition");
const workspace = document.querySelector(".workspace-image");
const connections = document.querySelector(".connection-map");
const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");
document.documentElement.classList.add("motion-ready");
const clamp = value => Math.max(0, Math.min(1, value));
let scene = null;
let frame = 0;
let disposed = false;
let sceneGeneration = 0;

function update() {
  frame = 0;
  if (disposed) return;
  const viewport = window.innerHeight;
  const rect = journey.getBoundingClientRect();
  const progress = reducedMotion.matches ? 0 : clamp(-rect.top / Math.max(1, journey.offsetHeight - hero.offsetHeight));
  hero.style.setProperty("--hero-progress", progress.toFixed(4));
  scene?.setProgress(progress);
  if (reducedMotion.matches) return;
  const orbRect = orb.getBoundingClientRect();
  orb.style.setProperty("--orb-drift", clamp((viewport - orbRect.top) / (viewport + orbRect.height)).toFixed(4));
  const workspaceRect = workspace.getBoundingClientRect();
  workspace.style.setProperty("--workspace-progress", clamp((viewport - workspaceRect.top) / (viewport * .75)).toFixed(4));
  const connectionRect = connections.getBoundingClientRect();
  connections.style.setProperty("--connection-progress", clamp((viewport - connectionRect.top) / (viewport + connectionRect.height)).toFixed(4));
}

function queueUpdate() {
  if (!frame && !disposed) frame = requestAnimationFrame(update);
}
async function loadScene() {
  const generation = ++sceneGeneration;
  scene?.dispose();
  scene = null;
  hero.classList.remove("webgl-ready");
  if (reducedMotion.matches || navigator.connection?.saveData || disposed) return;
  try {
    const { createLightSculpture } = await import("./src/light-sculpture.js");
    if (generation !== sceneGeneration || disposed || reducedMotion.matches) return;
    scene = createLightSculpture(document.querySelector(".light-canvas"), hero);
    queueUpdate();
  } catch {
    hero.classList.remove("webgl-ready");
  }
}
function pointerMove(event) {
  if (!scene || event.pointerType === "touch") return;
  const rect = hero.getBoundingClientRect();
  scene.setPointer((event.clientX / window.innerWidth - .5) * 2, ((event.clientY - rect.top) / rect.height - .5) * 2);
}
function pointerLeave() { scene?.setPointer(0, 0); }
function motionChanged() { loadScene(); queueUpdate(); }
function dispose() {
  disposed = true;
  sceneGeneration++;
  cancelAnimationFrame(frame);
  scene?.dispose();
  scene = null;
  window.removeEventListener("scroll", queueUpdate);
  window.removeEventListener("resize", queueUpdate);
  hero.removeEventListener("pointermove", pointerMove);
  hero.removeEventListener("pointerleave", pointerLeave);
  reducedMotion.removeEventListener("change", motionChanged);
}
window.addEventListener("scroll", queueUpdate, { passive: true });
window.addEventListener("resize", queueUpdate, { passive: true });
hero.addEventListener("pointermove", pointerMove, { passive: true });
hero.addEventListener("pointerleave", pointerLeave);
reducedMotion.addEventListener("change", motionChanged);
window.addEventListener("pagehide", dispose, { once: true });
window.addEventListener("pageshow", event => { if (event.persisted) location.reload(); });
update();
if ("requestIdleCallback" in window) requestIdleCallback(loadScene, { timeout: 900 });
else setTimeout(loadScene, 100);
