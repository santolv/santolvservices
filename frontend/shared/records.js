
(() => {
  'use strict';

  const API_ORIGIN=location.protocol==='https:'?location.origin:'http://localhost:8443';
  const base = '';
  const service = document.body.dataset.service;
  const shell = window.SantolvShell;
  const element = id => document.getElementById(id);
  const filters = element('filters');
  const form = element('edit-form');
  const editor = element('editor');
  const noteSelect = form.elements.note_id;
  const states = {pending: 'Pendiente', in_progress: 'En curso', done: 'Hecha'};
  const priorities = {low: 'Baja', normal: 'Normal', high: 'Alta'};
  const dateFormat = new Intl.DateTimeFormat('es', {dateStyle: 'medium', timeStyle: 'short'});
  const errors = {
    invalid_title: 'Introduce un título de 1 a 200 caracteres.',
    invalid_content: 'El contenido admite hasta 50 000 caracteres.',
    invalid_search: 'La búsqueda admite hasta 200 caracteres.',
    invalid_due_at: 'Introduce una fecha de vencimiento válida.',
    not_found: 'El registro o la nota vinculada ya no está disponible.',
    service_disabled: 'El servicio está desactivado.',
    service_forbidden: 'No tienes permiso para acceder a este servicio.'
  };

  let me, resource = service, noteFilter, listCursor, listRequest, activeParams;
  let editing, initialDue, pickerCursor, pickerRequest, pickerParams, saving = false;

  function returnHome() {
    document.body.hidden = true;
    location.replace('/');
  }

  async function api(path, {method = 'GET', body, signal} = {}) {
    const headers = {};
    if (method !== 'GET') headers['X-CSRF-Token'] = me.csrf_token;
    if (body !== undefined) headers['Content-Type'] = 'application/json';
    const response = await fetch(API_ORIGIN + '/api/v1' + path, {
      method, headers, credentials: 'include', signal,
      body: body === undefined ? undefined : JSON.stringify(body)
    });
    if (response.ok) return response.status === 204 ? null : response.json();
    if (response.status === 401) returnHome();
    const data = await response.json().catch(() => null);
    throw new Error(errors[data?.error?.code] || 'No se pudo completar la operación.');
  }

  function message(text, error = false) {
    element('message').textContent = text;
    element('message').dataset.error = String(error);
  }

  function resourcePath(id) {
    return base + '/' + resource + (id ? '/' + encodeURIComponent(id) : '');
  }

  function row(item) {
    const li = document.createElement('li');
    if (resource === 'tasks') li.className = 'task-row' + (item.status === 'done' ? ' done' : '');
    const button = document.createElement('button');
    button.type = 'button';
    const title = document.createElement('strong');
    title.textContent = item.title;
    button.append(title);
    const meta = document.createElement('span');
    meta.className = 'meta';
    if (resource === 'notes') {
      if (item.content) {
        const preview = document.createElement('span');
        preview.className = 'preview';
        const excerpt = Array.from(item.content.slice(0, 320)).slice(0, 140).join('');
        preview.textContent = excerpt + (excerpt.length < item.content.length ? '…' : '');
        button.append(preview);
      }
      meta.textContent = 'Editada ' + dateFormat.format(new Date(item.updated_at));
    } else {
      const details = [states[item.status], priorities[item.priority]];
      if (item.due_at) details.push('Vence ' + dateFormat.format(new Date(item.due_at)));
      if (item.note_id) details.push('Nota vinculada');
      meta.textContent = details.join(' · ');
    }
    button.append(meta);
    button.addEventListener('click', () => openEditor(item));
    li.append(button);
    return li;
  }

  async function loadList(append = false) {
    if (!me || (append && !listCursor)) return;
    listRequest?.abort();
    const request = new AbortController();
    listRequest = request;
    const items = element('items');
    if (!append) {
      items.replaceChildren();
      listCursor = null;
      const {q, archived, status, priority} = filters.elements;
      activeParams = new URLSearchParams();
      if (q.value.trim()) activeParams.set('q', q.value.trim());
      if (resource === 'notes') activeParams.set('archived', archived.value);
      else {
        if (status.value) activeParams.set('status', status.value);
        if (priority.value) activeParams.set('priority', priority.value);
        if (noteFilter) activeParams.set('note_id', noteFilter.id);
      }
    }
    const params = new URLSearchParams(activeParams);
    if (append) params.set('cursor', listCursor);
    element('more').disabled = true;
    element('more').hidden = !append;
    items.setAttribute('aria-busy', 'true');
    message('Cargando…');
    try {
      const data = await api(resourcePath() + '?' + params, {signal: request.signal});
      if (listRequest !== request) return;
      items.append(...data.items.map(row));
      listCursor = data.next_cursor;
      message(items.childElementCount ? '' : 'No hay resultados.');
    } catch (error) {
      if (listRequest === request) message(error.message, true);
    } finally {
      if (listRequest === request) {
        items.setAttribute('aria-busy', 'false');
        element('more').hidden = !listCursor;
        element('more').disabled = false;
      }
    }
  }

  function switchTo(next, linkedNote) {
    if (next !== resource || linkedNote) filters.reset();
    if (next !== service) { location.assign('/' + next + '/' + (linkedNote ? '?note=' + encodeURIComponent(linkedNote.id) : '')); return; }
    resource = next;
    noteFilter = linkedNote;
    const notes = resource === 'notes';

    element('new').textContent = notes ? 'Nueva nota' : 'Nueva tarea';
    element('archived-filter').hidden = !notes;
    document.querySelectorAll('.task-filter').forEach(label => { label.hidden = notes; });
    element('note-filter').hidden = !noteFilter;
    if (noteFilter) element('note-filter-label').textContent = 'Nota: ' + noteFilter.title;
    void loadList();
  }

  function editorError(text = '') {
    element('editor-error').textContent = text;
    element('editor-error').hidden = !text;
  }

  function localMinute(value) {
    if (!value) return '';
    const date = new Date(value);
    const pad = value => String(value).padStart(2, '0');
    return String(date.getFullYear()).padStart(4, '0') + '-' + pad(date.getMonth() + 1)
      + '-' + pad(date.getDate()) + 'T' + pad(date.getHours()) + ':' + pad(date.getMinutes());
  }

  function addNoteOption(id, title) {
    const existing = Array.from(noteSelect.options).find(option => option.value === id);
    if (existing) existing.textContent = title;
    else noteSelect.add(new Option(title, id));
  }

  async function loadNotes(append = false) {
    pickerRequest?.abort();
    const request = new AbortController();
    pickerRequest = request;
    const button = element('load-notes');
    const pickerMessage = element('picker-message');
    if (!append) {
      pickerCursor = null;
      pickerParams = new URLSearchParams({archived: String(element('picker-archived').checked)});
      const q = element('picker-query').value.trim();
      if (q) pickerParams.set('q', q);
      for (const option of Array.from(noteSelect.options)) {
        if (option.value && !option.selected) option.remove();
      }
    }
    const params = new URLSearchParams(pickerParams);
    if (append) params.set('cursor', pickerCursor);
    element('picker-filters').hidden = false;
    button.disabled = true;
    pickerMessage.textContent = 'Buscando notas…';
    try {
      const data = await api(base + '/notes?' + params, {signal: request.signal});
      if (pickerRequest !== request) return;
      for (const note of data.items) addNoteOption(note.id, note.title);
      pickerCursor = data.next_cursor;
      pickerMessage.textContent = data.items.length ? '' : 'No hay notas que coincidan.';
    } catch (error) {
      if (pickerRequest === request) pickerMessage.textContent = error.message;
    } finally {
      if (pickerRequest === request) {
        button.textContent = pickerCursor ? 'Cargar más notas' : 'Buscar notas';
        button.disabled = false;
      }
    }
  }

  function openEditor(item = null) {
    if (!me || editor.open) return;
    editing = item;
    const notes = resource === 'notes';
    form.reset();
    element('editor-title').textContent = notes
      ? (item ? 'Editar nota' : 'Nueva nota')
      : (item ? 'Editar tarea' : 'Nueva tarea');
    for (const [id, visible] of [['note-fields', notes], ['task-fields', !notes]]) {
      element(id).hidden = !visible;
      element(id).disabled = !visible;
    }
    element('archive-field').hidden = !item;
    element('linked-tasks').hidden = !(notes && item);
    element('delete').hidden = notes || !item;
    form.elements.title.value = item?.title ?? '';
    if (notes) {
      form.elements.content.value = item?.content ?? '';
      form.elements.archived.checked = Boolean(item?.archived_at);
    } else {
      form.elements.status.value = item?.status ?? 'pending';
      form.elements.priority.value = item?.priority ?? 'normal';
      initialDue = localMinute(item?.due_at);
      form.elements.due_at.value = initialDue;
      noteSelect.replaceChildren(new Option('Ninguna', ''));
      const noteId = item ? item.note_id : noteFilter?.id;
      if (noteId) {
        addNoteOption(noteId, noteFilter?.id === noteId ? noteFilter.title : 'Nota vinculada');
        noteSelect.value = noteId;
      }
      pickerCursor = null;
      element('picker-query').value = '';
      element('picker-archived').checked = false;
      element('picker-filters').hidden = true;
      element('picker-message').textContent = '';
      element('load-notes').textContent = 'Buscar notas';
      element('load-notes').disabled = false;
      if (noteId && noteFilter?.id !== noteId) {
        const request = new AbortController();
        pickerRequest = request;
        api(base + '/notes/' + encodeURIComponent(noteId), {signal: request.signal})
          .then(note => { if (pickerRequest === request) addNoteOption(note.id, note.title); })
          .catch(() => {});
      }
    }
    editorError();
    editor.showModal();
    form.elements.title.focus();
  }

  async function change(method, body) {
    if (saving) return;
    saving = true;
    for (const id of ['edit-fields', 'cancel', 'save', 'delete', 'linked-tasks']) element(id).disabled = true;
    editorError();
    try {
      await api(resourcePath(editing?.id), {method, body});
      editor.close();
      await loadList();
    } catch (error) {
      editorError(error.message);
    } finally {
      for (const id of ['edit-fields', 'cancel', 'save', 'delete', 'linked-tasks']) element(id).disabled = false;
      saving = false;
    }
  }

  form.addEventListener('submit', event => {
    event.preventDefault();
    const fields = form.elements;
    const body = {title: fields.title.value};
    if (resource === 'notes') {
      body.content = fields.content.value;
      if (editing) body.archived = fields.archived.checked;
    } else {
      body.status = fields.status.value;
      body.priority = fields.priority.value;
      body.note_id = fields.note_id.value || null;
      const due = fields.due_at.value;
      if (editing && due === initialDue) body.due_at = editing.due_at;
      else if (due) {
        const date = new Date(due);
        if (Number.isNaN(date.valueOf())) {
          editorError(errors.invalid_due_at);
          return;
        }
        body.due_at = date.toISOString();
      } else body.due_at = null;
    }
    void change(editing ? 'PUT' : 'POST', body);
  });

  editor.addEventListener('cancel', event => { if (saving) event.preventDefault(); });
  editor.addEventListener('close', () => { pickerRequest?.abort(); pickerRequest = null; });
  element('cancel').addEventListener('click', () => editor.close());
  element('delete').addEventListener('click', () => {
    if (window.confirm('¿Eliminar esta tarea?')) void change('DELETE');
  });
  element('linked-tasks').addEventListener('click', () => {
    const note = editing;
    editor.close();
    switchTo('tasks', note);

  });
  element('load-notes').addEventListener('click', () => void loadNotes(Boolean(pickerCursor)));
  element('search-notes').addEventListener('click', () => void loadNotes());
  element('picker-query').addEventListener('keydown', event => {
    if (event.key === 'Enter') { event.preventDefault(); void loadNotes(); }
  });
  element('picker-archived').addEventListener('change', () => void loadNotes());
  element('new').addEventListener('click', () => openEditor());
  element('clear-note-filter').addEventListener('click', () => switchTo('tasks'));
  element('more').addEventListener('click', () => void loadList(true));
  filters.addEventListener('submit', event => { event.preventDefault(); void loadList(); });
  filters.addEventListener('change', event => { if (event.target.matches('select')) void loadList(); });

  async function endSession() {
    await api('/auth/logout', {method: 'POST'});
    shell.sessionEnded();
    returnHome();
  }

  message('Cargando…');
  api('/me').then(user => {
    me = user;
    shell?.account(me, {logout: endSession, switch: endSession});
    shell?.session(me.expires_at);
    element('new').disabled = false;
    const noteId = new URLSearchParams(location.search).get('note');
    switchTo(service, service === 'tasks' && noteId ? {id:noteId,title:'Nota vinculada'} : undefined);
  }).catch(error => message(error.message, true));
})();
