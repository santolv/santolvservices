import { Heading1 as H1, Heading2 as H2, Heading3 as H3, createElement, ArrowLeft, ArrowRight, ArrowUpRight, AtSign, Bold, Bookmark, BookOpen, CalendarDays, Check, ChevronDown, ChevronLeft, ChevronRight, ChevronsDownUp, ChevronsUpDown, CircleHelp, CodeXml, Columns2, Download, ExternalLink, File, FilePlus2, Files, FileText, Focus, Folder, FolderOpen, FolderPlus, Gem, Grip, Hash, Image, Italic, LayoutDashboard, Link, Link2 as Links, List, ListFilter, MessageSquare, Minus, MoreHorizontal, Network, PanelLeft, PanelLeftClose, PanelRight, Pencil, Plus, Quote, Rows2, Save, Search, Settings, SlidersHorizontal, SquareTerminal, StickyNote, SunMoon, Tags, Trash2, Type, Upload, X } from 'lucide';
import { fileMention } from './references.js';
import { mountChat } from './chat.js';
import { createEditor } from './editor.js';
import { createNotion } from './notion.js';
import { createId as uid, readPage, setProperties, localDate, remapPath, rewriteKnowledgeReferences } from './knowledge.js';
import { escape, title, renderMarkdown, wikiTargets } from './markdown.js';

const $ = (selector, parent = document) => parent.querySelector(selector);
const $$ = (selector, parent = document) => [...parent.querySelectorAll(selector)];
const icons = { H1, H2, H3, ArrowLeft, ArrowRight, ArrowUpRight, AtSign, Bold, Bookmark, BookOpen, CalendarDays, Check, ChevronDown, ChevronLeft, ChevronRight, ChevronsDownUp, ChevronsUpDown, CircleHelp, CodeXml, Columns2, Download, ExternalLink, File, FilePlus2, Files, FileText, Focus, Folder, FolderOpen, FolderPlus, Gem, Grip, Hash, Image, Italic, LayoutDashboard, Link, Links, List, ListFilter, MessageSquare, Minus, MoreHorizontal, Network, PanelLeft, PanelLeftClose, PanelRight, Pencil, Plus, Quote, Rows2, Save, Search, Settings, SlidersHorizontal, SquareTerminal, StickyNote, SunMoon, Tags, Trash2, Type, Upload, X };
const icon = (name, size = 18) => createElement(Object.hasOwn(icons, name) ? icons[name] : icons.File, { width: size, height: size, 'stroke-width': 1.65, 'aria-hidden': 'true' }).outerHTML;
const button = (name, label, action, extra = '') => `<button class="icon-button ${extra}" title="${escape(label)}" aria-label="${escape(label)}" data-action="${action}">${icon(name)}</button>`;
const post = (path, body, method = 'POST') => api(path, { method, headers: { 'Content-Type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body) });
async function api(path, options) { const response = await fetch(path, options); const data = await response.json(); if (!response.ok) throw new Error(data.error || 'No se pudo completar la operación'); return data; }
const safeStorage = { get(key, fallback) { try { return JSON.parse(localStorage.getItem(key)) ?? fallback; } catch { return fallback; } }, set(key, value) { try { localStorage.setItem(key, JSON.stringify(value)); return true; } catch { return false; } } };
const prefs = { theme: 'dark', fontSize: 16, readable: true, spellcheck: false, defaultMode: 'live', ...safeStorage.get('sc-preferences', {}) };
const state = { files: [], documents: new Map(), groups: [{ id: uid(), tabs: [], active: null }], activeGroup: null, left: true, right: true, leftView: 'files', rightView: 'outline', folders: new Set(), bookmarks: [], history: [], historyIndex: -1, timers: new Map(), saves: new Map(), sort: 'asc' };
state.activeGroup = state.groups[0].id;
let workspaceTimer, searchTimer, graphCleanup = [], modalCleanup = null;
const app = $('#app');

app.innerHTML = `<div class="app-body"><aside class="sidebar sidebar-left" id="leftSidebar" aria-label="Panel izquierdo"><div class="sc-side-brand"><button class="vault-switcher" data-action="vault-menu">${icon('Gem',20)}<span><strong>Super Cerebro</strong><small>Bóveda local</small></span>${icon('ChevronDown',13)}</button>${button('PanelLeftClose','Ocultar panel izquierdo','toggle-left')}</div>
<div class="sc-side-scroll">
  <nav class="sc-sidebar-nav" aria-label="Navegación principal">
    <div class="sc-side-search-row"><button class="sc-side-search" data-action="search">${icon('Search',16)}<span>Buscar</span><kbd>Ctrl Alt F</kbd></button>${button('Plus','Nueva página · Ctrl+Alt+N','new-page','sc-side-create')}</div>
    <button class="sc-side-link" data-action="home">${icon('LayoutDashboard',17)} Inicio</button>
    <button class="sc-side-link" data-action="calendar">${icon('CalendarDays',17)} Calendario</button>
    <button class="sc-side-link" data-action="daily">${icon('CalendarDays',17)} Diario</button>
    <button class="sc-side-link" data-action="chat" aria-controls="aiChat">${icon('MessageSquare',17)} Chat con IA</button>
    <button class="sc-side-link" data-action="graph">${icon('Network',17)} Grafo</button>
    <a class="sc-side-link" href="/">${icon('Files',17)} Mis archivos</a>
  </nav>
  <details class="sc-side-section" id="favoriteSection" open><summary>${icon('ChevronRight',12)} Favoritos</summary><div id="favoriteNotes"></div></details>
  <section class="sc-side-section sc-side-library" aria-label="Biblioteca">
    <div class="sc-side-section-heading"><span>Biblioteca</span><div>${button('FolderPlus','Nueva carpeta','new-folder')}${button('ListFilter','Cambiar orden de archivos','sort')}${button('ChevronsDownUp','Contraer todas las carpetas','collapse-folders')}</div></div>
    <div class="sc-side-views" aria-label="Organizar la biblioteca"><button data-action="left-files">Archivos</button><button data-action="left-pages">Jerarquía</button></div>
    <div class="left-content" id="leftContent"></div>
  </section>
</div>
<nav class="sc-side-footer" aria-label="Herramientas de la bóveda"><button class="sc-side-link" data-action="templates">${icon('BookOpen',16)} Plantillas</button><button class="sc-side-link" data-action="import">${icon('Upload',16)} Importar</button><button class="sc-side-link" data-action="settings">${icon('Settings',16)} Ajustes</button></nav></aside><div class="resize-handle" id="resizeLeft" role="separator" tabindex="0" aria-label="Ancho del panel izquierdo" aria-orientation="vertical"></div>
<main id="workspace" class="workspace" aria-label="Espacio de trabajo"></main>
<div class="resize-handle" id="resizeRight" role="separator" tabindex="0" aria-label="Ancho del panel derecho" aria-orientation="vertical"></div><aside class="sidebar sidebar-right" id="rightSidebar" aria-label="Panel derecho"><div class="sidebar-tabs"><div role="tablist" aria-label="Información de la nota">${button('List', 'Esquema', 'right-outline', 'selected')}${button('Links', 'Enlaces entrantes', 'right-backlinks')}${button('ArrowUpRight', 'Enlaces salientes', 'right-outgoing')}${button('Tags', 'Etiquetas', 'right-tags')}${button('SlidersHorizontal', 'Propiedades', 'right-properties')}</div></div><div id="rightContent" class="right-content"></div></aside><aside id="aiChat" class="ai-chat" aria-label="Chat con IA" hidden></aside></div>
<footer class="status-bar"><span id="saveStatus" role="status">Bóveda local</span><span class="status-spacer"></span><span id="fileCount"></span><span id="wordCount"></span><button id="modeStatus" data-action="cycle-mode"></button></footer><div id="overlays"></div><div id="toast" role="status"></div><input id="importFiles" type="file" accept=".md,.markdown,.txt,.canvas,.base" multiple hidden>`;

const chat = mountChat($('#aiChat'), { storage: safeStorage, icon });
function chatWithFile() { const doc = activeDoc(); if (doc) chat.open(doc.path); }

function notify(message, error = false) { const el = $('#toast'); el.textContent = message; el.className = error ? 'show error' : 'show'; clearTimeout(el.timer); el.timer = setTimeout(() => el.className = '', 3500); }
function report(error) { console.error(error); notify(error.message || String(error), true); }
function run(promise) { Promise.resolve(promise).catch(report); }
function activeGroup() { return state.groups.find(group => group.id === state.activeGroup) || state.groups[0]; }
function activeTab() { const group = activeGroup(); return group?.tabs.find(tab => tab.id === group.active); }
function activeDoc() { return state.documents.get(activeTab()?.path); }
function persist() {
  clearTimeout(workspaceTimer); workspaceTimer = setTimeout(() => run(post('/api/workspace', { groups: state.groups.map(g => ({ id: g.id, size: g.size, active: g.active, tabs: g.tabs.map(({ id, path, type, mode, date }) => ({ id, path, type, mode, date })) })), horizontal: state.horizontal, leftView: state.leftView, activeGroup: state.activeGroup, left: state.left, right: state.right, leftWidth: $('#leftSidebar').style.width, rightWidth: $('#rightSidebar').style.width, folders: [...state.folders], bookmarks: state.bookmarks, rightView: state.rightView })), 300);
}
function applyPrefs() { document.documentElement.dataset.theme = prefs.theme; $('meta[name=theme-color]').content = getComputedStyle(document.documentElement).getPropertyValue('--ss-chrome').trim(); document.documentElement.style.setProperty('--editor-font-size', `${prefs.fontSize}px`); document.documentElement.classList.toggle('readable', prefs.readable); safeStorage.set('sc-preferences', prefs); }
function setDraft(path, content) { const drafts = safeStorage.get('sc-drafts', {}); if (content === null) delete drafts[path]; else drafts[path] = content; safeStorage.set('sc-drafts', drafts); }
async function loadDocument(path) {
  if (state.documents.has(path)) return state.documents.get(path);
  const response = await api('/api/file?path=' + encodeURIComponent(path));
  const content = response.content.replace(/\r\n?/g, '\n');
  const draft = safeStorage.get('sc-drafts', {})[path];
  const doc = { path, content: draft ?? content, saved: content, dirty: draft !== undefined && draft !== content };
  state.documents.set(path, doc); return doc;
}
async function saveDocument(doc) {
  clearTimeout(state.timers.get(doc.path));
  const previous = state.saves.get(doc.path) || Promise.resolve();
  const task = previous.catch(() => {}).then(async () => {
    if (!doc.dirty) return;
    const snapshot = doc.content, path = doc.path;
    $('#saveStatus').textContent = 'Guardando…';
    try { await post('/api/file', { path, content: snapshot }); doc.saved = snapshot; doc.dirty = doc.content !== snapshot; if (!doc.dirty) setDraft(path, null); status(); renderTabStates(); }
    catch (error) { $('#saveStatus').textContent = 'Error al guardar · cambios recuperables'; throw error; }
  });
  state.saves.set(doc.path, task); await task;
}
function changed(doc, content, origin) {
  doc.content = content; doc.dirty = content !== doc.saved; setDraft(doc.path, doc.dirty ? content : null);
  clearTimeout(state.timers.get(doc.path)); state.timers.set(doc.path, setTimeout(() => run(saveDocument(doc)), 600));
  for (const group of state.groups) for (const tab of group.tabs) {
    if (tab.path !== doc.path || tab.id === origin) continue;
    if (tab.editor && tab.editor.view.state.doc.toString() !== content) { tab.syncing = true; tab.editor.view.dispatch({ changes: { from: 0, to: tab.editor.view.state.doc.length, insert: content } }); tab.syncing = false; }
    if (tab.mode === 'reading' && tab.body) tab.body.innerHTML = renderMarkdown(content);
    if (tab.surface) tab.surface.refresh(content);
  }
  status(); renderTabStates(); renderRight();
  document.dispatchEvent(new CustomEvent('sc-document-changed', { detail: { path: doc.path, origin } }));
}
function status() {
  const doc = activeDoc(), tab = activeTab();
  $('.app-body').classList.toggle('sc-at-home', tab?.type === 'home');
  for (const [action,type] of [['home','home'],['graph','graph'],['calendar','calendar']]) {
    const link=$('.sc-side-link[data-action='+action+']');
    if (tab?.type===type) link.setAttribute('aria-current','page');
    else link.removeAttribute('aria-current');
  }
  $$('#leftSidebar [data-path]').forEach(row=>row.classList.toggle('active',row.dataset.path===tab?.path));
  $('.app-body').classList.toggle('workspace-wide', ['home', 'database', 'calendar'].includes(tab?.type));
  $('#saveStatus').textContent = doc?.dirty ? 'Cambios sin guardar' : doc ? 'Todos los cambios guardados' : 'Bóveda local';
  $('#fileCount').textContent = `${state.files.filter(f => !f.is_dir).length} archivos`;
  const content = doc && tab?.type === 'note' ? readPage(doc.content).body : doc?.content || ''; $('#wordCount').textContent = doc && tab?.type === 'note' ? `${content.trim() ? content.trim().split(/\s+/u).length : 0} palabras  ·  ${content.length} caracteres` : '';
  $('#modeStatus').innerHTML = tab?.type === 'note' ? `${icon(tab.mode === 'reading' ? 'BookOpen' : tab.mode === 'source' ? 'CodeXml' : tab.mode === 'blocks' ? 'Columns2' : 'Pencil', 14)} ${modeLabel(tab.mode)}` : '';
  $('#modeStatus').hidden = tab?.type !== 'note' || isMarkdown(tab.path);
  document.title = tab ? `${tab.type === 'calendar' ? 'Calendario' : tab.type === 'home' ? 'Inicio' : tab.type === 'graph' ? 'Vista de grafo' : tab.path ? title(tab.path) : 'Nueva pestaña'} — Super Cerebro` : 'Super Cerebro';
}
const isMarkdown = path => /\.(md|markdown)$/i.test(path || '');
const modeLabel = mode => ({ blocks: 'Bloques', live: 'Vista en vivo', source: 'Modo fuente', reading: 'Vista de lectura' }[mode]);
async function refreshFiles() {
  state.files = (await api('/api/files')).filter(file => file.is_dir || /\.(md|markdown|txt|canvas|base)$/i.test(file.path));
  renderLeft();
  status();
}
function resolveNote(raw, from = activeTab()?.path) {
  const direct = raw.split('|')[0].split('#')[0]; if (ctxFile(direct)) return direct;
  const path = raw.split('|')[0].split('#')[0].replace(/\\/g, '/').replace(/^\.\//, '').replace(/\.md$/i, '');
  if (!path && raw.includes('#')) return from;
  const relative = from?.includes('/') ? from.slice(0, from.lastIndexOf('/') + 1) + path + '.md' : path + '.md';
  return state.files.find(f => f.path === path + '.md')?.path || state.files.find(f => f.path === relative)?.path || state.files.find(f => !f.is_dir && title(f.path).toLocaleLowerCase() === path.toLocaleLowerCase())?.path;
}
function scrollToHeading(body, heading) {
  $$('h1,h2,h3,h4,h5,h6', body).find(el => el.textContent.trim() === heading)?.scrollIntoView({ block: 'start' });
}
function ctxFile(path) { return state.files.some(file => !file.is_dir && file.path === path); }
async function followLink(raw) {
  const path = resolveNote(raw);
  if (path) { await openNote(path); const heading = raw.split('#')[1]; if (heading) { const doc = activeDoc(), tab = activeTab(); const line = doc.content.split('\n').findIndex(line => line.replace(/^#+\s*/, '').trim() === heading); if (line >= 0) tab.editor?.line(line + 1); if (!tab.editor && tab.body) scrollToHeading(tab.body, heading); } }
  else { const name = raw.split('|')[0].split('#')[0]; await createNote(name); }
}
async function openNote(path, options = {}) {
  const group = options.group || activeGroup();
  const doc = await loadDocument(path);
  safeStorage.set("sc-recent-files", [path, ...recentFiles().filter(file => file !== path)].slice(0, 30));
  let tab = group.tabs.find(t => t.path === path);
  if (!tab) {
    const previous = group.tabs.find(t => t.id === group.active);
    tab = { id: uid(), type: path.endsWith('.base') ? 'database' : path.endsWith('.canvas') ? 'canvas' : 'note', path, mode: isMarkdown(path) ? 'blocks' : options.mode || prefs.defaultMode };
    if (!options.newTab && previous && ['note', 'empty', 'canvas', 'database'].includes(previous.type)) { previous.editor?.destroy(); previous.surface?.destroy(); group.tabs.splice(group.tabs.indexOf(previous), 1, tab); }
    else group.tabs.push(tab);
  }
  if (tab.type === 'note') {
    if (isMarkdown(path)) tab.mode = 'blocks';
    else if (options.mode) tab.mode = options.mode;
  }
  group.active = tab.id; state.activeGroup = group.id;
  if (!options.noHistory && state.history[state.historyIndex] !== path) { state.history = state.history.slice(0, state.historyIndex + 1); state.history.push(path); state.historyIndex = state.history.length - 1; }
  renderWorkspace(); renderLeft(); renderRight(); status(); persist();
  if (doc.dirty) notify('Se han recuperado cambios sin guardar de esta nota');
}
function newTab(group = activeGroup()) { const tab = { id: uid(), type: 'empty', mode: prefs.defaultMode }; group.tabs.push(tab); group.active = tab.id; state.activeGroup = group.id; renderWorkspace(); renderLeft(); status(); renderRight(); persist(); }
async function closeTab(group, id) {
  const tab = group.tabs.find(t => t.id === id), doc = state.documents.get(tab?.path);
  if (doc?.dirty) await saveDocument(doc);
  const index = group.tabs.findIndex(t => t.id === id); group.tabs.splice(index, 1);
  tab?.editor?.destroy(); tab?.surface?.destroy();
  if (group.active === id) group.active = group.tabs[Math.min(index, group.tabs.length - 1)]?.id;
  if (!group.tabs.length && state.groups.length > 1) { state.groups.splice(state.groups.indexOf(group), 1); state.activeGroup = state.groups[0].id; }
  renderWorkspace(); renderLeft(); renderRight(); status(); persist();
}
function renderTabStates() { $$('[data-tab-id]').forEach(el => { const group = state.groups.find(g => g.id === el.dataset.groupId), tab = group?.tabs.find(t => t.id === el.dataset.tabId); el.classList.toggle('dirty', !!state.documents.get(tab?.path)?.dirty); }); }
function renderWorkspace() {
  for (const group of state.groups) for (const tab of group.tabs) { tab.surface?.destroy(); tab.surface = null; }
  graphCleanup.forEach(fn => fn()); graphCleanup = [];
  for (const group of state.groups) for (const tab of group.tabs) if (tab.editor) { tab.scroll = tab.editor.view.scrollDOM.scrollTop; tab.selection = tab.editor.view.state.selection.main.head; tab.snapshot = tab.editor.snapshot(); tab.editor.destroy(); tab.editor = null; }
  const workspace = $('#workspace'); workspace.innerHTML = ''; workspace.classList.toggle('horizontal', state.horizontal === true);
  for (const group of state.groups) {
    const pane = document.createElement('section'); pane.className = 'pane'; pane.dataset.group = group.id;
    pane.style.flex = `${group.size || 1} 1 0%`;
    if (workspace.children.length) {
      const divider = document.createElement('div'); divider.className = 'pane-divider'; divider.tabIndex = 0; divider.setAttribute('role', 'separator'); divider.setAttribute('aria-label', 'Redimensionar división'); divider.setAttribute('aria-orientation', state.horizontal ? 'horizontal' : 'vertical');
      divider.onpointerdown = event => { const panes = $$('.pane', workspace); panes.forEach(p => { const g = state.groups.find(g => g.id === p.dataset.group), rect = p.getBoundingClientRect(); g.size = state.horizontal ? rect.height : rect.width; p.style.flex = `${g.size} 1 0%`; }); const previous = divider.previousElementSibling, previousGroup = state.groups.find(g => g.id === previous.dataset.group), total = previousGroup.size + group.size, start = state.horizontal ? event.clientY : event.clientX, initial = previousGroup.size; divider.setPointerCapture(event.pointerId); document.body.classList.add('resizing'); divider.onpointermove = move => { const delta = (state.horizontal ? move.clientY : move.clientX) - start; previousGroup.size = Math.max(120, Math.min(total - 120, initial + delta)); group.size = total - previousGroup.size; previous.style.flex = `${previousGroup.size} 1 0%`; pane.style.flex = `${group.size} 1 0%`; }; divider.onpointerup = () => { divider.onpointermove = null; document.body.classList.remove('resizing'); persist(); }; };
      workspace.append(divider);
    }
    pane.addEventListener('pointerdown', () => { if (state.activeGroup !== group.id) { state.activeGroup = group.id; status(); renderRight(); } });
    const tab = group.tabs.find(t => t.id === group.active);
    pane.dataset.view = tab?.type || 'empty';
    pane.innerHTML = `<div class="workspace-bar"><div class="tab-strip" role="tablist" aria-label="Notas abiertas">${group.tabs.map(t => `<div class="workspace-tab ${t.id === group.active ? 'active' : ''} ${state.documents.get(t.path)?.dirty ? 'dirty' : ''}" role="tab" tabindex="0" aria-selected="${t.id === group.active}" draggable="true" data-tab-id="${t.id}" data-group-id="${group.id}" title="${escape(t.path || (t.type === 'calendar' ? 'Calendario' : t.type === 'home' ? 'Inicio' : t.type === 'graph' ? 'Vista de grafo' : 'Nueva pestaña'))}">${icon(t.type === 'calendar' ? 'CalendarDays' : t.type === 'home' ? 'Gem' : t.type === 'database' ? 'Columns2' : t.type === 'graph' ? 'Network' : t.type === 'canvas' ? 'LayoutDashboard' : t.type === 'empty' ? 'FilePlus2' : 'FileText', 15)}<span>${escape(t.type === 'calendar' ? 'Calendario' : t.type === 'home' ? 'Inicio' : t.type === 'graph' ? 'Vista de grafo' : t.path ? title(t.path) : 'Nueva pestaña')}</span><button class="tab-close" aria-label="Cerrar pestaña" title="Cerrar pestaña · Ctrl+Alt+W">${icon('X', 13)}</button></div>`).join('')}${button('Plus', 'Nueva pestaña · Ctrl+Alt+T', 'new-tab')}<span class="tab-spacer"></span>${button('ChevronDown', 'Lista de pestañas', 'tab-list')}</div><div class="workspace-tools" role="toolbar" aria-label="Controles del espacio de trabajo">${button('SquareTerminal', 'Paleta de comandos · Ctrl+Alt+P', 'commands')}${button('PanelLeft', 'Mostrar u ocultar panel izquierdo', 'toggle-left')}${button('PanelRight', 'Mostrar u ocultar panel derecho', 'toggle-right')}</div></div><div class="note-header"><div class="navigation-buttons">${button('ArrowLeft', 'Volver · Ctrl+Alt+J', 'back')}${button('ArrowRight', 'Avanzar · Ctrl+Alt+L', 'forward')}</div><div class="breadcrumb">${escape(tab?.path?.includes('/') ? tab.path.slice(0, tab.path.lastIndexOf('/')).replaceAll('/', ' / ') : '')}${tab?.path?.includes('/') ? ' / ' : ''}<span>${escape(tab?.path ? title(tab.path) : tab?.type === 'home' ? 'Escritorio' : tab?.type === 'graph' ? 'Vista de grafo' : '')}</span></div><div class="note-actions">${tab?.path ? button("MessageSquare", "Preguntar sobre este archivo", "chat-with-file") : ""}${tab?.type === 'note' ? button('AtSign', 'Mencionar archivo · Ctrl+Alt+M', 'mention-file') : ''}${tab?.type === 'note' && !isMarkdown(tab.path) ? button(tab.mode === 'reading' ? 'Pencil' : 'BookOpen', tab.mode === 'reading' ? 'Cambiar a edición · Ctrl+Alt+E' : 'Cambiar a lectura · Ctrl+Alt+E', 'toggle-reading') : ''}${button('MoreHorizontal', 'Más opciones', 'note-menu')}</div></div><div class="pane-content"></div>`;
    workspace.append(pane);
    const content = $('.pane-content', pane);
    if (!tab || tab.type === 'empty') { content.innerHTML = `<div class="empty-state"><div class="empty-gem">${icon('Gem', 64)}</div><h1>No hay ningún archivo abierto</h1><button data-action="switcher">Abrir una nota existente <kbd>Ctrl Alt O</kbd></button><button data-action="new-note">Crear una nota nueva <kbd>Ctrl Alt N</kbd></button><button data-action="commands">Abrir paleta de comandos <kbd>Ctrl Alt P</kbd></button><button data-action="graph">Explorar la vista de grafo <kbd>Ctrl Alt G</kbd></button><p>Tu espacio para pensar y conectar ideas.</p></div>`; }
    else if (tab.type === 'home') tab.surface = notion.mountHome(content);
    else if (tab.type === 'calendar') tab.surface = notion.mountCalendar(content,tab);
    else if (tab.type === 'database') tab.surface = notion.mountDatabase(content, tab);
    else if (tab.type === 'note' && isMarkdown(tab.path)) { tab.body = content; tab.surface = notion.mountBlocks(content, tab); }
    else if (tab.type === 'graph') mountGraph(content, tab);
    else if (tab.type === 'canvas') tab.surface = mountCanvas(content, tab);
    else {
      const doc = state.documents.get(tab.path);
      const page = document.createElement('div'); page.className = 'note-page'; content.append(page);
      const name = document.createElement('input'); name.className = 'inline-title'; name.value = title(tab.path); name.setAttribute('aria-label', 'Título de la nota'); name.spellcheck = false; page.append(name);
      name.onkeydown = event => { if (event.key === 'Enter') { event.preventDefault(); name.blur(); } if (event.key === 'Escape') { event.preventDefault(); name.value = title(tab.path); name.blur(); } };
      name.onblur = () => { if (name.value.trim() === title(tab.path)) return; const parent = tab.path.includes('/') ? tab.path.slice(0, tab.path.lastIndexOf('/') + 1) : ''; run(renamePath(tab.path, parent + name.value.trim() + '.md').catch(error => { name.value = title(tab.path); throw error; })); };
      const body = document.createElement('div'); body.className = tab.mode === 'reading' ? 'markdown-preview' : 'markdown-editor'; page.append(body); tab.body = body;
      if (tab.mode === 'reading') body.innerHTML = renderMarkdown(doc.content);
      else { tab.editor = createEditor(body, doc.content, tab.mode, content => { if (!tab.syncing) changed(doc, content, tab.id); }, raw => run(followLink(raw)), tab.snapshot, () => state.files.filter(f => !f.is_dir && f.path.endsWith('.md')).map(f => f.path), () => tab.path); tab.editor.view.contentDOM.spellcheck = prefs.spellcheck; if (tab.selection != null) tab.editor.view.dispatch({ selection: { anchor: Math.min(tab.selection, doc.content.length) } }); tab.editor.view.scrollDOM.scrollTop = tab.scroll || 0; }
    }
    $$('[data-tab-id]', pane).forEach(el => {
      el.onclick = event => { if (event.target.closest('.tab-close')) return run(closeTab(group, el.dataset.tabId)); group.active = el.dataset.tabId; state.activeGroup = group.id; renderWorkspace(); renderLeft(); renderRight(); status(); persist(); };
      el.onkeydown = event => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); el.click(); } };
      el.onauxclick = event => { if (event.button === 1) { event.preventDefault(); run(closeTab(group, el.dataset.tabId)); } };
      el.ondragstart = event => { event.dataTransfer.setData('application/sc-tab', JSON.stringify({ group: group.id, tab: el.dataset.tabId })); };
      el.ondragover = event => event.preventDefault();
      el.ondrop = event => { event.preventDefault(); const data = event.dataTransfer.getData('application/sc-tab'); if (!data) return; const drag = JSON.parse(data), source = state.groups.find(g => g.id === drag.group), index = source.tabs.findIndex(t => t.id === drag.tab), moved = source.tabs.splice(index, 1)[0]; group.tabs.splice(group.tabs.findIndex(t => t.id === el.dataset.tabId), 0, moved); group.active = moved.id; if (source !== group && source.active === moved.id) source.active = source.tabs[0]?.id; state.groups = state.groups.filter(g => g.tabs.length || g === group); renderWorkspace(); persist(); };
    });
  }
  renderTabStates();
}

function renderLeft() {
  const root = $('#leftContent');
  const favorites=$('#favoriteNotes'), paths=state.bookmarks.filter(path=>state.files.some(file=>file.path===path&&!file.is_dir));
  favorites.innerHTML=paths.length ? paths.map(path=>`<button class="tree-row ${activeTab()?.path===path?'active':''}" data-path="${escape(path)}" title="${escape(path)}">${icon('Bookmark',15)}<span>${escape(title(path))}</span></button>`).join('') : '<p class="sc-side-empty">Sin favoritos todavía</p>';
  bindTree(favorites);
  $$('.sc-side-views [data-action]').forEach(el=>el.setAttribute('aria-pressed',String(el.dataset.action==='left-'+state.leftView)));
  if (state.leftView === 'pages') { run(notion.mountPageTree(root)); return; }
  if (state.leftView === 'search') {
    const old = $('#vaultSearch')?.value || ''; root.innerHTML = `<div class="search-controls"><div class="search-field">${icon('Search', 16)}<input id="vaultSearch" placeholder="Buscar en la bóveda…" aria-label="Buscar en la bóveda" value="${escape(old)}">${button('X', 'Limpiar búsqueda', 'clear-search')}</div><div class="search-options">Coincidencias en nombres y contenido</div></div><div id="searchResults" class="search-results"><p class="muted empty-hint">Escribe para buscar en todas tus notas.</p></div>`;
    $('#vaultSearch').oninput = () => { clearTimeout(searchTimer); searchTimer = setTimeout(() => run(searchVault()), 180); }; if (old) run(searchVault()); return;
  }
  root.innerHTML = '<div class="file-tree" id="fileTree" role="tree" aria-label="Archivos de la bóveda"></div>';
  const tree = $('#fileTree');
  const entries = [...state.files].sort((a, b) => Number(b.is_dir) - Number(a.is_dir) || (state.sort === 'asc' ? 1 : -1) * a.name.localeCompare(b.name, 'es', { numeric: true }));
  function branch(parent, depth) {
    for (const file of entries.filter(file => file.path.slice(0, file.path.lastIndexOf('/') + 1) === parent)) {
      const expanded = state.folders.has(file.path);
      const row = document.createElement('button'); row.className = `tree-row ${file.is_dir ? 'folder' : ''} ${activeTab()?.path === file.path ? 'active' : ''}`; row.dataset.path = file.path; row.dataset.directory = file.is_dir; row.setAttribute('role', 'treeitem'); row.style.paddingLeft = `${12 + depth * 16}px`; row.title = file.path;
      if (file.is_dir) row.setAttribute('aria-expanded', expanded);
      row.innerHTML = file.is_dir ? `${icon(expanded ? 'ChevronDown' : 'ChevronRight', 12)}${icon(expanded ? 'FolderOpen' : 'Folder', 15)}<span>${escape(file.name)}</span>` : `<span class="tree-file-spacer"></span>${file.path.endsWith('.base') ? icon('Columns2', 14) : file.path.endsWith('.canvas') ? icon('LayoutDashboard', 14) : icon('FileText', 14)}<span>${escape(title(file.path))}</span>`;
      tree.append(row); if (file.is_dir && expanded) branch(file.path + '/', depth + 1);
    }
  }
  branch('', 0); if (!state.files.length) tree.innerHTML = '<p class="empty-hint">Tu bóveda está vacía.<br>Crea tu primera nota.</p>';
  bindTree(root);
}
function bindTree(root) {
  $$('[data-path]', root).forEach(row => { row.onclick = event => { if (row.dataset.directory === 'true') { state.folders.has(row.dataset.path) ? state.folders.delete(row.dataset.path) : state.folders.add(row.dataset.path); renderLeft(); persist(); } else run(openNote(row.dataset.path, { newTab: event.ctrlKey || event.metaKey })); }; row.oncontextmenu = event => { event.preventDefault(); fileMenu(event, row.dataset.path, row.dataset.directory === 'true'); }; row.draggable = true; row.ondragstart = event => event.dataTransfer.setData('application/sc-file', row.dataset.path); if (row.dataset.directory === 'true') { row.ondragover = event => event.preventDefault(); row.ondrop = event => { event.preventDefault(); const from = event.dataTransfer.getData('application/sc-file'); if (from && !row.dataset.path.startsWith(from + '/')) run(renamePath(from, row.dataset.path + '/' + from.split('/').pop())); }; } });
}
async function searchVault() { const input = $('#vaultSearch'), query = input?.value.trim(); if (!input) return; if (!query) { $('#searchResults').innerHTML = '<p class="empty-hint">Escribe para buscar en todas tus notas.</p>'; return; } const hits = (await api('/api/search?q=' + encodeURIComponent(query))).filter(hit => state.files.some(file => file.path === hit.path)); if ($('#vaultSearch')?.value.trim() !== query) return; const root = $('#searchResults'); root.innerHTML = `<div class="search-count">${hits.length} resultados</div>${hits.filter(hit => !hit.path.startsWith('.')).map(hit => `<button class="search-result" data-path="${escape(hit.path)}"><strong>${icon('FileText', 14)} ${escape(title(hit.path))}</strong><small>${escape(hit.path)}</small><p>${escape(hit.snippet)}</p></button>`).join('')}`; bindTree(root); }

let rightRequest = 0;
async function renderRight() {
  const request = ++rightRequest, root = $('#rightContent'), doc = activeDoc(), tab = activeTab();
  $$('.sidebar-right [data-action]').forEach(el => el.classList.toggle('selected', el.dataset.action === `right-${state.rightView}`));
  const names = { outline: 'Esquema', backlinks: 'Enlaces entrantes', outgoing: 'Enlaces salientes', tags: 'Etiquetas', properties: 'Propiedades' };
  root.innerHTML = `<div class="panel-heading">${names[state.rightView]}</div><div id="rightDetails"></div>`;
  const details = $('#rightDetails'); if (!doc || tab.type !== 'note') { details.innerHTML = '<p class="empty-hint">Abre una nota para ver su información.</p>'; return; }
  if (state.rightView === 'outline') {
    let fenced = false; const headings = []; doc.content.split('\n').forEach((text, index) => { if (/^```/.test(text)) fenced = !fenced; const match = /^(#{1,6})\s+(.+)/.exec(text); if (!fenced && match) headings.push({ level: match[1].length, text: match[2], line: index + 1 }); });
    details.innerHTML = headings.length ? headings.map(h => `<button class="outline-row" style="padding-left:${12 + (h.level - 1) * 14}px" data-line="${h.line}">${icon('ChevronRight', 11)}<span>${escape(h.text)}</span></button>`).join('') : '<p class="empty-hint">No se han encontrado encabezados.</p>';
    $$('[data-line]', details).forEach(row => row.onclick = () => { if (tab.editor) tab.editor.line(Number(row.dataset.line)); else if (tab.body) scrollToHeading(tab.body, row.textContent.trim()); });
  } else if (state.rightView === 'outgoing') {
    const targets = [...new Set(wikiTargets(doc.content))]; details.innerHTML = `<div class="section-label">${targets.length} enlaces</div>` + (targets.length ? targets.map(target => `<button class="info-link" data-note="${escape(target)}">${icon('FileText', 14)}${escape(target)}</button>`).join('') : '<p class="empty-hint">Conecta una idea con [[otra nota]].</p>');
  } else if (state.rightView === 'backlinks') {
    try { const links = await api('/api/backlinks?name=' + encodeURIComponent(title(doc.path))); if (request !== rightRequest) return; details.innerHTML = `<div class="section-label">${icon('ChevronDown', 12)} MENCIONES ENLAZADAS <span>${links.length}</span></div>` + (links.length ? links.map(path => `<button class="info-link" data-path="${escape(path)}">${icon('FileText', 14)}${escape(title(path))}</button>`).join('') : '<p class="empty-hint">No hay menciones enlazadas.</p>'); bindTree(details); } catch (e) { if (request === rightRequest) details.textContent = e.message; }
  } else if (state.rightView === 'tags') {
    const counts = new Map(); const docs = await Promise.all(state.files.filter(f => !f.is_dir && f.path.endsWith('.md')).map(file => loadDocument(file.path).catch(() => null))); if (request !== rightRequest) return;
    docs.filter(Boolean).forEach(d => { for (const match of d.content.matchAll(/(?:^|\s)#([\p{L}\p{N}_/-]+)/gu)) counts.set(match[1], (counts.get(match[1]) || 0) + 1); });
    details.innerHTML = counts.size ? [...counts].sort((a,b) => b[1] - a[1]).map(([tag, count]) => `<button class="tag-row" data-tag="${escape(tag)}">${icon('Hash', 14)}<span>${escape(tag)}</span><small>${count}</small></button>`).join('') : '<p class="empty-hint">Añade #etiquetas a tus notas.</p>';
  } else {
    const props = Object.entries(readPage(doc.content).meta).map(([key, value]) => [key, typeof value === 'object' ? JSON.stringify(value) : String(value ?? '')]);
    details.innerHTML = `<div class="property-row"><span>${icon('FileText', 14)} Archivo</span><strong>${escape(tab.path)}</strong></div>${props.map(([key, value]) => `<div class="property-row"><span>${icon('List', 14)}${escape(key)}</span><strong>${escape(value)}</strong></div>`).join('')}<button class="text-button" data-action="add-property">${icon('Plus', 14)} Añadir propiedad</button>`;
  }
}

function toggleSidebar(side) { state[side] = !state[side]; updateSidebars(); persist(); }
function updateSidebars() { $('#leftSidebar').classList.toggle('hidden', !state.left); $('#rightSidebar').classList.toggle('hidden', !state.right); $('#resizeLeft').classList.toggle('hidden', !state.left); $('#resizeRight').classList.toggle('hidden', !state.right); }
function mentionFile() {
  const tab = activeTab();
  if (tab?.type !== 'note') return;
  if (tab.mode === 'reading') setMode('live');
  if (tab.editor) tab.editor.mention();
  else tab.surface?.mention?.();
}

function toggleReading() { const tab = activeTab(); setMode(tab?.mode === 'reading' ? (tab.editMode || 'live') : 'reading'); }
function setMode(mode) { const tab = activeTab(); if (tab?.type !== 'note' || isMarkdown(tab.path)) return; if (mode === 'reading') tab.editMode = tab.mode; tab.mode = mode; renderWorkspace(); status(); persist(); }
function split(horizontal = false) { const tab = activeTab(), current = activeGroup(); current.size = (current.size || 1) / 2; const group = { id: uid(), size: current.size, tabs: [], active: null }; state.groups.push(group); state.horizontal = horizontal; if (tab?.path) run(openNote(tab.path, { group, mode: tab.mode })); else { state.activeGroup = group.id; newTab(group); } }
async function navigate(direction) { const index = state.historyIndex + direction; if (index < 0 || index >= state.history.length) return; state.historyIndex = index; await openNote(state.history[index], { noHistory: true }); }

function closeOverlay() { modalCleanup?.(); modalCleanup = null; $('#overlays').innerHTML = ''; }
function dialog(heading, content, footer = '') {
  closeOverlay(); const previous = document.activeElement;
  $('#overlays').innerHTML = `<div class="modal-backdrop"><section class="modal" role="dialog" aria-modal="true" aria-label="${escape(heading)}"><header class="modal-header"><h2>${escape(heading)}</h2>${button('X', 'Cerrar', 'close-overlay')}</header><div class="modal-content">${content}</div>${footer ? `<footer class="modal-footer">${footer}</footer>` : ''}</section></div>`;
  $('.modal-backdrop').onclick = event => { if (event.target.classList.contains('modal-backdrop')) closeOverlay(); };
  const focusable = () => $$('input,button,select,textarea,a[href]', $('.modal'));
  const trap = event => { if (event.key !== 'Tab') return; const elements = focusable(), first = elements[0], last = elements.at(-1); if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); } else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); } };
  document.addEventListener('keydown', trap); modalCleanup = () => { document.removeEventListener('keydown', trap); if (previous?.isConnected) previous.focus(); };
  queueMicrotask(() => ($('.modal input') || $('.modal button'))?.focus()); return $('.modal');
}
function promptName(heading, initial, placeholder, callback) {
  dialog(heading, `<form id="nameForm"><label class="field-label" for="nameInput">${escape(placeholder)}</label><input id="nameInput" class="text-input" value="${escape(initial)}" autocomplete="off" required><p class="field-error" id="nameError"></p><div class="form-actions"><button type="button" class="secondary-button" data-action="close-overlay">Cancelar</button><button class="primary-button" type="submit">Aceptar</button></div></form>`);
  $('#nameForm').onsubmit = async event => { event.preventDefault(); const input = $('#nameInput'), value = input.value.trim(); if (!value) return; try { await callback(value); closeOverlay(); } catch (error) { $('#nameError').textContent = error.message; } }; queueMicrotask(() => $('#nameInput')?.select());
}
function validPath(path) { return path && !path.split('/').some(p => !p || p === '.' || p === '..' || /[<>:"\\|?*\x00-\x1f]/.test(p) || p.startsWith('.')); }
async function createNote(name, folder = '', canvas = false) {
  if (!name) return promptName(canvas ? 'Crear lienzo' : 'Crear nota', canvas ? 'Sin título.canvas' : 'Sin título', 'Nombre (puedes incluir una carpeta)', value => createNote(value, folder, canvas));
  const extension = canvas ? '.canvas' : '.md'; let path = folder + name; if (!path.toLowerCase().endsWith(extension)) path += extension;
  if (!validPath(path)) throw new Error('Usa un nombre de archivo válido.');
  if (state.files.some(f => f.path === path)) { await openNote(path); return; }
  await post('/api/file', { path, content: canvas ? JSON.stringify({ nodes: [], edges: [] }) : '' }); await refreshFiles(); path.split('/').slice(0, -1).reduce((parent, part) => { const folder = parent ? parent + '/' + part : part; state.folders.add(folder); return folder; }, ''); await openNote(path);
}
async function renamePath(from, to) {
  if (from === to) return;
  if (!validPath(to)) throw new Error('El nombre contiene caracteres no válidos.');
  if (state.files.some(f => f.path === to)) throw new Error('Ya existe un archivo con ese nombre.');
  const oldPaths = state.files.filter(file => !file.is_dir).map(file => file.path);
  await Promise.all(oldPaths.filter(path => /\.(md|base)$/.test(path)).map(loadDocument));
  const affected = [...state.documents.values()].filter(d => d.path === from || d.path.startsWith(from + '/')); await Promise.all([...state.documents.values()].filter(d => d.dirty).map(saveDocument));
  await post('/api/rename', { from, to });
  for (const doc of affected) { const old = doc.path; clearTimeout(state.timers.get(old)); state.documents.delete(old); doc.path = to + old.slice(from.length); state.documents.set(doc.path, doc); setDraft(old, null); }
  for (const group of state.groups) for (const tab of group.tabs) if (tab.path === from || tab.path?.startsWith(from + '/')) tab.path = to + tab.path.slice(from.length);
  state.bookmarks = state.bookmarks.map(path => path === from || path.startsWith(from + '/') ? to + path.slice(from.length) : path);
  state.folders = new Set([...state.folders].map(path => remapPath(path, from, to)));
  state.history = state.history.map(path => remapPath(path, from, to));
  safeStorage.set("sc-recent-files", recentFiles().map(path => remapPath(path, from, to)));
  if (homeCollection()) homeCollection(remapPath(homeCollection(), from, to));
  const positions = safeStorage.get("sc-graph-positions", {});
  safeStorage.set("sc-graph-positions", Object.fromEntries(Object.entries(positions).map(([path, point]) => [remapPath(path, from, to), point])));
  // The backend updates wikilinks in notes on rename. Reload clean cached documents.
  for (const [path, doc] of state.documents) if (!doc.dirty && path.endsWith('.md')) {
    const fresh = await api('/api/file?path=' + encodeURIComponent(path)); doc.content = doc.saved = fresh.content.replace(/\r\n?/g, '\n');
    const updated = rewriteKnowledgeReferences(doc.content, from, to, oldPaths);
    if (updated !== doc.content) { changed(doc, updated); await saveDocument(doc); }
  }
  for (const [path, doc] of state.documents) if (path.endsWith('.base')) {
    let base; try { base = JSON.parse(doc.content); } catch { continue; } if (!base || typeof base !== 'object') continue;
    const before = JSON.stringify(base); base.folder = remapPath(base.folder, from, to); base.parent = remapPath(base.parent, from, to);
    if (path === to && from.endsWith('.base') && base.name === title(from)) base.name = title(to);
    if (Array.isArray(base.templates)) for (const template of base.templates) {
      if (typeof template.body === 'string') template.body = rewriteKnowledgeReferences(template.body, from, to, oldPaths);
      if (template.properties && typeof template.properties === 'object') for (const key of Object.keys(template.properties)) if (typeof template.properties[key] === 'string') template.properties[key] = rewriteKnowledgeReferences(template.properties[key], from, to, oldPaths);
    }
    if (JSON.stringify(base) !== before) { changed(doc, JSON.stringify(base, null, 2)); await saveDocument(doc); }
  }
  await refreshFiles(); renderWorkspace(); renderRight(); persist();
}
function deletePath(path, directory = false) {
  dialog(directory ? 'Eliminar carpeta' : 'Eliminar nota', `<p>¿Eliminar <strong>${escape(path)}</strong>${directory ? ' y todo su contenido' : ''} de la bóveda?</p><p class="muted">Esta acción no se puede deshacer.</p><div class="form-actions"><button class="secondary-button" data-action="close-overlay">Cancelar</button><button class="danger-button" id="confirmDelete">Eliminar</button></div>`);
  $('#confirmDelete').onclick = () => run((async () => { const affected = state.files.filter(f => f.path === path || f.path.startsWith(path + '/')).map(f => f.path); for (const p of affected) { clearTimeout(state.timers.get(p)); await state.saves.get(p)?.catch(() => {}); }
    await api(`/api/${directory ? 'folder' : 'file'}?path=${encodeURIComponent(path)}`, { method: 'DELETE' }); for (const p of affected) { state.documents.delete(p); setDraft(p, null); } for (const group of state.groups) { group.tabs = group.tabs.filter(t => !affected.includes(t.path)); if (!group.tabs.some(t => t.id === group.active)) group.active = group.tabs[0]?.id; } state.bookmarks = state.bookmarks.filter(p => !affected.includes(p)); closeOverlay(); await refreshFiles(); renderWorkspace(); renderRight(); persist(); })());
}
function menu(event, items) {
  closeOverlay(); const root = $('#overlays'), el = document.createElement('div'); el.className = 'context-menu'; el.setAttribute('role', 'menu');
  for (const item of items) { if (!item) { const hr = document.createElement('hr'); el.append(hr); continue; } const row = document.createElement('button'); row.className = item.danger ? 'danger' : ''; row.setAttribute('role', 'menuitem'); row.innerHTML = `${icon(item.icon || 'FileText', 16)}<span>${escape(item.label)}</span>${item.shortcut ? `<kbd>${escape(item.shortcut)}</kbd>` : ''}`; row.onclick = () => { closeOverlay(); try { run(item.action()); } catch (e) { report(e); } }; el.append(row); }
  el.onkeydown = event => { if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return; event.preventDefault(); const buttons = $$('button', el), index = buttons.indexOf(document.activeElement); const next = event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length; buttons[next]?.focus(); };
  root.append(el); const rect = event.currentTarget?.getBoundingClientRect?.() || { left: event.clientX, bottom: event.clientY }; el.style.left = `${Math.max(8, Math.min(event.clientX || rect.left, innerWidth - el.offsetWidth - 8))}px`; el.style.top = `${Math.min(event.clientY || rect.bottom, innerHeight - el.offsetHeight - 8)}px`;
  const dismiss = e => { if (!el.contains(e.target)) closeOverlay(); }; setTimeout(() => document.addEventListener('pointerdown', dismiss), 0); modalCleanup = () => document.removeEventListener('pointerdown', dismiss); el.querySelector('button')?.focus();
}
function fileMenu(event, path, directory) { const folder = directory ? path + '/' : path.includes('/') ? path.slice(0, path.lastIndexOf('/') + 1) : ''; menu(event, [directory ? { icon: 'FilePlus2', label: 'Nueva nota aquí', action: () => createNote(null, folder) } : { icon: 'ExternalLink', label: 'Abrir en nueva división', action: async () => { split(); await openNote(path); } }, !directory ? { icon: 'Bookmark', label: state.bookmarks.includes(path) ? 'Quitar marcador' : 'Añadir marcador', action: () => toggleBookmark(path) } : null, { icon: 'Pencil', label: 'Renombrar o mover', action: () => promptName('Renombrar o mover', path, 'Ruta en la bóveda', to => renamePath(path, to)) }, null, { icon: 'Trash2', label: 'Eliminar', danger: true, action: () => deletePath(path, directory) }]); }
function toggleBookmark(path = activeTab()?.path) { if (!path) return; state.bookmarks.includes(path) ? state.bookmarks.splice(state.bookmarks.indexOf(path), 1) : state.bookmarks.push(path); renderLeft(); persist(); }
function noteMenu(event) { const tab = activeTab(); menu(event, [
  ...(tab?.type === 'note' && !isMarkdown(tab.path) ? ['live', 'source', 'reading'].map(mode => ({ icon: mode === 'reading' ? 'BookOpen' : mode === 'source' ? 'CodeXml' : 'Pencil', label: `${mode === tab.mode ? '✓ ' : ''}${modeLabel(mode)}`, action: () => setMode(mode) })) : []), null,
  { icon: 'Columns2', label: 'Dividir a la derecha', action: () => split(false) }, { icon: 'Rows2', label: 'Dividir debajo', action: () => split(true) },
  ...(tab?.type === 'note' ? [{ icon: 'Files', label: 'Mover página a…', action: () => notion.movePage(activeDoc()) }] : []),
  ...(tab?.surface?.find ? [{icon:'CalendarDays',label:'Programar en el calendario',action:()=>notion.scheduleNote(activeDoc())}] : []),
  ...(tab?.path ? [null, { icon: 'Bookmark', label: state.bookmarks.includes(tab.path) ? 'Quitar marcador' : 'Añadir marcador', action: () => toggleBookmark() }, { icon: 'Pencil', label: 'Renombrar archivo', action: () => promptName('Renombrar archivo', tab.path, 'Ruta en la bóveda', to => renamePath(tab.path, to)) }, { icon: 'Download', label: 'Exportar archivo', action: exportNote }, { icon: 'Trash2', label: 'Eliminar archivo', danger: true, action: () => deletePath(tab.path) }] : []),
]); }
function exportNote() { const doc = activeDoc(); if (!doc) return; const url = URL.createObjectURL(new Blob([doc.content], { type: 'text/plain;charset=utf-8' })), a = document.createElement('a'); a.href = url; a.download = doc.path.split('/').pop(); a.click(); setTimeout(() => URL.revokeObjectURL(url), 1000); }

function recentFiles() { return safeStorage.get("sc-recent-files", []); }
function homeCollection(path) { if (path !== undefined) safeStorage.set("sc-home-collection", path); return safeStorage.get("sc-home-collection", ""); }
function openHome() { state.leftView = "files"; const group = activeGroup(); let tab = group.tabs.find(t => t.type === 'home'); if (!tab) { tab = { id: uid(), type: 'home' }; group.tabs.push(tab); } group.active = tab.id; renderWorkspace(); renderLeft(); renderRight(); status(); persist(); }
let shortcutLock = false;
async function enableFullScreenShortcuts() {
  if (!navigator.keyboard?.lock || !document.documentElement.requestFullscreen) {
    notify('Este navegador admite Ctrl+Alt+N como alternativa.', true);
    return;
  }
  try {
    await document.documentElement.requestFullscreen();
    await navigator.keyboard.lock(['KeyN', 'KeyO', 'KeyP', 'KeyK', 'KeyT', 'KeyW']);
    shortcutLock = Boolean(document.fullscreenElement);
    notify('Atajos de pantalla completa activos. Sal con Escape.');
  } catch {
    if (document.fullscreenElement) await document.exitFullscreen();
    notify('El navegador reservó los atajos. Usa Ctrl+Alt+N para una nota nueva.', true);
  }
}
document.addEventListener('fullscreenchange', () => {
  if (!document.fullscreenElement && shortcutLock) {
    shortcutLock = false;
    navigator.keyboard.unlock();
  }
});

const commands = () => [
  { label: 'Abrir chat con IA', icon: 'MessageSquare', action: () => chat.open() },
  ...(activeDoc() ? [{ label: 'Preguntar sobre el archivo actual', icon: 'MessageSquare', action: chatWithFile }] : []),
  { label: 'Activar atajos en pantalla completa', icon: 'Focus', action: enableFullScreenShortcuts },
  { label: 'Volver a Mis archivos', icon: 'ArrowUpRight', action: () => location.assign('/') },
  { label: 'Abrir Inicio', icon: 'Gem', action: openHome },
  { label: 'Abrir calendario', icon: 'CalendarDays', action: () => openCalendar() },
  ...(activeTab()?.surface?.find ? [{label:'Programar esta nota',icon:'CalendarDays',action:()=>notion.scheduleNote(activeDoc())}] : []),
  { label: 'Página: crear con bloques', icon: 'FilePlus2', action: () => createNote() },
  { label: 'Colección: crear base de datos', icon: 'Columns2', action: () => notion.createDatabase() },
  { label: 'Plantillas: abrir biblioteca', icon: 'BookOpen', action: () => notion.templates() },
  { label: 'Crear nueva nota', icon: 'FilePlus2', shortcut: 'Ctrl Alt N', action: () => createNote() },
  { label: 'Abrir nota: selector rápido', icon: 'Files', shortcut: 'Ctrl Alt O', action: quickSwitcher },
  { label: 'Guardar archivo actual', icon: 'Save', shortcut: 'Ctrl S', action: () => activeDoc() && saveDocument(activeDoc()) },
  { label: 'Abrir nota de hoy', icon: 'CalendarDays', action: dailyNote },
  { label: 'Vista de grafo: abrir', icon: 'Network', shortcut: 'Ctrl Alt G', action: openGraph },
  { label: 'Lienzo: crear nuevo', icon: 'LayoutDashboard', action: () => createNote(null, '', true) },
  { label: 'Buscar en todos los archivos', icon: 'Search', shortcut: 'Ctrl Alt F', action: () => showLeft('search') },
  { label: 'Ayuda y atajos', icon: 'CircleHelp', action: () => settings('hotkeys') },
  { label: 'Alternar vista de lectura de texto plano', icon: 'BookOpen', shortcut: 'Ctrl Alt E', action: toggleReading },
  { label: 'Alternar vista en vivo y modo fuente', icon: 'CodeXml', action: () => setMode(activeTab()?.mode === 'source' ? 'live' : 'source') },
  { label: 'Dividir a la derecha', icon: 'Columns2', action: () => split(false) }, { label: 'Dividir debajo', icon: 'Rows2', action: () => split(true) },
  { label: 'Mostrar u ocultar panel izquierdo', icon: 'PanelLeft', action: () => toggleSidebar('left') }, { label: 'Mostrar u ocultar panel derecho', icon: 'PanelRight', action: () => toggleSidebar('right') },
  { label: 'Marcadores: alternar nota actual', icon: 'Bookmark', action: () => toggleBookmark() },
  { label: 'Importar archivos Markdown', icon: 'Upload', action: () => $('#importFiles').click() }, { label: 'Exportar nota actual', icon: 'Download', action: exportNote },
  ...(activeTab()?.surface?.insert ? [{ label: 'Insertar bloque en la nota', icon: 'Plus', action: () => activeTab()?.surface?.insert() }] : []),
  ...(activeTab()?.editor ? [{ label: 'Formato: negrita', icon: 'Bold', shortcut: 'Ctrl B', action: () => activeTab()?.editor?.insert('**') }, { label: 'Formato: cursiva', icon: 'Italic', shortcut: 'Ctrl I', action: () => activeTab()?.editor?.insert('*') }] : []),
  { label: 'Mencionar archivo', icon: 'AtSign', shortcut: 'Ctrl Alt M', action: mentionFile },
  ...(activeTab()?.editor || activeTab()?.surface?.link ? [{ label: 'Insertar enlace interno', icon: 'Link', shortcut: 'Ctrl Alt K', action: insertInternalLink }] : []),
  { label: 'Abrir ajustes', icon: 'Settings', shortcut: 'Ctrl Alt ,', action: settings },
  { label: 'Cambiar entre tema claro y oscuro', icon: 'SunMoon', shortcut:'Ctrl Mayús L', action: toggleTheme },
  { label: 'Abrir Super Cerebro en otra ventana', icon: 'ExternalLink', shortcut:'Ctrl Alt Mayús N', action: openAnotherWindow },
];
function toggleTheme() { prefs.theme = prefs.theme === 'dark' ? 'light' : 'dark'; applyPrefs(); renderWorkspace(); }
function openAnotherWindow() { window.open('/cerebro','_blank','noopener,popup'); }
function chooser(heading, placeholder, getItems, create) {
  dialog(heading, `<div class="chooser-search">${icon('Search',18)}<input class="chooser-input" id="chooserInput" aria-label="${escape(placeholder)}" placeholder="${escape(placeholder)}" autocomplete="off"></div><div class="chooser-list" id="chooserList" role="listbox"></div>`, `<span><kbd>↑</kbd><kbd>↓</kbd> navegar</span><span><kbd>↵</kbd> abrir</span><span><kbd>esc</kbd> cerrar</span>`);
  $('.modal').classList.add('chooser'); let selected = 0, list = [];
  const update = () => { const query = $('#chooserInput').value; list = getItems(query); if (!list.length && create && query.trim()) list = [{ label: `Crear «${query.trim()}»`, icon: 'FilePlus2', action: () => createNote(query.trim()) }]; selected = 0; draw(); };
  const execute = index => { const item = list[index]; if (!item) return; closeOverlay(); run(item.action()); };
  const draw = () => { $('#chooserList').innerHTML = list.length ? list.map((item, index) => `<button class="chooser-item ${index === selected ? 'selected' : ''}" role="option" aria-selected="${index === selected}" data-index="${index}">${icon(item.icon || 'FileText', 16)}<span>${escape(item.label)}${item.detail ? `<small>${escape(item.detail)}</small>` : ''}</span>${item.shortcut ? `<kbd>${escape(item.shortcut)}</kbd>` : ''}</button>`).join('') : '<p class="empty-hint">No se han encontrado resultados.</p>'; $$('.chooser-item').forEach(el => el.onclick = () => execute(Number(el.dataset.index))); $$('.chooser-item')[selected]?.scrollIntoView({ block: 'nearest' }); };
  $('#chooserInput').oninput = update; $('#chooserInput').onkeydown = event => { if (['ArrowDown', 'ArrowUp'].includes(event.key)) { event.preventDefault(); selected = (selected + (event.key === 'ArrowDown' ? 1 : -1) + list.length) % Math.max(list.length, 1); draw(); } if (event.key === 'Enter') { event.preventDefault(); execute(selected); } }; update();
}
function insertInternalLink() {
  const tab=activeTab();
  if (tab?.editor) tab.editor.insert('[[', ']]');
  else tab?.surface?.link?.();
}
function quickSwitcher() { chooser('Abrir nota', 'Escribe el nombre de una nota…', query => state.files.filter(f => !f.is_dir && f.path.toLocaleLowerCase().includes(query.toLocaleLowerCase())).map(f => ({ label: title(f.path), detail: f.path.includes('/') ? f.path : '', icon: f.path.endsWith('.canvas') ? 'LayoutDashboard' : 'FileText', action: () => openNote(f.path) })), true); }
function commandPalette() { chooser('Paleta de comandos', 'Escribe un comando…', query => commands().filter(c => c.label.toLocaleLowerCase().normalize('NFD').replace(/\p{Diacritic}/gu, '').includes(query.toLocaleLowerCase().normalize('NFD').replace(/\p{Diacritic}/gu, '')))); }
function showLeft(view) { if (view === 'bookmarks') { $('#favoriteSection').open = true; view = 'files'; } state.left = true; state.leftView = view; updateSidebars(); renderLeft(); if (view === 'search') $('#vaultSearch')?.focus(); persist(); }
function dailyNote(date = localDate()) { return createNote(date); }
function openCalendar(date) {
  const group=activeGroup();
  let tab=group.tabs.find(item=>item.type==='calendar');
  if(!tab){tab={id:uid(),type:'calendar'};group.tabs.push(tab);}
  if(typeof date==='string' && /^\d{4}-\d{2}-\d{2}$/.test(date) && Number.isFinite(Date.parse(date)))tab.date=date;
  group.active=tab.id;renderWorkspace();renderLeft();renderRight();status();persist();
}
async function checkReminders() {
  const reminders=await notion.dueReminders(), shown=safeStorage.get('sc-shown-reminders',[]);
  const key=item=>item.id+':'+item.when;
  const pending=reminders.filter(item=>!shown.includes(key(item)));
  if(!pending.length)return;
  safeStorage.set('sc-shown-reminders',[...shown,...pending.map(key)].slice(-500));
  notify('Recordatorio: '+pending.map(item=>item.label).join(' · '));
}

function settings(page = 'appearance') {
  const sections = [['appearance','Apariencia'],['editor','Editor'],['files','Archivos y enlaces'],['hotkeys','Atajos de teclado'],['about','Acerca de']];
  dialog('Ajustes', `<div class="settings-layout"><nav class="settings-nav"><span>OPCIONES</span>${sections.map(([key, label]) => `<button data-settings-page="${key}" class="${page === key ? 'active' : ''}">${label}</button>`).join('')}</nav><div class="settings-page" id="settingsPage"></div></div>`); $('.modal').classList.add('settings-modal');
  $$('[data-settings-page]').forEach(el => el.onclick = () => settings(el.dataset.settingsPage));
  const root = $('#settingsPage');
  const row = (heading, detail, control) => `<div class="setting-row"><div><strong>${heading}</strong><p>${detail}</p></div>${control}</div>`;
  if (page === 'appearance') root.innerHTML = `<h2>Apariencia</h2>${row('Tema base', 'Elige el aspecto de tu espacio de trabajo.', `<select id="themeSelect"><option value="dark" ${prefs.theme === 'dark' ? 'selected' : ''}>Oscuro</option><option value="light" ${prefs.theme === 'light' ? 'selected' : ''}>Claro</option></select>`)}${row('Tamaño de fuente', 'Tamaño del texto del editor, en píxeles.', `<input id="fontSize" type="number" min="12" max="28" value="${prefs.fontSize}">`)}${row('Ancho de línea legible', 'Limita el ancho del contenido para una lectura cómoda.', `<input id="readableToggle" class="toggle" type="checkbox" ${prefs.readable ? 'checked' : ''}>`)}`;
  if (page === 'editor') root.innerHTML = `<h2>Editor</h2>${row('Modo predeterminado', 'Modo de edición para archivos de texto plano.', `<select id="defaultMode"><option value="live" ${prefs.defaultMode === 'live' ? 'selected' : ''}>Vista en vivo</option><option value="source" ${prefs.defaultMode === 'source' ? 'selected' : ''}>Modo fuente</option><option value="reading" ${prefs.defaultMode === 'reading' ? 'selected' : ''}>Lectura</option></select>`)}${row('Corrección ortográfica', 'Usa el corrector del navegador en el editor.', `<input id="spellcheckToggle" class="toggle" type="checkbox" ${prefs.spellcheck ? 'checked' : ''}>`)}<p class="settings-note">Los cambios se guardan automáticamente. El historial de deshacer se mantiene al cambiar de pestaña o de modo de edición.</p>`;
  if (page === 'files') root.innerHTML = `<h2>Archivos y enlaces</h2>${row('Ubicación de la bóveda', 'Archivos Markdown locales. No se necesita una base de datos.', '<code>app/vault/</code>')}${row('Importar archivos', 'Añade archivos Markdown o lienzos a la bóveda.', '<button class="secondary-button" data-action="import">Importar</button>')}${row('Formato de enlaces internos', 'Enlaza notas por su nombre o ruta; admite alias y encabezados.', '<code>[[nota|alias]]</code>')}<p class="settings-note">Al renombrar un archivo, los enlaces internos se actualizan automáticamente. Los archivos existentes no se sobrescriben al importar.</p>`;
  if (page === 'hotkeys') root.innerHTML = '<h2>Atajos de teclado</h2>' + commands().filter(c => c.shortcut).map(c => row(escape(c.label), '', `<kbd>${escape(c.shortcut)}</kbd>`)).join('') + row('Cerrar pestaña', '', '<kbd>Ctrl Alt W</kbd>') + row('Nueva pestaña', '', '<kbd>Ctrl Alt T</kbd>') + row('Cambiar de pestaña', '', '<kbd>Ctrl Alt RePág / AvPág</kbd>') + row('Navegar por el historial', '', '<kbd>Ctrl Alt J / L</kbd>') + row('Buscar en la nota', '', '<kbd>Ctrl F</kbd>') + row('Saltar a una página', 'Ctrl+Alt+O funciona también si el navegador reserva Ctrl+P o Ctrl+K.', '<kbd>Ctrl P / K</kbd>') + row('Mostrar u ocultar la barra lateral', '', '<kbd>Ctrl Alt B</kbd>') + row('Seleccionar el bloque actual', 'Esc otra vez lo elimina. Deshaz desde la barra de la nota.', '<kbd>Esc</kbd>');
  if (page === 'about') root.innerHTML = `<div class="about-mark">${icon('Gem', 48)}</div><h2>Super Cerebro</h2><p>Un espacio de notas local, inspirado en Obsidian.</p><p class="muted">Rust · Markdown · CodeMirror</p><p class="settings-note">Esta aplicación es independiente de Obsidian. Incluye edición Markdown, enlaces, grafo y lienzos. No incluye Obsidian Sync, Publish ni compatibilidad con plugins de Obsidian.</p>`;
  const controlLabels = { '#themeSelect': 'Tema base', '#fontSize': 'Tamaño de fuente', '#readableToggle': 'Ancho de línea legible', '#spellcheckToggle': 'Corrección ortográfica', '#defaultMode': 'Modo predeterminado' };
  const bind = (selector, callback) => { const el = $(selector); if (el) { el.setAttribute('aria-label', controlLabels[selector]); el.onchange = () => { callback(el); applyPrefs(); renderWorkspace(); }; } };
  bind('#themeSelect', el => prefs.theme = el.value); bind('#fontSize', el => prefs.fontSize = Math.max(12, Math.min(28, Number(el.value) || 16))); bind('#readableToggle', el => prefs.readable = el.checked); bind('#spellcheckToggle', el => prefs.spellcheck = el.checked); bind('#defaultMode', el => prefs.defaultMode = el.value);
}

function openGraph() { let tab = activeGroup().tabs.find(t => t.type === 'graph'); if (!tab) { tab = { id: uid(), type: 'graph' }; activeGroup().tabs.push(tab); } activeGroup().active = tab.id; renderWorkspace(); renderLeft(); renderRight(); status(); persist(); }
async function mountGraph(container, tab) {
  container.innerHTML = `<div class="graph-view"><canvas aria-label="Grafo de conexiones entre notas" tabindex="0"></canvas><div class="graph-controls"><button class="graph-mode" data-action="graph-mode" aria-label="Vista 3D" aria-pressed="true">3D</button>${button('Plus', 'Acercar grafo', 'graph-zoom-in')}${button('Minus', 'Alejar grafo', 'graph-zoom-out')}${button('Focus', 'Centrar grafo', 'graph-reset')}${button('Link', 'Conectar notas', 'graph-connect')}</div><div class="graph-caption" role="status"></div></div>`;
  const root = $('.graph-view', container), canvas = $('canvas', root), ctx = canvas.getContext('2d'); let alive = true; graphCleanup.push(() => alive = false);
  const data = await api('/api/graph'); if (!alive) return;
  const nodes = data.nodes.filter(n => !n.path.startsWith('.'));
  const nodeMap = new Map(nodes.map((node, index) => {
    const angle = index * Math.PI * (3 - Math.sqrt(5)), radius = Math.sqrt(index) * 52;
    return [node.id, { ...node, x: Math.cos(angle) * radius, y: Math.sin(angle) * radius, z: Math.sin(index * 1.7) * radius * .65 }];
  }));
  const edges = data.edges.filter(edge => nodeMap.has(edge.source) && nodeMap.has(edge.target)); let zoom = 1, offsetX = 0, offsetY = 0, hovered = null, frame = 0, drag = null, width = 0, height = 0;
  let mode3d = safeStorage.get('sc-graph-3d', true), yaw = .3, pitch = -.2, projected = [];
  let connecting = false, source = null, busy = false;
  const colors = () => { const css = getComputedStyle(document.documentElement); return { text: css.getPropertyValue('--text-muted'), accent: css.getPropertyValue('--accent'), line: css.getPropertyValue('--graph-line'), node: css.getPropertyValue('--graph-node') }; };
  function camera() {
    const cy = Math.cos(yaw), sy = Math.sin(yaw), cp = Math.cos(pitch), sp = Math.sin(pitch);
    return { cy, sy, cp, sp };
  }
  function draw() {
    if (!alive) return;
    const dpr = devicePixelRatio || 1;
    width = root.clientWidth; height = root.clientHeight;
    if (canvas.width !== Math.round(width*dpr)) canvas.width = Math.round(width*dpr);
    if (canvas.height !== Math.round(height*dpr)) canvas.height = Math.round(height*dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);
    const color = colors(), {cy,sy,cp,sp}=camera(), distance=Math.max(420, Math.sqrt(nodes.length)*96);
    projected = all.map(node => {
      const x=mode3d?node.x*cy+node.z*sy:node.x;
      const z=mode3d?node.z*cy-node.x*sy:0;
      const y=mode3d?node.y*cp-z*sp:node.y, depth=mode3d?node.y*sp+z*cp:0;
      const scale=zoom*distance/Math.max(distance*.32,distance-depth);
      return {node, x:width/2+offsetX+x*scale, y:height/2+offsetY+y*scale, depth, scale};
    }).sort((a,b)=>a.depth-b.depth);
    const points=new Map(projected.map(point=>[point.node.id,point]));
    const neighbors = new Set(edges.filter(e => e.source === hovered?.id || e.target === hovered?.id).flatMap(e => [e.source, e.target]));
    for (const edge of edges) { const a = points.get(edge.source), b = points.get(edge.target); ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.lineWidth = 1; ctx.strokeStyle = hovered && (edge.source === hovered.id || edge.target === hovered.id) ? color.accent : color.line; ctx.stroke(); }
    for (const {node,x,y,scale} of projected) {
      const focus=node===source||node===hovered||neighbors.has(node.id), radius=Math.max(2.5,Math.min(13,(focus?6:4.5)*scale));
      ctx.globalAlpha=focus?1:Math.max(.48,Math.min(1,scale/zoom));
      ctx.beginPath();ctx.arc(x,y,radius,0,Math.PI*2);ctx.fillStyle=focus?color.accent:color.node;ctx.fill();
      ctx.font='12px Segoe UI';ctx.textAlign='center';ctx.fillStyle=focus?color.accent:color.text;ctx.fillText(node.title,x,y+radius+13);
    }
    ctx.globalAlpha=1;
    if (!nodes.length) { ctx.fillStyle = color.text; ctx.textAlign = 'center'; ctx.font = '14px Segoe UI'; ctx.fillText('Crea notas y conéctalas con [[enlaces internos]].', width / 2, height / 2); }
  }
  // Settle a small force-directed layout, without an endless render loop.
  const all = [...nodeMap.values()];
  for (let tick=0;tick<90 && all.length<=150;tick++) {
    for (let i=0;i<all.length;i++) for (let j=i+1;j<all.length;j++) {
      const a=all[i],b=all[j],dx=a.x-b.x||.1,dy=a.y-b.y||.1,dz=a.z-b.z;
      const force=100/Math.max(100,dx*dx+dy*dy+dz*dz);
      a.x+=dx*force;a.y+=dy*force;a.z+=dz*force;
      b.x-=dx*force;b.y-=dy*force;b.z-=dz*force;
    }
    for (const edge of edges) {
      const a=nodeMap.get(edge.source),b=nodeMap.get(edge.target),dx=b.x-a.x,dy=b.y-a.y,dz=b.z-a.z;
      const length=Math.hypot(dx,dy,dz)||1, pull=(length-110)*.015/length;
      a.x+=dx*pull;a.y+=dy*pull;a.z+=dz*pull;
      b.x-=dx*pull;b.y-=dy*pull;b.z-=dz*pull;
    }
    all.forEach(node=>{node.x*=.997;node.y*=.997;node.z*=.997;});
  }
  const observer = new ResizeObserver(() => { cancelAnimationFrame(frame); frame = requestAnimationFrame(draw); }); observer.observe(root); graphCleanup.push(() => { observer.disconnect(); cancelAnimationFrame(frame); });
  const positionStore = 'sc-graph-positions';
  const positions = safeStorage.get(positionStore, {});
  for (const node of all) {
    const saved = positions[node.path];
    if (Number.isFinite(saved?.x) && Number.isFinite(saved?.y)) {
      node.x = saved.x;
      node.y = saved.y;
      if (Number.isFinite(saved.z)) node.z = saved.z;
    }
  }

  const caption = $('.graph-caption', root);
  const connectButton = $('[data-action=graph-connect]', root);
  const modeButton = $('[data-action=graph-mode]', root);
  const nodeAt = event => {
    const rect = canvas.getBoundingClientRect();
    for (let i=projected.length-1;i>=0;i--) {
      const p=projected[i];
      if (Math.hypot(p.x-(event.clientX-rect.left),p.y-(event.clientY-rect.top)) < Math.max(11,p.scale*5)) return p;
    }
  };
  function setConnecting(value, first = null) {
    connecting = value;
    source = first;
    connectButton.setAttribute('aria-pressed', String(value));
    modeButton.textContent=mode3d?'3D':'2D';
    modeButton.setAttribute('aria-pressed',String(mode3d));
    caption.textContent = !value ? (mode3d ? 'Arrastra el fondo para girar · Mayús + arrastrar para desplazar · Flechas para girar' : 'Arrastra el fondo para desplazar · Clic en una nota para abrirla')
      : first ? 'Elige la nota de destino' : 'Elige la nota de origen';
    draw();
  }

  async function connect(from, to) {
    if (busy || from === to) return;
    if (edges.some(edge => edge.source === from.id && edge.target === to.id)) {
      notify('Las notas ya tienen ese enlace.');
      setConnecting(false);
      return;
    }
    busy = true;
    try {
      const doc = await loadDocument(from.path);
      const link = /[#\[\]]/.test(to.path) ? fileMention(to.path) : '[[' + to.path + ']]';
      const separator = !doc.content || doc.content.endsWith('\n\n') ? '' : doc.content.endsWith('\n') ? '\n' : '\n\n';
      if (!doc.content.split('\n').includes(link)) changed(doc, doc.content + separator + link + '\n');
      await saveDocument(doc);
      edges.push({ source: from.id, target: to.id });
      notify('Enlace guardado en ' + from.title);
    } finally {
      busy = false;
      setConnecting(false);
    }
  }

  function pickNode(heading, choose, exclude) {
    chooser(heading, 'Buscar nota…', query => all
      .filter(node => node !== exclude && node.path.toLocaleLowerCase().includes(query.toLocaleLowerCase()))
      .map(node => ({ label: node.title, detail: node.path, action: () => choose(node) })));
  }
  canvas.onpointerdown = event => {
    if (event.button !== 0) return;
    const hit = nodeAt(event), node=hit?.node;
    drag = { x: event.clientX, y: event.clientY, node, scale:hit?.scale, ox: node?.x ?? offsetX, oy: node?.y ?? offsetY, oz:node?.z, yaw, pitch, orbit:mode3d&&!event.shiftKey, moved:false };
    canvas.setPointerCapture(event.pointerId);
  };
  canvas.onpointermove = event => {
    if (drag) {
      const dx = event.clientX - drag.x, dy = event.clientY - drag.y;
      drag.moved ||= Math.hypot(dx, dy) > 4;
      if (drag.moved && drag.node && !connecting) {
        const x=dx/drag.scale, y=dy/drag.scale;
        const {cy,sy,cp,sp}=camera();
        drag.node.x=drag.ox+(mode3d?x*cy+y*sp*sy:x);
        drag.node.y=drag.oy+(mode3d?y*cp:y);
        if (mode3d) drag.node.z=drag.oz+x*sy-y*sp*cy;
      } else if (drag.moved && !drag.node) {
        if (drag.orbit) { yaw=drag.yaw+dx*.008; pitch=drag.pitch+dy*.008; }
        else { offsetX = drag.ox + dx; offsetY = drag.oy + dy; }
      }
    } else {
      hovered = nodeAt(event)?.node;
      canvas.style.cursor = hovered && connecting ? 'crosshair' : hovered ? 'move' : 'grab';
    }
    draw();
  };
  canvas.onpointerup = () => {
    const interaction = drag;
    drag = null;
    if (!interaction?.node) return;
    const { node, moved } = interaction;
    if (moved && !connecting) {
      positions[node.path] = { x: node.x, y: node.y, z:node.z };
      safeStorage.set(positionStore, positions);
    } else if (!moved && connecting) {
      if (!source || source === node) setConnecting(true, node);
      else run(connect(source, node));
    } else if (!moved) run(openNote(node.path));
  };
  canvas.onpointercancel = () => {
    if (drag?.node && drag.moved && !connecting) {
      drag.node.x = drag.ox;
      drag.node.y = drag.oy;
      drag.node.z = drag.oz;
    }
    drag = null;
    draw();
  };
  canvas.onwheel = event => { event.preventDefault(); zoom = Math.min(4, Math.max(.2, zoom * (event.deltaY > 0 ? .9 : 1.1))); draw(); };
  canvas.onkeydown = event => {
    if (!mode3d || !['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(event.key)) return;
    event.preventDefault();
    if (event.key==='ArrowLeft'||event.key==='ArrowRight') yaw+=event.key==='ArrowLeft'?-.12:.12;
    else pitch+=event.key==='ArrowUp'?-.12:.12;
    draw();
  };
  root.onclick = event => {
    const action = event.target.closest('[data-action]')?.dataset.action;
    if (action === 'graph-connect') {
      if (event.detail === 0) pickNode('Nota de origen', from => pickNode('Nota de destino', to => connect(from, to), from));
      else setConnecting(!connecting);
      return;
    }
    if (action === 'graph-zoom-in') zoom = Math.min(4, zoom * 1.2);
    if (action === 'graph-zoom-out') zoom = Math.max(.2, zoom / 1.2);
    if (action === 'graph-reset') { zoom = 1; offsetX = offsetY = 0; yaw=.3;pitch=-.2; }
    if (action === 'graph-mode') { mode3d=!mode3d;safeStorage.set('sc-graph-3d',mode3d);setConnecting(false);return; }
    draw();
  };
  setConnecting(false);
}

function mountCanvas(container, tab, inline = false) {
  const doc=state.documents.get(tab.path);
  function read(content) {
    try {
      const parsed=JSON.parse(content);
      if (Array.isArray(parsed?.nodes)&&Array.isArray(parsed?.edges)) return parsed;
    } catch {}
    return null;
  }
  let data=read(doc.content);
  if (!data) {
    container.innerHTML='<p class="empty-hint">El archivo no contiene un lienzo válido.</p>';
    return;
  }
  container.innerHTML = `<div class="canvas-view"><div class="canvas-world"><svg class="canvas-edges"></svg><div class="canvas-cards"></div></div><div class="canvas-toolbar">${button('StickyNote', 'Añadir tarjeta', 'canvas-card')}${button('FilePlus2', 'Añadir tarjeta de archivo', 'canvas-file')}${button('Link', 'Conectar tarjetas', 'canvas-connect')}${button('Focus', 'Centrar lienzo', 'canvas-reset')}</div><div class="graph-caption">Arrastra las tarjetas · Doble clic en el fondo para crear · Arrastra el fondo para desplazar</div></div>`;
  const root = $('.canvas-view', container), world = $('.canvas-world', root), cards = $('.canvas-cards', root), svg = $('.canvas-edges', root); let connecting = false, startNode = null, pan = { x: 0, y: 0 }, zoom = 1;
  const save = () => changed(doc, JSON.stringify(data, null, 2), tab.id);
  function drawEdges() { svg.innerHTML = data.edges.map(edge => { const from = data.nodes.find(n => n.id === edge.fromNode), to = data.nodes.find(n => n.id === edge.toNode); if (!from || !to) return ''; const x1 = from.x + from.width, y1 = from.y + from.height / 2, x2 = to.x, y2 = to.y + to.height / 2; return `<path d="M${x1} ${y1} C${x1+80} ${y1} ${x2-80} ${y2} ${x2} ${y2}"/>`; }).join(''); }
  function draw() { cards.innerHTML = ''; for (const node of data.nodes) {
    const card = document.createElement('div'); card.className = 'canvas-card'; card.style.cssText = `left:${Number(node.x) || 0}px;top:${Number(node.y) || 0}px;width:${Number(node.width) || 260}px;height:${Number(node.height) || 170}px`; card.dataset.node = node.id;
    card.innerHTML = `<header>${icon(node.type === 'file' ? 'FileText' : 'Grip', 14)}<span>${node.type === 'file' ? escape(title(node.file || '')) : 'Tarjeta'}</span><button aria-label="Eliminar tarjeta">${icon('X', 13)}</button></header>${node.type === 'file' ? `<button class="canvas-file">${icon('FileText', 24)}${escape(node.file || '')}</button>` : '<textarea spellcheck="false" placeholder="Escribe una idea…" aria-label="Contenido de la tarjeta"></textarea>'}`;
    if (node.type !== 'file') { const textarea = $('textarea', card); textarea.value = node.text || ''; textarea.oninput = () => { node.text = textarea.value; save(); }; } else $('.canvas-file', card).onclick = () => run(openNote(node.file));
    $('header button', card).onclick = event => { event.stopPropagation(); data.nodes = data.nodes.filter(n => n.id !== node.id); data.edges = data.edges.filter(e => e.fromNode !== node.id && e.toNode !== node.id); save(); draw(); };
    card.onclick = () => { if (!connecting) return; if (!startNode) { startNode = node.id; card.classList.add('connecting'); } else if (startNode !== node.id) { data.edges.push({ id: uid(), fromNode: startNode, fromSide: 'right', toNode: node.id, toSide: 'left' }); startNode = null; connecting = false; save(); draw(); } };
    $('header', card).onpointerdown = event => { if (event.target.closest('button') || connecting) return; event.stopPropagation(); const x = event.clientX, y = event.clientY, nx = node.x, ny = node.y, handle = event.currentTarget; handle.setPointerCapture(event.pointerId); handle.onpointermove = move => { node.x = Math.round(nx + (move.clientX - x) / zoom); node.y = Math.round(ny + (move.clientY - y) / zoom); card.style.left = node.x + 'px'; card.style.top = node.y + 'px'; drawEdges(); }; handle.onpointerup = () => { handle.onpointermove = null; save(); }; };
    cards.append(card);
  } drawEdges(); }
  function add(type = 'text', file) { const number = data.nodes.length; data.nodes.push({ id: uid(), type, x: 80 + (number % 3) * 300, y: 80 + Math.floor(number / 3) * 220, width: 260, height: 170, ...(type === 'file' ? { file } : { text: '' }) }); save(); draw(); }
  const transform = () => world.style.transform = `translate(${pan.x}px,${pan.y}px) scale(${zoom})`;
  root.ondblclick = event => { if (!event.target.closest('.canvas-card,.canvas-toolbar')) add(); };
  root.onpointerdown = event => { if (event.target.closest('.canvas-card,.canvas-toolbar')) return; const x = event.clientX, y = event.clientY, ox = pan.x, oy = pan.y; root.setPointerCapture(event.pointerId); root.onpointermove = e => { pan.x = ox + e.clientX - x; pan.y = oy + e.clientY - y; transform(); }; root.onpointerup = () => root.onpointermove = null; };
  root.onwheel = event => { if (event.target.closest('textarea')) return; event.preventDefault(); zoom = Math.max(.25, Math.min(2, zoom * (event.deltaY > 0 ? .95 : 1.05))); transform(); };
  root.onclick = event => { const action = event.target.closest('[data-action]')?.dataset.action; if (action === 'canvas-card') add(); if (action === 'canvas-file') chooser('Añadir archivo al lienzo', 'Buscar nota…', query => state.files.filter(f => !f.is_dir && f.path.endsWith('.md') && f.path.includes(query)).map(f => ({ label: title(f.path), action: () => add('file', f.path) }))); if (action === 'canvas-connect') { connecting = !connecting; startNode = null; notify('Selecciona la tarjeta de origen y después la de destino.'); } if (action === 'canvas-reset') { pan = { x: 0, y: 0 }; zoom = 1; transform(); } }; draw();
  const refresh=content=>{const next=read(content);if(next){data=next;draw();}};
  const sync=event=>{if(event.detail.path===doc.path && event.detail.origin!==tab.id)refresh(doc.content);};
  if (inline) document.addEventListener('sc-document-changed',sync);
  return { refresh, destroy:()=>document.removeEventListener('sc-document-changed',sync) };
}

const actions = {
  chat: () => chat.open(), 'chat-with-file': chatWithFile,
  home: openHome, calendar: () => openCalendar(), 'new-page': () => createNote(), 'new-database': () => notion.createDatabase(), templates: () => notion.templates(),
  'toggle-left': () => toggleSidebar('left'), 'toggle-right': () => toggleSidebar('right'),
  'left-files': () => showLeft('files'), 'left-pages': () => showLeft('pages'), 'left-search': () => showLeft('search'), 'left-bookmarks': () => showLeft('bookmarks'), search: () => showLeft('search'),
  'mention-file': mentionFile,
  switcher: quickSwitcher, commands: commandPalette, graph: openGraph, canvas: () => createNote(null, '', true), daily: () => dailyNote(), settings, help: () => settings('hotkeys'),
  'new-note': () => createNote(), 'new-tab': () => newTab(), 'new-folder': () => promptName('Nueva carpeta', '', 'Nombre de la carpeta', async path => { if (!validPath(path)) throw new Error('Nombre no válido.'); if (state.files.some(f => f.path === path)) throw new Error('Ya existe un archivo o carpeta con ese nombre.'); await post('/api/folder', { path }); state.folders.add(path); await refreshFiles(); }),
  'toggle-reading': toggleReading, 'cycle-mode': () => setMode(activeTab()?.mode === 'live' ? 'source' : activeTab()?.mode === 'source' ? 'reading' : 'live'),
  'close-overlay': closeOverlay, back: () => navigate(-1), forward: () => navigate(1),
  'clear-search': () => { $('#vaultSearch').value = ''; run(searchVault()); $('#vaultSearch').focus(); },
  sort: () => { state.sort = state.sort === 'asc' ? 'desc' : 'asc'; renderLeft(); }, 'collapse-folders': () => { state.folders.clear(); renderLeft(); persist(); },
  import: () => $('#importFiles').click(),
  'add-property': () => promptName('Añadir propiedad', '', 'Nombre de la propiedad', key => { if (!/^[\p{L}\p{N}_-]+$/u.test(key)) throw new Error('Usa letras, números o guiones.'); const doc = activeDoc(); if (!doc) return; const content = /^---\r?\n/.test(doc.content) ? doc.content.replace(/^(---\r?\n)/, `$1${key}: \n`) : `---\n${key}: \n---\n\n${doc.content}`; changed(doc, content); renderWorkspace(); }),
};
document.addEventListener('click', event => {
  const dateLink=event.target.closest('a[href^="/cerebro?fecha="]');
  if(dateLink){event.preventDefault();openCalendar(new URL(dateLink.href).searchParams.get('fecha'));return;}
  const markdownLink = event.target.closest('.markdown-preview a[href]:not(.sc-reference)');
  if (markdownLink && !markdownLink.dataset.note && !markdownLink.dataset.tag) { const href = markdownLink.getAttribute('href'); if (href.startsWith('#') || /^[^:]+\.md(?:#.*)?$/.test(href)) { event.preventDefault(); run(followLink(decodeURIComponent(href))); return; } }
  const link = event.target.closest('[data-note]'); if (link && !link.closest('.cm-editor')) { event.preventDefault(); run(followLink(link.dataset.note)); return; }
  const tag = event.target.closest('[data-tag]'); if (tag) { event.preventDefault(); showLeft('search'); $('#vaultSearch').value = '#' + tag.dataset.tag; run(searchVault()); return; }
  const el = event.target.closest('[data-action]'); if (!el) return;
  const groupId = el.closest('[data-group]')?.dataset.group; if (groupId) state.activeGroup = groupId;
  const action = el.dataset.action;
  if (action.startsWith('right-')) { state.rightView = action.slice(6); run(renderRight()); persist(); }
  else if (action === 'note-menu') noteMenu({ currentTarget: el });
  else if (action === 'vault-menu') menu({ currentTarget: el }, [{ icon: 'Upload', label: 'Importar archivos', action: () => $('#importFiles').click() }, { icon: 'Settings', label: 'Ajustes de la bóveda', action: settings }]);
  else if (action === 'tab-list') chooser('Pestañas abiertas', 'Buscar pestaña…', query => state.groups.flatMap(g => g.tabs.map(t => ({ label: t.path ? title(t.path) : t.type === 'calendar' ? 'Calendario' : t.type === 'home' ? 'Inicio' : t.type === 'graph' ? 'Vista de grafo' : 'Nueva pestaña', action: () => { g.active = t.id; state.activeGroup = g.id; renderWorkspace(); renderRight(); status(); persist(); } }))).filter(t => t.label.toLocaleLowerCase().includes(query.toLocaleLowerCase())));
  else if (actions[action]) { try { run(actions[action]()); } catch (e) { report(e); } }
});
function switchTab(direction) {
  const group = activeGroup();
  if (!group.tabs.length) return;
  const current = group.tabs.findIndex(tab => tab.id === group.active);
  group.active = group.tabs[(current + direction + group.tabs.length) % group.tabs.length].id;
  renderWorkspace();
  renderRight();
  status();
  persist();
}

const browserCommands = {
  KeyK: insertInternalLink, KeyM: mentionFile, KeyP: commandPalette, KeyO: quickSwitcher, Comma: settings, KeyN: createNote,
  KeyG: openGraph, KeyT: newTab, KeyE: toggleReading, KeyF: () => showLeft('search'),
  KeyW: () => activeTab() && closeTab(activeGroup(), activeTab().id),
  KeyJ: () => navigate(-1), KeyL: () => navigate(1),
  PageUp: () => switchTab(-1), PageDown: () => switchTab(1),
  KeyB: () => toggleSidebar('left'),
};

document.addEventListener('keydown', event => {
  if (event.defaultPrevented || event.isComposing || document.querySelector('dialog[open]')) return;
  if (event.key === 'Escape') { closeOverlay(); return; }
  if (!(event.ctrlKey || event.metaKey) || event.getModifierState('AltGraph')) return;
  const editor = activeTab()?.editor;
  const code = event.code;
  let action;
  if (!event.altKey && !event.shiftKey) {
    if (shortcutLock && ['KeyO','KeyT','KeyW'].includes(code)) action = browserCommands[code];
    if (code==='KeyP'||code==='KeyK') action=quickSwitcher;
    if (code==='KeyN') action=()=>createNote();
    if (code==='KeyF'&&activeTab()?.surface?.find) action=()=>activeTab().surface.find();
    if (event.key==='\\'||code==='Backslash') action=()=>toggleSidebar('left');
    if (code === 'KeyS' && activeDoc()) action = () => saveDocument(activeDoc());
    if (code === 'KeyB' && editor) action = () => editor.insert('**');
    if (code === 'KeyI' && editor) action = () => editor.insert('*');
  } else if (event.altKey && !event.shiftKey) {
    action = browserCommands[code];
  } else if (event.shiftKey) {
    if (code==='KeyL') action=toggleTheme;
    if (code==='KeyN') action=openAnotherWindow;
  }
  if (!action || event.repeat) return;
  event.preventDefault();
  run(action());
});
window.addEventListener('beforeunload', event => { if ([...state.documents.values()].some(doc => doc.dirty)) { event.preventDefault(); event.returnValue = ''; } });
document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'hidden') for (const doc of state.documents.values()) if (doc.dirty) run(saveDocument(doc)); });
$('#importFiles').onchange = () => run((async () => { let count = 0; for (const file of $('#importFiles').files) { let path = file.name, index = 1; while (state.files.some(f => f.path === path)) { const dot = file.name.lastIndexOf('.'); path = file.name.slice(0, dot) + ` (${index++})` + file.name.slice(dot); } if (!validPath(path)) continue; await post('/api/file', { path, content: await file.text() }); state.files.push({ path, name: path, is_dir: false }); count++; } await refreshFiles(); notify(`${count} archivos importados`); $('#importFiles').value = ''; })());
for (const side of ['Left', 'Right']) {
  const handle = $('#resize' + side), sidebar = $('#' + side.toLowerCase() + 'Sidebar');
  handle.onpointerdown = event => { const x = event.clientX, width = sidebar.offsetWidth; handle.setPointerCapture(event.pointerId); document.body.classList.add('resizing'); handle.onpointermove = move => { sidebar.style.width = `${Math.max(180, Math.min(480, width + (move.clientX - x) * (side === 'Right' ? -1 : 1)))}px`; }; handle.onpointerup = () => { handle.onpointermove = null; document.body.classList.remove('resizing'); persist(); }; };
  handle.onkeydown = event => { if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') { event.preventDefault(); sidebar.style.width = `${Math.max(180, Math.min(480, sidebar.offsetWidth + (event.key === 'ArrowRight' ? 16 : -16) * (side === 'Right' ? -1 : 1)))}px`; persist(); } };
}
async function init() {
  applyPrefs(); await refreshFiles();
  const saved = await api('/api/workspace');
  state.leftView = ['files','pages','search'].includes(saved.leftView) ? saved.leftView : 'files';
  state.bookmarks = (saved.bookmarks || []).filter(path => state.files.some(f => f.path === path)); state.folders = new Set(saved.folders || []); state.left = saved.left ?? true; state.right = saved.right ?? false; state.rightView = saved.rightView || 'outline';
  if (saved.leftWidth) $('#leftSidebar').style.width = saved.leftWidth; if (saved.rightWidth) $('#rightSidebar').style.width = saved.rightWidth;
  if (saved.groups?.length) {
    state.horizontal = saved.horizontal === true;
    state.groups = saved.groups.map(g => ({ ...g, tabs: (g.tabs || []).filter(t => !t.path || state.files.some(f => f.path === t.path)).map(t => isMarkdown(t.path) && t.type === 'note' ? { ...t, mode: 'blocks' } : t) }));
    for (const g of state.groups) { for (const tab of g.tabs) if (tab.path) await loadDocument(tab.path); if (!g.tabs.some(t => t.id === g.active)) g.active = g.tabs[0]?.id; }
    state.activeGroup = state.groups.some(g => g.id === saved.activeGroup) ? saved.activeGroup : state.groups[0].id;
  }
  updateSidebars(); renderWorkspace(); renderLeft(); renderRight(); status();
  const params=new URLSearchParams(location.search), requested=params.get('path');
  if(params.has('fecha'))openCalendar(params.get('fecha'));
  else
  if (requested && state.files.some(file => !file.is_dir && file.path === requested)) {
    await openNote(requested);
  } else if (!activeTab() || activeTab().type === 'empty') {
    openHome();
  }
  run(checkReminders());
  setInterval(()=>run(checkReminders()),30_000);
}
const notion = createNotion({ state, prefs, activePath: () => activeTab()?.path, chatWithNote: path => chat.open(path), createNote, recentFiles, homeCollection, dailyNote, openCalendar, icon, button, api, post, run, report, notify, loadDocument, changed, saveDocument, openNote, refreshFiles, renderWorkspace, renderRight, persist, promptName, dialog, closeOverlay, menu, chooser, showLeft, openHome, validPath, renamePath, mountCanvas });
run(init());

