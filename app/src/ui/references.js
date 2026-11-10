const formats = [
  { extension: 'docx', label: 'Documento de texto', editor: 'WordCraft' },
  { extension: 'pptx', label: 'Presentación', editor: 'DeckCraft' },
  { extension: 'xlsx', label: 'Hoja de cálculo', editor: 'GridCraft' },
  { extension: 'pdf', label: 'PDF', editor: 'PdfCraft' },
];

export function fileName(path) {
  return path.split('/').at(-1);
}

function formatFor(path) {
  return formats.find(({ extension }) => path.toLowerCase().endsWith('.' + extension));
}

export function referenceUrl(path) {
  const encoded = encodeURIComponent(path);
  if (formatFor(path)) return '/?edit=' + encoded;
  if (/\.(md|markdown|txt|canvas|base)$/i.test(path)) return '/cerebro?path=' + encoded;
  return '/api/download?path=' + encoded;
}

export function referenceLabel(path) {
  return formatFor(path)?.editor || 'Archivo de la bóveda';
}

export function fileMention(path) {
  return '@[[' + encodeURIComponent(path) + ']]';
}

export function decodeMention(encoded) {
  try { return decodeURIComponent(encoded); }
  catch { return null; }
}

export function atMentionStart(text) {
  return !text || /[\s([{]$/.test(text);
}

async function jsonRequest(url, options) {
  const response = await fetch(url, options);
  if (response.ok) return response.json();
  const result = await response.json().catch(() => ({}));
  throw new Error(result.error || 'No se pudo acceder a los archivos.');
}

export function pickReference({ currentPath, onSelect }) {
  if (document.querySelector('.reference-picker')) return;
  const dialog = document.createElement('dialog');
  dialog.className = 'reference-picker';
  dialog.setAttribute('aria-labelledby', 'reference-heading');
  dialog.innerHTML = [
    '<header><h2 id="reference-heading">Mencionar archivo</h2><button type="button" data-close aria-label="Cerrar">×</button></header>',
    '<input type="search" data-search aria-label="Buscar archivo" placeholder="Buscar por nombre o carpeta…" autocomplete="off">',
    '<div data-results class="reference-results"></div>',
    '<button type="button" data-new class="reference-new">＋ Crear archivo</button>',
    '<form hidden>',
    '<label>Nombre<input name="name" required maxlength="180" autocomplete="off"></label>',
    '<label>Tipo<select name="extension"></select></label>',
    '<footer><button type="button" data-back>Volver</button><button type="submit">Crear y seleccionar</button></footer>',
    '</form>',
    '<p role="status" data-status></p>',
  ].join('');
  const search = dialog.querySelector('[data-search]');
  const results = dialog.querySelector('[data-results]');
  const status = dialog.querySelector('[data-status]');
  const create = dialog.querySelector('[data-new]');
  const form = dialog.querySelector('form');
  const submit = form.querySelector('[type=submit]');
  let files = null;

  const parent = currentPath.includes('/') ? currentPath.slice(0, currentPath.lastIndexOf('/') + 1) : '';
  const options = form.elements.extension;
  for (const { extension, label } of formats) options.add(new Option(label, extension));

  function select(path) {
    dialog.close();
    onSelect(path);
  }

  function render() {
    if (!files) { status.textContent = 'Cargando archivos…'; return; }
    const query = search.value.trim().toLocaleLowerCase();
    const matches = files.filter(file => !file.is_dir && file.path !== currentPath && file.path.toLocaleLowerCase().includes(query));
    results.replaceChildren();
    for (const file of matches.slice(0, 60)) {
      const row = document.createElement('button');
      const label = document.createElement('strong');
      const detail = document.createElement('small');
      row.type = 'button';
      label.textContent = fileName(file.path);
      detail.textContent = file.path;
      row.append(label, detail);
      row.onclick = () => select(file.path);
      results.append(row);
    }
    status.textContent = matches.length ? '' : 'No hay archivos que coincidan.';
  }

  function showCreation(show) {
    form.hidden = !show;
    search.hidden = results.hidden = create.hidden = show;
    status.textContent = '';
    if (show) {
      form.elements.name.value = search.value.trim();
      form.elements.name.focus();
    } else {
      render();
      search.focus();
    }
  }

  dialog.querySelector('[data-close]').onclick = () => dialog.close();
  dialog.addEventListener('close', () => dialog.remove(), { once: true });
  create.onclick = () => showCreation(true);
  dialog.querySelector('[data-back]').onclick = () => showCreation(false);
  search.oninput = render;
  search.onkeydown = event => {
    if (event.key === 'Enter' || event.key === 'ArrowDown') {
      const first = results.querySelector('button');
      event.preventDefault();
      if (event.key === 'ArrowDown') first?.focus();
      else if (first) first.click();
      else showCreation(true);
    }
  };
  form.onsubmit = async event => {
    event.preventDefault();
    const name = form.elements.name.value.trim();
    if (!name || name.startsWith('.') || /[/\\<>:"|?*\u0000-\u001f]/.test(name)) {
      status.textContent = 'Introduce un nombre de archivo válido.';
      return;
    }
    const suffix = '.' + options.value;
    const path = parent + name + (name.toLowerCase().endsWith(suffix) ? '' : suffix);
    submit.disabled = true;
    status.textContent = 'Creando archivo…';
    try {
      await jsonRequest('/api/documents?path=' + encodeURIComponent(path), { method: 'POST' });
      if (dialog.open) select(path);
    } catch (error) {
      status.textContent = error.message;
    } finally {
      submit.disabled = false;
    }
  };
  document.body.append(dialog);
  dialog.showModal();
  search.focus();
  status.textContent = 'Cargando archivos…';
  jsonRequest('/api/files').then(entries => {
    files = entries;
    if (form.hidden) render();
  }).catch(error => { status.textContent = error.message; });
}
