export function mountChat(panel, { storage, icon, documentOnly = false, onVisibility = () => {} }) {
  panel.innerHTML = [
    '<header class="ai-chat-header"><div><h2>Chat con IA</h2><span data-chat-model></span></div>',
    '<button class="icon-button" type="button" data-chat-close aria-label="Cerrar chat">' + icon('X', 17) + '</button></header>',
    '<div class="ai-chat-context"><span data-chat-context></span>',
    '<button class="icon-button" type="button" data-chat-general aria-label="Volver al chat general" title="Volver al chat general">' + icon('X', 14) + '</button></div>',
    '<div class="ai-chat-thread"><p data-chat-empty>Escribe un mensaje para empezar.</p><ol data-chat-messages aria-label="Conversación"></ol></div>',
    '<form class="ai-chat-composer">',
    '<textarea aria-label="Tu pregunta" placeholder="Escribe tu pregunta…" rows="3" maxlength="8000" required></textarea>',
    '<div class="ai-chat-actions"><button class="secondary-button" type="button" data-chat-copy>Copiar consulta</button>',
    '<button class="primary-button" type="submit" disabled>Enviar</button></div>',
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
  const send = find('[type=submit]');
  const model = find('[data-chat-model]');
  const drafts = new Map();
  const unsavedThreads = new Map();
  const pending = new Set();
  let enabled = false;
  let opening = 0;
  let path = '';
  let opener;

  const key = () => 'sc-chat:' + path;
  function questions() {
    const thread = unsavedThreads.get(key()) ?? storage.get(key(), []);
    return Array.isArray(thread) ? thread.flatMap(message => {
      if (typeof message === 'string') return [{ role: 'user', content: message }];
      return message && ['user', 'assistant'].includes(message.role) && typeof message.content === 'string' ? [message] : [];
    }) : [];
  }
  const question = () => composer.value.trim() || questions().reverse().find(message => message.role === 'user')?.content;
  const endpoint = (route, file) => '/api/ai/' + route + '?path=' + encodeURIComponent(file);
  function store(threadKey, thread) {
    if (storage.set(threadKey, thread)) unsavedThreads.delete(threadKey);
    else unsavedThreads.set(threadKey, thread);
  }
  function render() {
    context.textContent = path || 'Chat general';
    context.title = path;
    general.hidden = documentOnly || !path;
    const thread = questions();
    find('[data-chat-empty]').hidden = thread.length > 0;
    messages.replaceChildren(...thread.map(message => {
      const item = document.createElement('li');
      item.dataset.role = message.role;
      item.textContent = message.content;
      return item;
    }));
    messages.parentElement.scrollTop = messages.parentElement.scrollHeight;
    copy.disabled = !question();
    send.disabled = !enabled || pending.has(key());
  }
  async function open(file = '') {
    if (panel.hidden) opener = document.activeElement;
    path = file;
    const current = ++opening;
    enabled = false;
    model.textContent = 'Comprobando proveedor…';
    panel.hidden = false;
    onVisibility(true);
    composer.value = drafts.get(key()) || '';
    status.textContent = '';
    preview.hidden = true;
    preview.value = '';
    render();
    composer.focus();
    try {
      const response = await fetch(endpoint('status', file));
      if (!response.ok) throw new Error();
      const config = await response.json();
      if (current !== opening) return;
      enabled = config.enabled;
      model.textContent = enabled ? config.model : 'Configura la IA en Mis archivos → Ajustes';
    } catch {
      if (current !== opening) return;
      model.textContent = 'Proveedor no disponible';
    }
    render();
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
  form.onsubmit = async event => {
    event.preventDefault();
    const text = composer.value;
    const threadKey = key();
    const filePath = path;
    if (!text.trim() || !enabled || pending.has(threadKey)) return;
    const history = questions();
    pending.add(threadKey);
    render();
    status.textContent = 'Esperando respuesta…';
    try {
      const response = await fetch(endpoint('chat', filePath), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ message: text }),
      });
      const result = await response.json();
      if (!response.ok) throw new Error(result.error || 'La IA no pudo responder.');
      store(threadKey, [...history, { role: 'user', content: text }, { role: 'assistant', content: result.content }]);
      if ((drafts.get(threadKey) ?? text) === text) {
        drafts.delete(threadKey);
        if (key() === threadKey && composer.value === text) composer.value = '';
      }
      if (key() === threadKey) status.textContent = '';
    } catch (error) {
      if (key() === threadKey) status.textContent = error.message;
    } finally {
      pending.delete(threadKey);
      if (key() === threadKey) render();
    }
  };
  copy.onclick = async () => {
    const text = question();
    if (!text) return;
    const threadKey = key();
    try {
      await navigator.clipboard.writeText(text);
      if (panel.hidden || key() !== threadKey) return;
      status.textContent = 'Consulta copiada.';
    } catch {
      if (panel.hidden || key() !== threadKey) return;
      preview.value = text;
      preview.hidden = false;
      preview.focus();
      preview.select();
      status.textContent = 'Pulsa Ctrl+C para copiar el texto seleccionado.';
    }
  };

  return { open };
}
