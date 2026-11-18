import A11yDialog from '../assets/vendor/a11y-dialog/a11y-dialog.esm.js';

const element = document.querySelector('#asset-dialog');
const dialog = new A11yDialog(element);
const image = document.querySelector('#asset-dialog-image');
const title = document.querySelector('#asset-dialog-title');
const download = document.querySelector('#asset-dialog-download');
const error = document.querySelector('#asset-dialog-error');
let inerted = [];

// Original-file links remain usable if JavaScript or the dialog library fails.
for (const link of document.querySelectorAll('.asset-preview')) {
  link.setAttribute('aria-haspopup', 'dialog');
  link.setAttribute('aria-controls', element.id);
  link.addEventListener('click', event => {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    const thumbnail = link.querySelector('img');
    title.textContent = link.closest('figure')?.querySelector('.asset-title')?.textContent || '3D concept assembly';
    image.alt = thumbnail.alt;
    error.hidden = true;
    image.hidden = false;
    image.src = link.href;
    download.href = link.href;
    dialog.show(event);
  });
}
image.addEventListener('error', () => {
  image.hidden = true;
  error.hidden = false;
});
dialog.on('show', () => {
  document.documentElement.classList.add('modal-open');
  // Let the library remember the opener and move focus before making it inert.
  queueMicrotask(() => {
    if (!dialog.shown || inerted.length) return;
    inerted = [...document.body.children].filter(child => child !== element && !child.inert);
    inerted.forEach(child => { child.inert = true; });
  });
  element.querySelector('.modal-body').scrollTop = 0;
});
dialog.on('hide', () => {
  inerted.forEach(child => { child.inert = false; });
  inerted = [];
  document.documentElement.classList.remove('modal-open');
  image.removeAttribute('src');
});
