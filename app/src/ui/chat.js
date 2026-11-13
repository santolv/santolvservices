const endpoint = (route, path = '') => '/api/ai/' + route + '?path=' + encodeURIComponent(path);

async function request(route, path, options) {
  const response = await fetch(endpoint(route, path), options);
  const result = await response.json();
  if (!response.ok) throw new Error(result.error || 'No se pudo abrir el chat.');
  return result;
}

async function streamChat(path, message, onText) {
  const response = await fetch(endpoint('chat', path), {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ message }),
  });
  if (!response.ok) {
    const { error } = await response.json();
    throw new Error(error || 'La IA no pudo responder.');
  }
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  let turns;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split('\n');
      buffer = lines.pop();
      for (const line of lines) {
        const event = JSON.parse(line);
        if (event.error) throw new Error(event.error);
        if (event.delta) onText(event.delta);
        if (event.turns) turns = event.turns;
      }
    }
  } finally {
    await reader.cancel().catch(() => {});
    reader.releaseLock();
  }
  if (!turns) throw new Error('La respuesta se interrumpió antes de guardarse.');
  return turns;
}

export function mountChat(panel, { icon, onVisibility = () => {}, beforeSend = () => {} }) {
  panel.innerHTML = [
    '<header class="ai-chat-header"><div><h2>Chat con IA</h2><span data-model></span></div>',
    '<button class="icon-button" type="button" data-close aria-label="Cerrar chat">' + icon('X', 17) + '</button></header>',
    '<div class="ai-chat-context"><span data-context></span>',
    '<button type="button" class="secondary-button" data-chats>Chats</button>',
    '<button type="button" class="secondary-button" data-delete disabled>Borrar chat</button></div>',
    '<div class="ai-chat-thread"><p data-empty>Escribe un mensaje para empezar.</p><ol aria-label="Conversación"></ol></div>',
    '<div class="ai-chat-index" hidden></div>',
    '<form class="ai-chat-composer"><textarea aria-label="Tu pregunta" placeholder="Escribe tu pregunta…" rows="3" maxlength="8000" required></textarea>',
    '<div class="ai-chat-actions"><button class="primary-button" type="submit" disabled>Enviar</button></div></form>',
    '<p class="ai-chat-status" role="status"></p>',
  ].join('');

  const find = selector => panel.querySelector(selector);
  const composer = find('textarea');
  const form = find('form');
  const send = find('[type=submit]');
  const remove = find('[data-delete]');
  const status = find('[role=status]');
  const model = find('[data-model]');
  const index = find('.ai-chat-index');
  const thread = find('.ai-chat-thread');
  let path = '';
  let opener;
  let generation = 0;
  let pending = false;
  let enabled = false;

  function show(browse) {
    if (panel.hidden) opener = document.activeElement;
    panel.hidden = false;
    onVisibility(true);
    index.hidden = !browse;
    thread.hidden = browse;
    form.hidden = browse;
    remove.hidden = browse;
    status.textContent = '';
  }

  function render(turns) {
    find('[data-empty]').hidden = turns.length > 0;
    remove.disabled = turns.length === 0;
    const list = find('ol');
    list.replaceChildren();
    for (const turn of turns) {
      const question = document.createElement('li');
      question.textContent = turn.author + ': ' + turn.message;
      const answer = document.createElement('li');
      answer.dataset.role = 'assistant';
      answer.textContent = turn.reply.response;
      for (const change of turn.reply.changes) {
        const details = document.createElement('details');
        const label = document.createElement('summary');
        label.textContent = change.location;
        details.append(label);
        for (const [tag, value] of [['del', change.original], ['ins', change.replacement]]) {
          if (!value) continue;
          const text = document.createElement(tag);
          text.textContent = value;
          details.append(text);
        }
        answer.append(details);
      }
      list.append(question, answer);
    }
    thread.scrollTop = thread.scrollHeight;
  }

  async function open(file = '') {
    const current = ++generation;
    path = file;
    enabled = false;
    show(false);
    composer.value = '';
    send.disabled = true;
    model.textContent = '';
    find('[data-context]').textContent = file || 'Chat general';
    find('[data-context]').title = file;
    render([]);
    composer.focus();
    const [history, provider] = await Promise.allSettled([request('history', file), request('status', file)]);
    if (current !== generation) return;
    if (history.status === 'fulfilled') render(history.value.turns);
    else status.textContent = history.reason.message;
    enabled = provider.status === 'fulfilled' && provider.value.enabled;
    model.textContent = enabled ? provider.value.model : 'Proveedor de IA no disponible';
    send.disabled = !enabled || pending;
  }

  async function browse() {
    const current = ++generation;
    show(true);
    model.textContent = '';
    find('[data-context]').textContent = 'Historial de chats';
    find('[data-context]').title = '';
    index.replaceChildren();
    try {
      const { paths, general } = await request('chats');
      if (current !== generation) return;
      for (const file of new Set(general ? ['', ...paths] : paths)) {
        const button = document.createElement('button');
        button.type = 'button';
        button.textContent = file || 'Chat general';
        button.onclick = () => open(file);
        index.append(button);
      }
      if (!paths.length) status.textContent = 'Todavía no hay chats guardados.';
    } catch (error) {
      if (current === generation) status.textContent = error.message;
    }
  }

  find('[data-close]').onclick = () => {
    generation++;
    panel.hidden = true;
    onVisibility(false);
    if (opener?.isConnected) opener.focus();
  };
  find('[data-chats]').onclick = browse;
  remove.onclick = async () => {
    if (pending || !window.confirm('¿Borrar este chat?')) return;
    const current = generation;
    try {
      await request('history', path, { method: 'DELETE' });
      if (current === generation) render([]);
    } catch (error) {
      if (current === generation) status.textContent = error.message;
    }
  };
  form.onsubmit = async event => {
    event.preventDefault();
    const message = composer.value;
    if (!message.trim() || !enabled || pending) return;
    const file = path;
    const current = generation;
    pending = true;
    send.disabled = true;
    status.textContent = '';
    composer.value = '';
    const wasEmpty = !find('[data-empty]').hidden;
    find('[data-empty]').hidden = true;
    const question = document.createElement('li');
    question.textContent = message;
    const answer = document.createElement('li');
    answer.dataset.role = 'assistant';
    const dots = document.createElement('span');
    dots.className = 'ai-chat-thinking';
    dots.setAttribute('role', 'status');
    dots.setAttribute('aria-label', 'La IA está preparando la respuesta');
    dots.innerHTML = '<i></i><i></i><i></i>';
    answer.append(dots);
    const text = document.createTextNode('');
    answer.append(text);
    find('ol').append(question, answer);
    thread.scrollTop = thread.scrollHeight;
    try {
      await beforeSend(file);
      const turns = await streamChat(file, message, delta => {
        if (current !== generation) return;
        const nearBottom = thread.scrollHeight - thread.scrollTop - thread.clientHeight < 36;
        dots.remove();
        text.appendData(delta);
        if (nearBottom) thread.scrollTop = thread.scrollHeight;
      });
      if (current === generation) {
        render(turns);
        status.textContent = '';
      }
    } catch (error) {
      if (current === generation) {
        question.remove();
        answer.remove();
        find('[data-empty]').hidden = !wasEmpty;
        if (!composer.value) composer.value = message;
        status.textContent = error.message;
      }
    } finally {
      pending = false;
      send.disabled = !enabled;
    }
  };
  return { open, browse };
}
