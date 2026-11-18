export function catalogueMatches(item, query) {
  const words = query.trim().toLowerCase().replace(/(^|\s)\./g, '$1').split(/\s+/).filter(Boolean);
  const text = [item.name, item.id || '', item.mime || '', ...(item.extensions || []), ...(item.aliases || [])].join(' ').toLowerCase();
  return words.every(word => text.includes(word));
}

export async function initFormatCatalogue(root) {
  const status = root.querySelector('[data-catalogue-status]');
  const controls = root.querySelector('fieldset');
  const results = root.querySelector('[data-catalogue-results]');
  const query = root.querySelector('input[type="search"]');
  const scope = root.querySelector('select');
  const mimeToggle = root.querySelector('input[type="checkbox"]');
  status.textContent = 'Loading the full catalogue…';
  try {
    const response = await fetch(new URL('../assets/data/xberg-formats.json', import.meta.url));
    if (!response.ok) throw new Error(`Catalogue response: ${response.status}`);
    const data = await response.json();
    if (data.schema !== 1 || !Array.isArray(data.formats) || !Array.isArray(data.languages)) throw new TypeError('Invalid catalogue');
    const items = [
      ...data.formats.map(item => ({ ...item, kind:'format' })),
      ...data.languages.map(item => ({ ...item, kind:'code', group:'Code & configuration', mime:'text/x-source-code', aliases:[] })),
    ];
    const groups = new Map();
    const rows = [];
    const fragment = document.createDocumentFragment();
    for (const item of items) {
      if (!groups.has(item.group)) {
        const section = document.createElement('section');
        section.className = 'catalogue-group';
        const heading = document.createElement('h3');
        heading.textContent = item.group;
        const list = document.createElement('ul');
        list.className = 'catalogue-items';
        section.append(heading, list);
        fragment.append(section);
        groups.set(item.group, { section, list, rows:[] });
      }
      const row = document.createElement('li');
      row.className = 'catalogue-item';
      row.dataset.kind = item.kind;
      const line = document.createElement('div');
      line.className = 'catalogue-line';
      const name = document.createElement('span');
      name.className = 'catalogue-name';
      name.textContent = item.name;
      const extensions = document.createElement('span');
      extensions.className = 'catalogue-extensions';
      if (item.extensions.length) extensions.textContent = item.extensions.map(extension => `.${extension}`).join(' · ');
      else extensions.textContent = item.kind === 'code' ? 'Grammar selection' : 'MIME-only format';
      line.append(name, extensions);
      const mime = document.createElement('p');
      mime.className = 'catalogue-mime';
      mime.textContent = [item.mime, ...item.aliases].join(' · ');
      row.append(line, mime);
      const entry = { item, row };
      const group = groups.get(item.group);
      group.list.append(row);
      group.rows.push(entry);
      rows.push(entry);
    }
    results.replaceChildren(fragment);
    function filter() {
      let formats = 0, languages = 0;
      for (const { item, row } of rows) {
        const matchesScope = scope.value === 'all' || scope.value === (item.kind === 'code' ? 'code' : 'formats');
        row.hidden = !matchesScope || !catalogueMatches(item, query.value);
        if (!row.hidden) { if (item.kind === 'code') languages += 1; else formats += 1; }
      }
      for (const { section, rows:groupRows } of groups.values()) section.hidden = groupRows.every(({ row }) => row.hidden);
      root.classList.toggle('catalogue-show-mime', mimeToggle.checked || Boolean(query.value.trim()));
      if (formats + languages) {
        const formatLabel = formats === 1 ? 'format' : 'formats';
        const grammarLabel = languages === 1 ? 'grammar' : 'grammars';
        status.textContent = `${formats} ${formatLabel} · ${languages} code/configuration ${grammarLabel}`;
      } else {
        status.textContent = 'No matching file types. Try a name, extension, or MIME type.';
      }
    }
    query.addEventListener('input', filter);
    scope.addEventListener('change', filter);
    mimeToggle.addEventListener('change', filter);
    controls.hidden = false;
    controls.disabled = false;
    filter();
  } catch {
    status.textContent = 'The catalogue could not be loaded. Download the complete reference list below.';
  }
}
