import { clamp, smoothstep } from './state.mjs';

export const collections = Object.freeze([
  { id: 'wikipedia', label: 'Wikipedia', screen: 'Wikipedia', format: 'ZIM', description: 'An encyclopedia without a connection.' },
  { id: 'maps', label: 'Street maps', screen: 'Street maps', format: 'MBTiles / PMTiles / GPX', description: 'Streets, terrain, and routes kept locally.' },
  { id: 'medicine', label: 'Medical wiki', screen: 'Medical wiki', format: 'ZIM / HTML', description: 'An open medical reference, with its sources and revision dates.' },
  { id: 'science', label: 'Science', screen: 'Scientific papers', format: 'PDF / HTML / JATS', description: 'Medical journals and open-access scientific literature.' },
  { id: 'manuals', label: 'Manuals', screen: 'Technical manuals', format: 'PDF / HTML / SVG', description: 'Service manuals, datasheets, and practical engineering.' },
  { id: 'books', label: 'Books', screen: 'Books & reference', format: 'EPUB / PDF / OPDS', description: 'A considered shelf, ready for your own reader.' },
]);

export function collectionProgress(top, height, viewportHeight) {
  return clamp((viewportHeight * 0.45 - top) / Math.max(1, height - viewportHeight * 0.55));
}

/** One full orbit, one arrival per segment. Reversing scroll reverses the state. */
export function collectionSequence(progress, { anchor = { x: 0.5, y: 0.43 }, reduced = false } = {}) {
  const phase = clamp(progress) * collections.length;
  const loaded = Math.min(collections.length, Math.floor(phase + 1e-9));
  const bubbles = collections.map((item, index) => {
    const angle = (index - phase) * Math.PI / 3 - Math.PI / 2;
    const local = clamp(phase - index);
    const travel = smoothstep((local - 0.4) / 0.5);
    const orbitX = 0.5 + Math.cos(angle) * 0.38;
    const orbitY = 0.5 + Math.sin(angle) * 0.37;
    const landed = index < loaded;
    return {
      id: item.id, landed,
      x: orbitX + (anchor.x - orbitX) * travel,
      y: orbitY + (anchor.y - orbitY) * travel,
      scale: 1 - 0.8 * travel,
      opacity: landed || reduced ? 0 : 1 - smoothstep((local - 0.85) / 0.15),
    };
  });
  return { loaded, total: collections.length, bubbles,
    last: loaded ? collections[loaded - 1] : null,
    complete: loaded === collections.length };
}

export function collectionContent(sequence) {
  return {
    header: sequence.complete ? 'LIBRARY READY' : 'LOCAL LIBRARY',
    title: `${String(sequence.loaded).padStart(2, '0')} / ${String(sequence.total).padStart(2, '0')}`,
    titleSize: 28,
    detail: sequence.last?.screen || 'Building your library',
    footer: 'COLLECTION STUDY', icon: 'books',
    label: `${sequence.loaded} of ${sequence.total} illustrative collections loaded${sequence.last ? `, ${sequence.last.label}` : ''}`,
  };
}
