import { openCraft } from './craft-editor.js';
import { mountAccount } from './accounts.js';

const createLabels = { docx: 'Documento de texto', pptx: 'Presentación', xlsx: 'Hoja de cálculo', pdf: 'PDF' };
const $ = selector => document.querySelector(selector);
const params = new URLSearchParams(location.search);
const textFile = /\.(md|markdown|txt|canvas|base)$/i;
let folder = params.get('folder') || '';
let files = [];
let editors = [];
let createEditor = null;

async function request(url, options) {
  const response = await fetch(url, options);
  const result = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(result.error || `Error del servidor (${response.status})`);
  return result;
}

function message(text, error = false) {
  $('#message').textContent = text;
  $('#message').toggleAttribute('data-error', error);
}

function preferences() {
  try { return JSON.parse(localStorage.getItem('sc-preferences')) || {}; }
  catch { return {}; }
}

function setTheme(theme) {
  document.documentElement.dataset.theme = theme;
  const toggle = $('#theme-toggle');
  const light = theme === 'light';
  const label = light ? 'Tema oscuro' : 'Tema claro';
  toggle.setAttribute('aria-label', label);
  toggle.title = label;
  toggle.querySelector('use').setAttribute('href', light ? '#icon-moon' : '#icon-sun');
}

setTheme(preferences().theme === 'light' ? 'light' : 'dark');
$('#theme-toggle').onclick = () => {
  const theme = document.documentElement.dataset.theme === 'light' ? 'dark' : 'light';
  setTheme(theme);
  try { localStorage.setItem('sc-preferences', JSON.stringify({ ...preferences(), theme })); } catch {}
};

function fileUrl(route, path) {
  return route + '?path=' + encodeURIComponent(path);
}

function goToFolder(path) {
  folder = path;
  $('#file-search').value = '';
  $('#file-type').value = '';
  history.replaceState(null, '', folder ? '/?folder=' + encodeURIComponent(folder) : '/');
  render();
}

function labelButton(label, action) {
  const button = document.createElement('button');
  button.type = 'button';
  button.textContent = label;
  button.onclick = action;
  return button;
}

function editorFor(path) {
  const extension = path.split('.').at(-1).toLowerCase();
  return editors.find(editor => editor.extension === extension);
}

function editorUrl(path) { return '/?edit=' + encodeURIComponent(path); }

function fileKind(file) {
  if (file.is_dir) return 'folder';
  if (editorFor(file.path)) return 'office';
  return textFile.test(file.path) ? 'note' : 'other';
}

function fileIcon(name) {
  const namespace = 'http://www.w3.org/2000/svg';
  const icon = document.createElementNS(namespace, 'svg');
  const use = document.createElementNS(namespace, 'use');
  icon.classList.add('hub-icon');
  icon.setAttribute('aria-hidden', 'true');
  use.setAttribute('href', '#icon-' + name);
  icon.append(use);
  return icon;
}

function render() {
  const crumbs = $('#breadcrumbs');
  crumbs.hidden = !folder;
  crumbs.replaceChildren(labelButton('Mis archivos', () => goToFolder('')));
  let path = '';
  for (const part of folder ? folder.split('/') : []) {
    path = path ? path + '/' + part : part;
    const destination = path;
    crumbs.append(' / ', labelButton(part, () => goToFolder(destination)));
  }
  crumbs.lastElementChild.setAttribute('aria-current', 'location');

  const search = $('#file-search').value.trim().toLocaleLowerCase();
  const prefix = folder ? folder + '/' : '';
  const kind = $('#file-type').value;
  const entries = files.filter(file => {
    if (!file.path.startsWith(prefix) || (kind && fileKind(file) !== kind)) return false;
    return search ? file.path.toLocaleLowerCase().includes(search) : !file.path.slice(prefix.length).includes('/');
  });
  entries.sort((a, b) => Number(b.is_dir) - Number(a.is_dir) || a.name.localeCompare(b.name, 'es'));
  const body = $('#files');
  body.replaceChildren();
  for (const file of entries) {
    const row = body.insertRow();
    const name = row.insertCell();
    const type = row.insertCell();
    const actions = row.insertCell();
    type.className = 'hub-type';
    const extension = file.name.includes('.') ? file.name.split('.').at(-1).toLowerCase() : '';
    const symbol = fileIcon(file.is_dir ? 'folder' : 'file');
    if (!file.is_dir && extension) symbol.dataset.extension = extension;
    const control = file.is_dir ? labelButton('', () => goToFolder(file.path)) : document.createElement('a');
    control.className = 'hub-file-name';
    control.append(symbol, search ? file.path : file.name);
    name.append(control);
    if (file.is_dir) {
      type.textContent = 'Carpeta';
      continue;
    }
    const editor = editorFor(file.path);
    type.textContent = extension ? extension.toUpperCase() : 'Archivo';
    const download = fileUrl('/api/download', file.path);
    control.href = editor?.available ? editorUrl(file.path)
      : textFile.test(file.path) ? fileUrl('/cerebro', file.path) : download;
    const action = document.createElement('a');
    action.className = 'hub-action';
    action.href = download;
    action.append(fileIcon('download'));
    action.setAttribute('aria-label', 'Descargar ' + file.name);
    action.title = 'Descargar';
    actions.append(action);
  }
  const count = entries.length;
  message(count ? count + (count === 1 ? ' elemento' : ' elementos')
    : search || kind ? 'Sin resultados en esta carpeta.' : 'Esta carpeta está vacía.');
}

async function refresh() {
  files = await request('/api/files');
  if (folder && !files.some(file => file.is_dir && file.path === folder)) folder = '';
  render();
}

$('#file-search').oninput = render;
$('#file-type').onchange = render;

const newMenu = $('#new-menu');
const newButton = newMenu.querySelector('summary');
document.addEventListener('click', event => {
  if (newMenu.open && !newMenu.contains(event.target)) newMenu.open = false;
});
newMenu.addEventListener('keydown', event => {
  if (event.key === 'Escape' && newMenu.open) {
    newMenu.open = false;
    newButton.focus();
  }
});
$('#upload').onclick = () => {
  newMenu.open = false;
  newButton.focus();
  $('#uploads').click();
};
$('#uploads').onchange = async event => {
  const input = event.target;
  const selected = Array.from(input.files);
  const destination = folder;
  $('#upload').disabled = true;
  const failures = [];
  for (const file of selected) {
    const path = destination ? destination + '/' + file.name : file.name;
    try { await request(fileUrl('/api/upload', path), { method: 'PUT', body: file }); }
    catch (error) { failures.push(file.name + ': ' + error.message); }
  }
  input.value = '';
  $('#upload').disabled = false;
  try {
    await refresh();
    message(`${selected.length - failures.length} archivos subidos.` + (failures.length ? ' ' + failures.join(' · ') : ''), failures.length > 0);
  } catch (error) { message(error.message, true); }
};

function promptCreate(editor = null) {
  newMenu.open = false;
  newButton.focus();
  createEditor = editor;
  $('#create-title').textContent = editor ? createLabels[editor.extension] + ' · ' + editor.name : 'Nueva carpeta';
  $('#create-name').value = '';
  $('#create-error').textContent = '';
  $('#create-dialog').showModal();
  $('#create-name').focus();
}

$('#new-folder').onclick = () => promptCreate();
$('#cancel-create').onclick = () => $('#create-dialog').close();
$('#create-form').onsubmit = async event => {
  event.preventDefault();
  const input = $('#create-name');
  const button = event.submitter;
  let name = input.value.trim();
  if (!name || name.startsWith('.') || /[/\\<>:"|?*]/.test(name)) {
    $('#create-error').textContent = 'Introduce un nombre de archivo o carpeta válido.';
    return;
  }
  const editor = createEditor;
  const suffix = editor ? '.' + editor.extension : '';
  if (suffix && !name.toLowerCase().endsWith(suffix)) name += suffix;
  const path = folder ? folder + '/' + name : name;
  if (button) button.disabled = true;
  try {
    if (!editor) {
      await request('/api/folder', {
        method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path }),
      });
    } else {
      await request(fileUrl('/api/documents', path), { method: 'POST' });
      if (editor.available) {
        location.assign(editorUrl(path));
        return;
      }
    }
    $('#create-dialog').close();
    await refresh();
    message(editor ? 'Documento creado. Puedes descargarlo desde la lista.' : 'Carpeta creada.');
  } catch (error) {
    if ($('#create-dialog').open) $('#create-error').textContent = error.message;
    else message(error.message, true);
  } finally {
    if (button) button.disabled = false;
  }
};

request('/api/hub').then(config => {
  editors = config.editors;
  mountAccount(config.account);
  const path = params.get('edit');
  if (path) return openCraft(path, editorFor(path), config.account);
  if (config.account && !config.account.owner) {
    $('#new-menu').hidden = true;
    $('.hub-tools').hidden = true;
    $('.hub-nav a[href="/cerebro"]').hidden = true;
    $('.hub-heading h1').textContent = 'Compartidos conmigo';
    return refresh();
  }

  for (const editor of editors) {
    const create = labelButton('', () => promptCreate(editor));
    create.append(fileIcon('file'), createLabels[editor.extension] + ' · ' + editor.name);
    $('#craft-create').append(create);
    const tool = labelButton('', () => promptCreate(editor));
    tool.append(fileIcon('file'), editor.name);
    if (!editor.available) {
      tool.disabled = true;
      tool.title = 'Editor pendiente de instalar en el servidor';
    }
    $('#craft-tools').append(tool);
  }
  for (const tool of config.tools) {
    const link = document.createElement('a');
    link.href = tool.url;
    link.target = '_blank';
    link.rel = 'noopener noreferrer';
    link.textContent = tool.name + ' ↗';
    $('#tools').append(link);
  }
  return refresh();
}).catch(error => message(error.message, true));
