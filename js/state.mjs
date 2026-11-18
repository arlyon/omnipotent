export const clamp = (value, min = 0, max = 1) => Math.min(max, Math.max(min, value));
export const smoothstep = (value) => { const t = clamp(value); return t * t * (3 - 2 * t); };

export function storyProgress(top, height, viewportHeight) {
  return clamp(-top / Math.max(1, height - viewportHeight));
}

export function modelPose(progress, separated = null, reduced = false) {
  const p = clamp(progress);
  let seated = reduced ? 1 : smoothstep((p - 0.04) / 0.26);
  let screen = 'blank';
  if (p >= 0.30) screen = p < 0.36 ? 'refresh' : 'ready';
  if (reduced) screen = 'ready';
  if (separated !== null) {
    seated = separated ? 0 : 1;
    screen = separated ? 'blank' : 'ready';
  }
  return { yaw: 0, roll: 0, seated, screen };
}

export const screenStates = Object.freeze({
  blank: { label: 'Blank paper, sled open' },
  refresh: { label: 'Refreshing the ink display' },
  ready: { label: 'Library ready, local collection available', header: 'OMNIPOTENT', title: 'LIBRARY READY', detail: 'Knowledge, kept close.', footer: 'LOCAL / OPEN / OFFLINE', icon: 'books' },
  sleep: { label: 'Sleeping, local library ready', header: 'SLEEP MODE', title: 'LIBRARY READY', detail: 'Radios asleep', footer: 'WAKE WITH MAGNET', icon: 'moon' },
  access: { label: 'Local access, Wi-Fi online', header: 'LOCAL ACCESS', title: 'ON AIR', detail: 'Wi-Fi online', footer: 'OPEN LOCAL LIBRARY', icon: 'radio' },
  index: { label: 'Indexing the local library', header: 'MEDIA ARCHIVE', title: 'INDEXING', detail: 'Preparing catalogue', footer: 'CONCEPT STATE', icon: 'books' },
});
