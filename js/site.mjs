import { storyProgress, screenStates } from './state.mjs';
import { collections, collectionSequence, collectionContent, collectionProgress } from './collections.mjs';
import './epaper.mjs';
import A11yDialog from '../assets/vendor/a11y-dialog/a11y-dialog.esm.js';
import { initFormatCatalogue } from './catalogue.mjs';

const dialogElement = document.querySelector('#details-dialog');
const dialogBody = document.querySelector('#details-dialog-body');
const detailSources = new Map();
// Enhance existing disclosures without duplicating their content or losing the no-JS version.
for (const details of document.querySelectorAll('details[data-dialog-label]')) {
  const summary = details.querySelector('summary');
  const trigger = document.createElement('button');
  trigger.type = 'button';
  trigger.className = 'inline-link details-trigger';
  trigger.textContent = summary.textContent.trim();
  const arrow = document.createElement('span');
  arrow.textContent = '↗';
  arrow.setAttribute('aria-hidden', 'true');
  trigger.append(arrow);
  details.before(trigger);
  detailSources.set(details.id, {
    title: details.dataset.dialogTitle || summary.textContent.trim(),
    label: details.dataset.dialogLabel,
    content: [...details.childNodes].filter(node => node !== summary),
    trigger,
  });
  for (const opener of [trigger, ...document.querySelectorAll(`a[href="#${details.id}"]`)]) {
    opener.dataset.a11yDialogShow = dialogElement.id;
    opener.dataset.dialogSource = details.id;
    opener.setAttribute('aria-haspopup', 'dialog');
    opener.setAttribute('aria-controls', dialogElement.id);
    if (opener.tagName === 'A') opener.addEventListener('click', event => event.preventDefault());
  }
  details.hidden = true;
}
const detailsDialog = new A11yDialog(dialogElement);
let inertedBackground = [];
detailsDialog.on('show', event => {
  const opener = event.detail?.target?.closest('[data-dialog-source]');
  const source = detailSources.get(opener?.dataset.dialogSource || location.hash.slice(1));
  if (!source) { event.preventDefault(); return; }
  document.querySelector('#details-dialog-title').textContent = source.title;
  dialogElement.querySelector('.eyebrow').textContent = source.label;
  dialogBody.replaceChildren(...source.content);
  // The library records the opener after this event; let it move focus before making the opener inert.
  queueMicrotask(() => {
    if (!detailsDialog.shown || inertedBackground.length) return;
    inertedBackground = [...document.body.children].filter(element => element !== dialogElement && !element.inert);
    inertedBackground.forEach(element => { element.inert = true; });
  });
  document.documentElement.classList.add('modal-open');
  dialogBody.scrollTop = 0;
});
detailsDialog.on('hide', () => {
  inertedBackground.forEach(element => { element.inert = false; });
  inertedBackground = [];
  document.documentElement.classList.remove('modal-open');
});
function openDetailsFromHash() {
  const source = detailSources.get(location.hash.slice(1));
  if (!source) return;
  detailsDialog.hide();
  source.trigger.focus({ preventScroll:true });
  detailsDialog.show();
}
window.addEventListener('hashchange', openDetailsFromHash);
// Initial fragment navigation must finish before the modal takes keyboard focus.
window.addEventListener('load', openDetailsFromHash, { once:true });
initFormatCatalogue(document.querySelector('#format-catalogue'));

const reduced = window.matchMedia('(prefers-reduced-motion: reduce)');
const screen = document.querySelector('#epaper');
screen.state = 'sleep';
document.querySelectorAll('[data-screen]').forEach((button) => {
  button.disabled = false;
  button.addEventListener('click', () => {
    if (!Object.hasOwn(screenStates, button.dataset.screen)) return;
    document.querySelectorAll('[data-screen]').forEach((other) => other.setAttribute('aria-pressed', String(other === button)));
    screen.refreshTo(button.dataset.screen);
  });
});

const story = document.querySelector('#story');
const viewport = document.querySelector('#product-viewport');
const host = document.querySelector('#model-host');
const status = document.querySelector('#model-status');
const assemblyButton = document.querySelector('#assembly-toggle');
const motionButton = document.querySelector('#motion-toggle');
const controls = document.querySelector('.assembly-controls');
const resetViewButton = document.querySelector('#reset-view');
const stageHint = document.querySelector('.stage-index');
const orbit = document.querySelector('#collection-orbit');
const library = document.querySelector('#library');
const collectionDisplay = document.querySelector('#collection-display');
const countLabel = document.querySelector('#collection-count');
const stage = document.querySelector('.product-stage');
const bubbles = new Map([...document.querySelectorAll('[data-bubble]')].map((element) => [element.dataset.bubble, element]));
let model = null, paused = false, separated = null, queued = false, lastProgress = 0;
let anchor = { x: 0.5, y: 0.43 }, lastCount = -1;

function progress() {
  const rect = story.getBoundingClientRect();
  // Assembly occupies the opening viewport, independent of collection length.
  return storyProgress(rect.top, window.innerHeight * 1.7, window.innerHeight);
}

function updateMotion() {
  queued = false;
  if (paused) return;
  lastProgress = progress();
  const rect = library.getBoundingClientRect();
  const entered = rect.top <= window.innerHeight * 0.45;
  const active = entered && rect.bottom > window.innerHeight * 0.2;
  const amount = collectionProgress(rect.top, rect.height, window.innerHeight);
  const sequence = collectionSequence(reduced.matches ? 1 : amount, { anchor, reduced: reduced.matches });
  orbit.hidden = reduced.matches || !active;
  stage.classList.toggle('collecting', active && !reduced.matches);
  if (model) {
    const text = active ? `${sequence.loaded} / ${sequence.total} collections` : '3D model';
    if (status.textContent !== text) status.textContent = text;
  }
  if (!orbit.hidden) {
    for (const bubble of sequence.bubbles) {
      const element = bubbles.get(bubble.id);
      element.style.left = `${bubble.x * 100}%`;
      element.style.top = `${bubble.y * 100}%`;
      element.style.transform = `translate(-50%, -50%) scale(${bubble.scale})`;
      element.style.opacity = String(bubble.opacity);
      element.dataset.landed = String(bubble.landed);
    }
  }
  if (sequence.loaded !== lastCount) {
    lastCount = sequence.loaded;
    countLabel.textContent = `${sequence.loaded} / ${sequence.total} collections`;
    collectionDisplay.content = collectionContent(sequence);
  }
  model?.update({ progress: lastProgress, separated, reduced: reduced.matches,
    collection: entered ? sequence : null, collecting: active && !reduced.matches });
}
function scheduleMotion() {
  if (queued || paused) return;
  queued = true; requestAnimationFrame(updateMotion);
}
function showFallback() {
  viewport.classList.remove('model-ready');
  controls.hidden = true;
  resetViewButton.hidden = true;
  stageHint.textContent = 'Scroll to explore';
  status.textContent = 'Static view · 3D unavailable';
}
assemblyButton.addEventListener('click', () => {
  separated = Number(host.dataset.seated || 0) >= 0.5;
  model?.update({ progress: lastProgress, separated, reduced: reduced.matches });
});
resetViewButton.addEventListener('click', () => {
  model?.resetView();
  host.focus({ preventScroll:true });
});
motionButton.addEventListener('click', () => {
  paused = !paused;
  motionButton.setAttribute('aria-pressed', String(paused));
  motionButton.setAttribute('aria-label', paused ? 'Resume scroll-linked animation' : 'Pause scroll-linked animation');
  motionButton.textContent = paused ? '▷' : 'Ⅱ';
  if (!paused) scheduleMotion();
});

async function startModel() {
  if (reduced.matches) {
    status.textContent = 'Static view · reduced motion';
    return;
  }
  status.textContent = 'Loading 3D assembly…';
  try {
    const { createModel } = await import('./model.mjs');
    model = await createModel(host, {
      onReady() {
        viewport.classList.add('model-ready');
        status.textContent = '3D model';
        controls.hidden = false;
        stageHint.textContent = 'Drag to rotate';
      },
      onViewChange({ rotated }) { resetViewButton.hidden = !rotated; },
      onFailure: showFallback,
      onPose({ seated, screenAnchor }) {
        const closed = seated >= 0.999;
        assemblyButton.setAttribute('aria-pressed', String(closed));
        assemblyButton.firstChild.textContent = closed ? 'Open base ' : 'Insert base ';
        assemblyButton.querySelector('span').textContent = closed ? '↗' : '↙';
        if (Math.abs(anchor.x - screenAnchor.x) + Math.abs(anchor.y - screenAnchor.y) > 0.002) {
          anchor = screenAnchor;
          scheduleMotion();
        }
      },
    });
    updateMotion();
  } catch {
    showFallback();
  }
}
window.addEventListener('scroll', scheduleMotion, { passive: true });
window.addEventListener('resize', scheduleMotion, { passive: true });
const librarySize = new ResizeObserver(scheduleMotion);
librarySize.observe(library);
reduced.addEventListener('change', () => {
  motionButton.disabled = reduced.matches;
  motionButton.setAttribute('aria-label', reduced.matches ? 'Scroll movement disabled by reduced-motion preference' : 'Pause scroll-linked animation');
  if (!reduced.matches && !model) startModel();
  if (reduced.matches) { paused = false; motionButton.setAttribute('aria-pressed', 'false'); }
  updateMotion();
});
window.addEventListener('pagehide', (event) => {
  if (!event.persisted) {
    window.removeEventListener('hashchange', openDetailsFromHash);
    window.removeEventListener('load', openDetailsFromHash);
    detailsDialog.destroy(); librarySize.disconnect(); model?.dispose();
  }
});
// Disable no-JS-only labels without hiding the actual collection descriptions.
countLabel.textContent = `0 / ${collections.length} collections`;
updateMotion();
startModel();
