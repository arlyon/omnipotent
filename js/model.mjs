import * as THREE from 'three';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { RoomEnvironment } from 'three/addons/environments/RoomEnvironment.js';
import { modelPose, smoothstep } from './state.mjs';
import { createEpaperSurface } from './epaper.mjs';
import { collectionContent } from './collections.mjs';

export async function createModel(host, { onReady, onFailure, onPose = () => {}, onViewChange = () => {} }) {
  const renderer = new THREE.WebGLRenderer({ alpha: true, antialias: true, powerPreference: 'low-power' });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 1.5));
  renderer.toneMapping = THREE.NeutralToneMapping;
  renderer.toneMappingExposure = 0.95;
  renderer.setClearColor(0x000000, 0);
  renderer.domElement.setAttribute('aria-hidden', 'true');
  host.append(renderer.domElement);
  const scene = new THREE.Scene();
  const camera = new THREE.OrthographicCamera(-1.4, 1.4, 1.4, -1.4, 0.01, 20);
  // The source is tilted in its own coordinates. Look along the measured
  // display normal, with its long axis vertical, rather than adding a yaw.
  const target = new THREE.Vector3(0, 0.605380237, -0.795936388).multiplyScalar(0.2);
  camera.position.copy(target).addScaledVector(new THREE.Vector3(0, 0.795936388, 0.605380237), 5);
  camera.lookAt(target);
  const room = new RoomEnvironment();
  const pmrem = new THREE.PMREMGenerator(renderer);
  const environment = pmrem.fromScene(room, 0.04);
  scene.environment = environment.texture;
  scene.environmentIntensity = 0.6;
  room.dispose(); pmrem.dispose();
  scene.add(new THREE.HemisphereLight(0xf9f4ed, 0x554640, 0.9));
  const key = new THREE.DirectionalLight(0xfff8f0, 1.8);
  key.position.set(-3, 4, 5); scene.add(key);
  // Inspection is a separate parent transform, so scrolling can still animate the assembly.
  const inspection = new THREE.Group(); inspection.position.copy(target); scene.add(inspection);
  const assembly = new THREE.Group(); assembly.position.copy(target).negate(); inspection.add(assembly);
  const viewUp = new THREE.Vector3(0, 0.605380237, -0.795936388).normalize();
  const viewRight = new THREE.Vector3(1, 0, 0);
  const yawRotation = new THREE.Quaternion(), pitchRotation = new THREE.Quaternion();
  let viewYaw = 0, viewPitch = 0, drag = null;
  const display = createEpaperSurface();
  const texture = new THREE.CanvasTexture(display.canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.flipY = false;
  texture.magFilter = THREE.NearestFilter;
  texture.minFilter = THREE.NearestFilter;
  texture.generateMipmaps = false;
  let model, sled, fragments, origin, fragmentOrigin, seatOffset, panel;
  let disposed = false, visible = true, frame = 0, currentSeated = 0, transition = null;
  let displayKey = '', modelScale = 1;
  let latest = { progress: 0, separated: null, reduced: false, collection: null, collecting: false };
  const unsubscribe = display.subscribe(() => { texture.needsUpdate = true; requestRender(); });

  function render(now = performance.now()) {
    frame = 0;
    if (disposed || !visible) return;
    const { width, height } = host.getBoundingClientRect();
    if (width <= 0 || height <= 0) return;
    const ratio = width / height;
    const halfHeight = Math.max(1.5 - (height < 500 ? 0.35 * currentSeated : 0), 0.82 / ratio);
    camera.left = -halfHeight * ratio; camera.right = halfHeight * ratio;
    camera.top = halfHeight; camera.bottom = -halfHeight;
    camera.updateProjectionMatrix();
    renderer.setSize(width, height, false);
    if (model) {
      const pose = modelPose(latest.progress, latest.separated, latest.reduced);
      assembly.rotation.set(0, pose.yaw, pose.roll);
      currentSeated = pose.seated;
      let state = pose.screen;
      if (transition && !latest.reduced) {
        const elapsed = now - transition.started;
        currentSeated = THREE.MathUtils.lerp(transition.from, pose.seated, smoothstep(elapsed / 600));
        if (currentSeated < 0.999) state = 'blank';
        else if (elapsed < 760) state = 'refresh';
        if (elapsed >= 760) transition = null;
      }
      sled.position.copy(origin).addScaledVector(seatOffset, currentSeated);
      if (fragments) fragments.position.copy(fragmentOrigin).addScaledVector(seatOffset, currentSeated);
      const targetScale = latest.collecting && width >= 500 ? 0.84 : 1;
      modelScale = latest.reduced ? targetScale : THREE.MathUtils.lerp(modelScale, targetScale, 0.22);
      if (Math.abs(modelScale - targetScale) < 0.001) modelScale = targetScale;
      assembly.scale.setScalar(modelScale);
      if (latest.collection && currentSeated >= 0.999) {
        const key = `collection:${latest.collection.loaded}`;
        if (key !== displayKey) display.setContent(collectionContent(latest.collection));
        displayKey = key;
        state = latest.collection.complete ? 'ready' : 'collections';
      } else {
        display.setState(state);
        displayKey = state;
      }
      scene.updateMatrixWorld(true);
      const screenPosition = new THREE.Box3().setFromObject(panel).getCenter(new THREE.Vector3()).project(camera);
      const screenAnchor = { x: (screenPosition.x + 1) / 2, y: (1 - screenPosition.y) / 2 };
      host.dataset.seated = currentSeated.toFixed(3);
      host.dataset.screen = state;
      host.dataset.loaded = String(latest.collection?.loaded || 0);
      onPose({ seated: currentSeated, screen: state, screenAnchor });
      if (transition || modelScale !== targetScale) requestRender();
    }
    renderer.render(scene, camera);
  }
  function requestRender() {
    if (!frame && !disposed && visible) frame = requestAnimationFrame(render);
  }
  function setView(yaw, pitch) {
    viewYaw = Math.atan2(Math.sin(yaw), Math.cos(yaw));
    viewPitch = THREE.MathUtils.clamp(pitch, -Math.PI / 2 + 0.05, Math.PI / 2 - 0.05);
    yawRotation.setFromAxisAngle(viewUp, viewYaw);
    pitchRotation.setFromAxisAngle(viewRight, viewPitch);
    inspection.quaternion.multiplyQuaternions(pitchRotation, yawRotation);
    host.dataset.viewYaw = viewYaw.toFixed(4);
    host.dataset.viewPitch = viewPitch.toFixed(4);
    onViewChange({ rotated:Math.abs(viewYaw) + Math.abs(viewPitch) > 0.0001 });
    requestRender();
  }
  function endDrag(event) {
    if (!drag || event.pointerId !== drag.id) return;
    const id = drag.id;
    drag = null;
    host.classList.remove('is-dragging');
    if (renderer.domElement.hasPointerCapture(id)) renderer.domElement.releasePointerCapture(id);
  }
  function resetView() {
    if (disposed) return;
    if (drag) endDrag({ pointerId:drag.id });
    setView(0, 0);
  }
  function pointerDown(event) {
    if (!model || !event.isPrimary || event.button !== 0 || drag) return;
    drag = { id:event.pointerId, x:event.clientX, y:event.clientY };
    renderer.domElement.setPointerCapture(event.pointerId);
    host.classList.add('is-dragging');
    if (event.pointerType !== 'touch') {
      event.preventDefault();
      host.focus({ preventScroll:true });
    }
  }
  function pointerMove(event) {
    if (!drag || event.pointerId !== drag.id) return;
    const rect = host.getBoundingClientRect();
    const dx = event.clientX - drag.x, dy = event.clientY - drag.y;
    drag.x = event.clientX; drag.y = event.clientY;
    // One-finger vertical swipes remain native page scrolling on touchscreens.
    const tilt = event.pointerType === 'touch' ? 0 : dy / Math.max(rect.height, 1) * Math.PI;
    setView(viewYaw + dx / Math.max(rect.width, 1) * Math.PI * 2, viewPitch + tilt);
  }
  function keyDown(event) {
    if (!model || event.altKey || event.ctrlKey || event.metaKey) return;
    const step = Math.PI / 12;
    switch (event.key) {
      case 'ArrowLeft': setView(viewYaw - step, viewPitch); break;
      case 'ArrowRight': setView(viewYaw + step, viewPitch); break;
      case 'ArrowUp': setView(viewYaw, viewPitch - step); break;
      case 'ArrowDown': setView(viewYaw, viewPitch + step); break;
      case 'Home': resetView(); break;
      default: return;
    }
    event.preventDefault();
  }
  renderer.domElement.addEventListener('pointerdown', pointerDown);
  renderer.domElement.addEventListener('pointermove', pointerMove);
  for (const event of ['pointerup', 'pointercancel', 'lostpointercapture']) renderer.domElement.addEventListener(event, endDrag);
  host.addEventListener('keydown', keyDown);

  function update(state) {
    if (state.separated !== undefined && state.separated !== latest.separated && state.separated !== null) {
      transition = { from: currentSeated, started: performance.now() };
    }
    if (state.separated === null || state.reduced === true) transition = null;
    latest = { ...latest, ...state };
    requestRender();
  }
  const resize = new ResizeObserver(requestRender); resize.observe(host);
  const intersection = new IntersectionObserver(([entry]) => { visible = entry.isIntersecting; if (visible) requestRender(); });
  intersection.observe(host);
  function releaseModel(root) {
    const textures = new Set(), materials = new Set(), geometries = new Set();
    root.traverse((object) => {
      if (!object.isMesh) return;
      geometries.add(object.geometry);
      for (const material of Array.isArray(object.material) ? object.material : [object.material]) {
        materials.add(material);
        for (const value of Object.values(material)) if (value?.isTexture) textures.add(value);
      }
    });
    textures.forEach((item) => item.dispose());
    materials.forEach((material) => material.dispose());
    geometries.forEach((geometry) => geometry.dispose());
  }
  function dispose() {
    if (disposed) return;
    disposed = true;
    if (drag) endDrag({ pointerId:drag.id });
    renderer.domElement.removeEventListener('pointerdown', pointerDown);
    renderer.domElement.removeEventListener('pointermove', pointerMove);
    for (const event of ['pointerup', 'pointercancel', 'lostpointercapture']) renderer.domElement.removeEventListener(event, endDrag);
    host.removeEventListener('keydown', keyDown);
    host.setAttribute('aria-hidden', 'true');
    for (const name of ['tabindex', 'role', 'aria-label', 'aria-describedby']) host.removeAttribute(name);
    cancelAnimationFrame(frame); resize.disconnect(); intersection.disconnect(); unsubscribe(); display.dispose();
    if (model) releaseModel(model);
    texture.dispose(); environment.dispose(); renderer.dispose(); renderer.domElement.remove();
  }
  renderer.domElement.addEventListener('webglcontextlost', (event) => {
    event.preventDefault(); onFailure(); dispose();
  });
  try {
    const gltf = await new GLTFLoader().loadAsync(new URL('../assets/models/omnipotent/omnipotent-1k.glb', import.meta.url).href);
    model = gltf.scene;
    if (disposed) { releaseModel(model); return { update, dispose, resetView }; }
    const root = model.getObjectByName('omnipotent');
    panel = model.getObjectByName('epaper_surface');
    sled = model.getObjectByName('sled_assembly');
    fragments = model.getObjectByName('unidentified_fragments');
    const offset = root?.userData.sled_seat_offset;
    if (!sled || !panel?.isMesh || !Array.isArray(offset) || offset.length !== 3 || !offset.every(Number.isFinite)) {
      throw new Error('Prepared screen or calibrated sled offset is missing');
    }
    seatOffset = new THREE.Vector3(...offset);
    origin = sled.position.clone();
    fragmentOrigin = fragments?.position.clone();
    panel.material.dispose();
    // Flat ink, no normal/bump input and no lighting-derived lettering or glare.
    panel.material = new THREE.MeshBasicMaterial({ map: texture, toneMapped: false });
    assembly.add(model);
    host.removeAttribute('aria-hidden');
    host.tabIndex = 0;
    host.setAttribute('role', 'group');
    host.setAttribute('aria-label', 'Rotatable 3D concept: red and porcelain Omnipotent with a removable power base');
    host.setAttribute('aria-describedby', 'model-instructions');
    setView(0, 0);
    render(); onReady();
    return { update, dispose, resetView };
  } catch (error) {
    dispose(); throw error;
  }
}
