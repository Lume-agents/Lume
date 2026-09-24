import {
  ACESFilmicToneMapping, Color, DirectionalLight, Group, InstancedMesh,
  MeshPhysicalMaterial, Object3D, PerspectiveCamera, PMREMGenerator,
  Scene, SRGBColorSpace, WebGLRenderer,
} from "three";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";
import { RoundedBoxGeometry } from "three/addons/geometries/RoundedBoxGeometry.js";

export function createLightSculpture(canvas, host) {
  const renderer = new WebGLRenderer({ canvas, alpha: true, antialias: true, powerPreference: "low-power" });
  renderer.setClearColor(0x000000, 0);
  renderer.outputColorSpace = SRGBColorSpace;
  renderer.toneMapping = ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.35;
  const scene = new Scene();
  const camera = new PerspectiveCamera(35, 1, .1, 40);
  camera.position.set(0, 0, 9);

  const environment = new RoomEnvironment();
  const pmrem = new PMREMGenerator(renderer);
  const environmentTarget = pmrem.fromScene(environment, .04);
  scene.environment = environmentTarget.texture;
  environment.dispose();
  pmrem.dispose();

  const material = new MeshPhysicalMaterial({
    color: new Color("#8c9f77"), metalness: .94, roughness: .22,
    clearcoat: .65, clearcoatRoughness: .18, envMapIntensity: 1.7,
  });
  const geometry = new RoundedBoxGeometry(.074, .82, .57, 3, .035);
  const count = window.innerWidth < 700 ? 62 : 86;
  const blades = new InstancedMesh(geometry, material, count);
  blades.frustumCulled = false;
  const sculpture = new Group();
  sculpture.add(blades);
  scene.add(sculpture);

  const key = new DirectionalLight(0xf4ffe6, 4.5);
  key.position.set(-3, 4, 5);
  scene.add(key);
  const edge = new DirectionalLight(0xcfe1ac, 3);
  edge.position.set(4, 0, -2);
  scene.add(edge);
  const dummy = new Object3D();
  let currentProgress = 0, targetProgress = 0;
  let pointerX = 0, pointerY = 0, targetX = 0, targetY = 0;
  let active = true, destroyed = false, frame = 0;
  const start = performance.now();

  function pose(progress) {
    const exponent = .78 - progress * .12;
    for (let i = 0; i < count; i++) {
      const angle = i / count * Math.PI * 2;
      const c = Math.cos(angle), s = Math.sin(angle);
      const x = Math.sign(c) * Math.pow(Math.abs(c), exponent);
      const y = Math.sign(s) * Math.pow(Math.abs(s), exponent);
      dummy.position.set(x * (2.27 + progress * .57), y * (1.13 + progress * .24), Math.sin(angle * 2) * .18);
      dummy.rotation.set(Math.sin(angle) * .28 + progress * .3, .32 + Math.sin(angle * 2) * .38, angle - Math.PI / 2);
      dummy.scale.set(1, .86 + Math.sin(angle + .6) * .12 + progress * .08, 1);
      dummy.updateMatrix();
      blades.setMatrixAt(i, dummy.matrix);
    }
    blades.instanceMatrix.needsUpdate = true;
  }

  function render(time) {
    frame = 0;
    if (destroyed || !active || document.hidden) return;
    currentProgress += (targetProgress - currentProgress) * .085;
    pointerX += (targetX - pointerX) * .065;
    pointerY += (targetY - pointerY) * .065;
    const entrance = 1 - Math.pow(1 - Math.min(1, (time - start) / 1600), 4);
    pose(currentProgress);
    sculpture.rotation.set(.28 + pointerY * .14 - currentProgress * .22, -.28 + pointerX * .19, -.12 + currentProgress * .13 + (1 - entrance) * .14);
    sculpture.scale.setScalar(.94 + entrance * .06);
    renderer.render(scene, camera);
    host.classList.add("webgl-ready");
    if (entrance < 1 || Math.abs(targetProgress - currentProgress) > .0002 || Math.abs(targetX - pointerX) > .0002 || Math.abs(targetY - pointerY) > .0002) request();
  }
  function request() { if (!frame && active && !destroyed && !document.hidden) frame = requestAnimationFrame(render); }
  function resize() {
    const bounds = canvas.parentElement.getBoundingClientRect();
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, window.innerWidth < 700 ? 1.25 : 1.6));
    renderer.setSize(bounds.width, bounds.height, false);
    camera.aspect = bounds.width / Math.max(1, bounds.height);
    camera.updateProjectionMatrix();
    request();
  }
  const observer = new IntersectionObserver(([entry]) => {
    active = entry.isIntersecting;
    if (active) request();
    else { cancelAnimationFrame(frame); frame = 0; }
  }, { threshold: .01 });
  observer.observe(host);
  const resizeObserver = new ResizeObserver(resize);
  resizeObserver.observe(canvas.parentElement);
  function visibility() { if (document.hidden) { cancelAnimationFrame(frame); frame = 0; } else request(); }
  document.addEventListener("visibilitychange", visibility);
  function contextLost(event) { event.preventDefault(); dispose(); }
  canvas.addEventListener("webglcontextlost", contextLost);
  function dispose() {
    if (destroyed) return;
    destroyed = true;
    cancelAnimationFrame(frame);
    observer.disconnect();
    resizeObserver.disconnect();
    document.removeEventListener("visibilitychange", visibility);
    canvas.removeEventListener("webglcontextlost", contextLost);
    host.classList.remove("webgl-ready");
    geometry.dispose(); material.dispose(); environmentTarget.dispose(); renderer.dispose();
    renderer.forceContextLoss();
  }
  resize();
  return {
    setProgress(value) { targetProgress = value; request(); },
    setPointer(x, y) { targetX = x; targetY = y; request(); },
    dispose,
  };
}
