export function buildChatPrompt(question, document) {
  if (!document) return question.trim();
  return [
    question.trim(),
    'Usa el archivo adjunto como material de referencia.',
    'Archivo: ' + document.path,
    ...(document.truncated ? ['El texto está recortado por longitud.'] : []),
    '',
    document.content,
  ].join('\n');
}

export function mountChat(panel, { storage, getDocument, icon, documentOnly = false, onVisibility = () => {} }) {
  panel.innerHTML = [
    '<header class="ai-chat-header"><div><h2>Chat con IA</h2><span>Proveedor pendiente</span></div>',
    '<button class="icon-button" type="button" data-chat-close aria-label="Cerrar chat">' + icon('X', 17) + '</button></header>',
    '<div class="ai-chat-context"><span data-chat-context></span>',
    '<button class="icon-button" type="button" data-chat-general aria-label="Volver al chat general" title="Volver al chat general">' + icon('X', 14) + '</button></div>',
    '<div class="ai-chat-thread"><p data-chat-empty>Tus preguntas guardadas aparecerán aquí.</p><ol data-chat-messages aria-label="Preguntas guardadas"></ol></div>',
    '<form class="ai-chat-composer">',
    '<textarea aria-label="Tu pregunta" placeholder="Escribe tu pregunta…" rows="3" maxlength="8000" required></textarea>',
    '<div class="ai-chat-actions"><button class="secondary-button" type="button" data-chat-copy>Copiar consulta</button>',
    '<button class="primary-button" type="submit">Guardar pregunta</button></div>',
    '<p data-chat-status role="status"></p>',
    '<textarea data-chat-preview aria-label="Consulta para copiar" rows="4" readonly hidden></textarea>',
    '</form>',
  ].join('');

  const find = selector => panel.querySelector(selector);
  const form = find('form');
  const composer = find('textarea');
  const preview = find('[data-chat-preview]');
  const context = find('[data-chat-context]');
  const general = find('[data-chat-general]');
  const messages = find('[data-chat-messages]');
  const status = find('[data-chat-status]');
  const copy = find('[data-chat-copy]');
  const drafts = new Map();
  const unsavedThreads = new Map();
  let path = '';
  let opener;

  const key = () => 'sc-chat:' + path;
  function questions() {
    const thread = unsavedThreads.get(key()) ?? storage.get(key(), []);
    return Array.isArray(thread) ? thread.filter(question => typeof question === 'string') : [];
  }
  const question = () => composer.value.trim() || questions().at(-1);
  function render() {
    context.textContent = path || 'Chat general';
    context.title = path;
    general.hidden = documentOnly || !path;
    const thread = questions();
    find('[data-chat-empty]').hidden = thread.length > 0;
    messages.replaceChildren(...thread.map(text => {
      const item = document.createElement('li');
      item.textContent = text;
      return item;
    }));
    messages.parentElement.scrollTop = messages.parentElement.scrollHeight;
    copy.disabled = !question();
  }
  function open(file = '') {
    if (panel.hidden) opener = document.activeElement;
    path = file;
    panel.hidden = false;
    onVisibility(true);
    composer.value = drafts.get(key()) || '';
    status.textContent = '';
    preview.hidden = true;
    preview.value = '';
    render();
    composer.focus();
  }

  find('[data-chat-close]').onclick = () => {
    panel.hidden = true;
    onVisibility(false);
    preview.hidden = true;
    preview.value = '';
    if (opener?.isConnected) opener.focus();
  };
  general.onclick = () => open();
  composer.oninput = () => {
    drafts.set(key(), composer.value);
    copy.disabled = !question();
    preview.hidden = true;
    preview.value = '';
    status.textContent = '';
  };
  form.onsubmit = event => {
    event.preventDefault();
    const text = composer.value.trim();
    if (!text) return;
    const thread = [...questions(), text];
    const saved = storage.set(key(), thread);
    if (saved) unsavedThreads.delete(key());
    else unsavedThreads.set(key(), thread);
    drafts.delete(key());
    composer.value = '';
    preview.hidden = true;
    preview.value = '';
    status.textContent = saved ? 'Pregunta guardada en este navegador.' : 'Pregunta guardada durante esta sesión.';
    render();
    composer.focus();
  };
  copy.onclick = async () => {
    const text = question();
    if (!text) return;
    const threadKey = key();
    let prompt;
    copy.disabled = true;
    status.textContent = 'Preparando consulta…';
    try {
      const file = path ? await getDocument(path) : null;
      if (path && !file) throw new Error('Vuelve a abrir el chat desde el archivo para usar su contenido.');
      prompt = buildChatPrompt(text, file);
    } catch (error) {
      if (!panel.hidden && key() === threadKey) status.textContent = error.message;
      return;
    } finally {
      if (!panel.hidden && key() === threadKey) copy.disabled = !question();
    }
    if (panel.hidden || key() !== threadKey) return;
    try {
      await navigator.clipboard.writeText(prompt);
      if (panel.hidden || key() !== threadKey) return;
      status.textContent = path ? 'Consulta y archivo copiados.' : 'Consulta copiada.';
    } catch {
      if (panel.hidden || key() !== threadKey) return;
      preview.value = prompt;
      preview.hidden = false;
      preview.focus();
      preview.select();
      status.textContent = 'Pulsa Ctrl+C para copiar el texto seleccionado.';
    }
  };

  return { open };
}
