import { screenStates } from './state.mjs';

export const EPAPER_WIDTH = 296;
export const EPAPER_HEIGHT = 160;
// Concept pigments, not calibrated measurements of an unselected panel.
export const epaperPalette = Object.freeze([
  Object.freeze([245, 243, 233]), Object.freeze([25, 25, 24]),
  Object.freeze([196, 29, 34]), Object.freeze([248, 210, 0]),
]);
const colors = epaperPalette.map((rgb) => `rgb(${rgb.join(',')})`);
const neutralPalette = epaperPalette.slice(0, 2);

function quantize(context) {
  const image = context.getImageData(0, 0, EPAPER_WIDTH, EPAPER_HEIGHT);
  for (let i = 0; i < image.data.length; i += 4) {
    let nearest = epaperPalette[0], best = Infinity;
    // Monochrome text antialiasing must not turn into red edge pixels simply
    // because a mid-grey happens to be closer to red in RGB distance.
    const range = Math.max(image.data[i], image.data[i + 1], image.data[i + 2]) - Math.min(image.data[i], image.data[i + 1], image.data[i + 2]);
    const candidates = range < 32 ? neutralPalette : epaperPalette;
    for (const rgb of candidates) {
      const distance = rgb.reduce((sum, value, channel) => sum + (image.data[i + channel] - value) ** 2, 0);
      if (distance < best) { best = distance; nearest = rgb; }
    }
    image.data[i] = nearest[0]; image.data[i + 1] = nearest[1]; image.data[i + 2] = nearest[2]; image.data[i + 3] = 255;
  }
  context.putImageData(image, 0, 0);
}

function text(context, value, x, y, size, width, bold = false) {
  const label = String(value ?? '');
  let actual = size;
  do {
    context.font = `${bold ? 'bold ' : ''}${actual}px monospace`;
    if (context.measureText(label).width <= width || actual <= 9) break;
    actual -= 1;
  } while (actual >= 9);
  context.save(); context.beginPath(); context.rect(x, y - size, width, size + 5); context.clip();
  context.fillText(label, x, y); context.restore();
}

/** Draw arbitrary text content with one of three simple icons. No HTML is interpreted. */
export function renderEpaper(canvas, content, { blank = false, refreshing = false } = {}) {
  if (canvas.width !== EPAPER_WIDTH) canvas.width = EPAPER_WIDTH;
  if (canvas.height !== EPAPER_HEIGHT) canvas.height = EPAPER_HEIGHT;
  const ctx = canvas.getContext('2d', { willReadFrequently: true });
  if (!ctx) throw new Error('A 2D canvas is required for the e-paper display');
  const [paper, ink, red, yellow] = colors;
  ctx.fillStyle = paper; ctx.fillRect(0, 0, EPAPER_WIDTH, EPAPER_HEIGHT);
  if (!blank) {
    ctx.fillStyle = ink; ctx.fillRect(0, 0, 296, 27);
    ctx.fillStyle = yellow; ctx.fillRect(10, 10, 7, 7);
    ctx.fillStyle = paper; text(ctx, content.header, 25, 18, 12, 260, true);
    ctx.fillStyle = ink; ctx.fillRect(10, 39, 68, 62);
    if (content.icon === 'moon') {
      ctx.fillStyle = yellow; ctx.beginPath(); ctx.arc(42, 68, 20, 0, Math.PI * 2); ctx.fill();
      ctx.fillStyle = ink; ctx.beginPath(); ctx.arc(51, 59, 19, 0, Math.PI * 2); ctx.fill();
      ctx.fillStyle = paper; ctx.fillRect(59, 48, 4, 4);
    } else if (content.icon === 'radio') {
      ctx.strokeStyle = yellow; ctx.lineWidth = 3;
      for (const radius of [11, 21]) {
        ctx.beginPath(); ctx.arc(44, 68, radius, Math.PI * 0.72, Math.PI * 1.28); ctx.stroke();
        ctx.beginPath(); ctx.arc(44, 68, radius, -Math.PI * 0.28, Math.PI * 0.28); ctx.stroke();
      }
      ctx.fillStyle = red; ctx.fillRect(41, 65, 6, 6);
      ctx.fillStyle = yellow; ctx.fillRect(42, 74, 4, 14);
    } else {
      ctx.fillStyle = yellow; ctx.fillRect(21, 52, 9, 36);
      ctx.fillStyle = paper; ctx.fillRect(35, 46, 9, 42);
      ctx.fillStyle = red; ctx.fillRect(49, 58, 12, 30);
    }
    const titleSize = Number.isFinite(content.titleSize) ? Math.min(28, Math.max(9, content.titleSize)) : 19;
    ctx.fillStyle = ink; text(ctx, content.title, 91, 62, titleSize, 195, true);
    text(ctx, content.detail, 92, 85, 11, 194);
    ctx.fillRect(10, 112, 276, 2);
    text(ctx, content.footer, 10, 134, 10, 248, true);
    ctx.fillStyle = red; ctx.fillRect(270, 128, 6, 6);
  } else if (refreshing) {
    // One brief ink-clear band, not repeated black/white strobing or a glow.
    ctx.fillStyle = ink; ctx.fillRect(0, 76, 296, 3);
  }
  quantize(ctx);
}

/** Standalone canvas producer. It has no Three.js or custom-element dependency. */
export function createEpaperSurface({ canvas = document.createElement('canvas'), state = 'blank' } = {}) {
  let current, disposed = false;
  const listeners = new Set();
  function notify(label) {
    canvas.setAttribute('aria-label', `Concept e-paper screen: ${label}`);
    for (const listener of listeners) listener(canvas);
  }
  function setState(next) {
    if (!Object.hasOwn(screenStates, next)) throw new RangeError(`Unknown screen state: ${next}`);
    if (disposed || current === next) return;
    const content = screenStates[next];
    renderEpaper(canvas, content, { blank: next === 'blank' || next === 'refresh', refreshing: next === 'refresh' });
    current = next; notify(content.label);
  }
  function setContent(content) {
    if (!content || typeof content !== 'object') throw new TypeError('Display content must be an object');
    if (disposed) return;
    renderEpaper(canvas, content);
    current = 'custom'; notify(String(content.label || content.title || 'Custom content'));
  }
  setState(state);
  return {
    canvas, setState, setContent,
    get state() { return current; },
    subscribe(listener) { listeners.add(listener); return () => listeners.delete(listener); },
    dispose() { disposed = true; listeners.clear(); },
  };
}

/** <epaper-display state="ready"> with fallback children when JS is unavailable. */
export class EpaperDisplay extends HTMLElement {
  static observedAttributes = ['state'];
  #surface;
  #timer;
  #unsubscribe;
  constructor() {
    super();
    const shadow = this.attachShadow({ mode: 'open' });
    const style = document.createElement('style');
    style.textContent = ':host{display:block;aspect-ratio:296/160}canvas{display:block;width:100%;height:auto;image-rendering:pixelated}';
    this.#surface = createEpaperSurface();
    shadow.append(style, this.#surface.canvas);
  }
  connectedCallback() {
    this.setAttribute('role', 'img');
    this.#unsubscribe = this.#surface.subscribe(() => {
      this.setAttribute('aria-label', this.#surface.canvas.getAttribute('aria-label'));
      this.dispatchEvent(new CustomEvent('displaychange', { detail: { state: this.#surface.state } }));
    });
    if (!Object.hasOwn(screenStates, this.state)) {
      this.setAttribute('aria-invalid', 'true');
      this.setAttribute('aria-label', 'Invalid e-paper state');
      return;
    }
    this.#surface.setState(this.state);
    this.setAttribute('aria-label', this.#surface.canvas.getAttribute('aria-label'));
  }
  disconnectedCallback() { clearTimeout(this.#timer); this.#unsubscribe?.(); }
  attributeChangedCallback() {
    clearTimeout(this.#timer);
    if (this.#surface) {
      if (!Object.hasOwn(screenStates, this.state)) {
        this.setAttribute('aria-invalid', 'true');
        this.setAttribute('aria-label', 'Invalid e-paper state');
        return;
      }
      this.removeAttribute('aria-invalid');
      this.#surface.setState(this.state);
    }
  }
  get canvas() { return this.#surface.canvas; }
  get state() { return this.getAttribute('state') || 'ready'; }
  set state(value) {
    if (!Object.hasOwn(screenStates, value)) throw new RangeError(`Unknown screen state: ${value}`);
    this.setAttribute('state', value);
  }
  set content(value) { clearTimeout(this.#timer); this.#surface.setContent(value); }
  refreshTo(state) {
    if (!Object.hasOwn(screenStates, state)) throw new RangeError(`Unknown screen state: ${state}`);
    clearTimeout(this.#timer);
    if (matchMedia('(prefers-reduced-motion: reduce)').matches) { this.state = state; return; }
    this.#surface.setState('refresh');
    this.#timer = setTimeout(() => { this.state = state; }, 160);
  }
}
if (!customElements.get('epaper-display')) customElements.define('epaper-display', EpaperDisplay);
