// Hosts an ArtCraft editor (WordCraft, GridCraft, DeckCraft, PdfCraft) inside a suite page.
// The editor runs its original WebAssembly build in "page" host mode (data-host="page" on its
// canvas): the host sends `<app>:open` with the file, and the app hands every save to the host as
// `<app>:save` instead of downloading. Files come from and go back to Drive through the Drive API,
// with Drive's revision check, so a save never overwrites a newer version saved elsewhere.
(() => {
  'use strict';
  const API_ORIGIN=location.protocol==='https:'?location.origin:'http://localhost:8443';
  const config = window.SantolvOffice;
  const canvas = document.getElementById(config.canvas);
  const status = document.getElementById('office-status');
  const params = new URLSearchParams(location.search);
  const CHUNK = 1024 * 1024;
  const MAX_BYTES = 512 * 1024 * 1024;
  let me, file = null, dirty = false, saving = Promise.resolve();

  function show(text, state = 'info') {
    status.textContent = text;
    status.dataset.state = state;
    status.hidden = !text;
  }
  async function api(path, {method = 'GET', body, headers = {}} = {}) {
    const h = new Headers(headers);
    if (method !== 'GET') { h.set('X-CSRF-Token', me.csrf_token); h.set('Idempotency-Key', crypto.randomUUID()); }
    if (body !== undefined && !(body instanceof ArrayBuffer) && !ArrayBuffer.isView(body)) { h.set('Content-Type', 'application/json'); body = JSON.stringify(body); }
    const response = await fetch(API_ORIGIN + '/api/v1' + path, {method, body, headers: h, credentials: 'include'});
    if (response.status === 401) { document.body.hidden = true; location.replace('/'); throw new Error('Sesión terminada.'); }
    const data = response.status === 204 ? null : await response.json().catch(() => null);
    if (!response.ok) throw Object.assign(new Error(messageFor(data?.error?.code)), {code: data?.error?.code, status: response.status});
    return data;
  }
  function messageFor(code) {
    return ({
      revision_changed: 'Otra persona ha guardado una versión más reciente. Guarda con otro nombre para no perder tus cambios.',
      revision_conflict: 'Otra persona ha guardado una versión más reciente. Guarda con otro nombre para no perder tus cambios.',
      name_conflict: 'Ya existe un archivo con ese nombre en la carpeta.',
      quota_exceeded: 'No queda espacio en tu Drive.',
      not_found: 'El archivo ya no está disponible o no tienes acceso.',
      service_disabled: 'El servicio está desactivado.',
      service_forbidden: 'No tienes permiso para usar este editor.'
    })[code] || 'No se pudo completar la operación.';
  }
  async function sha256(bytes) {
    return Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256', bytes)), b => b.toString(16).padStart(2, '0')).join('');
  }
  async function download(id) {
    const response = await fetch(API_ORIGIN + '/api/v1/nodes/' + encodeURIComponent(id) + '/content', {credentials: 'include'});
    if (!response.ok) throw new Error(messageFor((await response.json().catch(() => null))?.error?.code));
    const size = Number(response.headers.get('content-length') || 0);
    if (size > MAX_BYTES) throw new Error('El archivo supera el tamaño máximo que admite el editor.');
    return new Uint8Array(await response.arrayBuffer());
  }
  // Drive's resumable upload: create, send checked chunks, complete, wait until stored.
  async function upload(bytes, {name, spaceId, parentId, nodeId, revision}) {
    const created = await api('/uploads', {
      method: 'POST',
      body: {space_id: spaceId, parent_id: parentId ?? null, node_id: nodeId ?? null, name, size: bytes.byteLength, sha256: await sha256(bytes)},
      headers: nodeId ? {'If-Match': '"' + revision + '"'} : {}
    });
    let accepted = false;
    try {
      for (let offset = 0; offset < bytes.byteLength; offset += CHUNK) {
        const chunk = bytes.slice(offset, offset + CHUNK);
        await api('/uploads/' + created.id + '/content', {method: 'PATCH', body: chunk,
          headers: {'Content-Type': 'application/octet-stream', 'Upload-Offset': String(offset), 'X-Chunk-SHA256': await sha256(chunk)}});
      }
      await api('/uploads/' + created.id + '/complete', {method: 'POST'});
      accepted = true;
      for (let attempt = 0; attempt < 240; attempt++) {
        const state = await api('/uploads/' + created.id);
        if (state.state === 'ready') return state.result_node;
        if (state.completion_error) throw new Error(state.completion_error);
        await new Promise(resolve => setTimeout(resolve, 250));
      }
      throw new Error('Drive sigue procesando el archivo. Comprueba la carpeta en unos instantes.');
    } catch (error) {
      if (!accepted) await api('/uploads/' + created.id, {method: 'DELETE'}).catch(() => {});
      throw error;
    }
  }
  async function personalSpace() {
    const {items} = await api('/spaces');
    const space = items.find(item => item.kind === 'personal');
    if (!space) throw new Error('No se encontró tu unidad de Drive.');
    return space.id;
  }
  function send(kind, detail) {
    canvas.dispatchEvent(new CustomEvent(config.app + ':' + kind, {detail}));
  }
  // A save from the editor: the opened file itself (a new Drive version, revision-checked) or a
  // new file beside it (Save As, an export, or an untitled document).
  async function save({name, bytes, saveAs}) {
    const data = new Uint8Array(bytes);
    const replace = file && !saveAs && name === file.name;
    show(replace ? 'Guardando…' : 'Guardando «' + name + '» en Drive…', 'pending');
    try {
      if (replace) {
        await upload(data, {name, spaceId: file.space_id, parentId: file.parent_id, nodeId: file.id, revision: file.revision});
        file = await api('/nodes/' + encodeURIComponent(file.id));
      } else {
        const spaceId = file?.space_id || await personalSpace();
        const nodeId = await upload(data, {name, spaceId, parentId: file?.parent_id ?? null});
        if (saveAs || !file) {
          file = await api('/nodes/' + encodeURIComponent(nodeId));
          history.replaceState(null, '', location.pathname + '?node=' + encodeURIComponent(file.id));
        }
      }
      document.title = (file?.name || name) + ' · Santolv';
      show('Guardado en Drive', 'saved');
    } catch (error) {
      show(error.message, 'error');
    }
  }
  async function start() {
    me = await api('/me');
    window.SantolvShell?.account(me, {logout: async () => { await api('/auth/logout', {method: 'POST'}); window.SantolvShell?.sessionEnded(); location.replace('/'); }, switch: async () => { await api('/auth/logout', {method: 'POST'}); location.replace('/'); }});
    window.SantolvShell?.session(me.expires_at);
    const node = params.get('node');
    let bytes = null;
    if (node) {
      show('Abriendo el archivo…', 'pending');
      file = await api('/nodes/' + encodeURIComponent(node));
      if (file.kind !== 'file') throw new Error('Solo se pueden abrir archivos.');
      bytes = await download(file.id);
      document.title = file.name + ' · Santolv';
    }
    canvas.addEventListener(config.app + ':ready', () => {
      if (bytes) { send('open', {name: file.name, bytes}); bytes = null; }
      show('');
    });
    canvas.addEventListener(config.app + ':save', event => { saving = saving.then(() => save(event.detail || {})); });
    canvas.addEventListener(config.app + ':dirty', event => {
      dirty = Boolean(event.detail?.dirty);
      if (dirty) show('Cambios sin guardar', 'dirty');
    });
    canvas.addEventListener(config.app + ':failed', event => show('El editor no pudo iniciarse: ' + (event.detail?.error || 'error desconocido'), 'error'));
    addEventListener('beforeunload', event => { if (dirty) { event.preventDefault(); event.returnValue = ''; } });
    show('Cargando el editor…', 'pending');
    const module = await import(config.module);
    await module.default({module_or_path: config.wasm});
  }
  start().catch(error => show(error.message || 'No se pudo abrir el editor.', 'error'));
})();
