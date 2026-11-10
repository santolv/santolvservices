import { mountChat } from './chat.js';

const WORDCRAFT_AUTOSAVE_MS = 4000;
const MAX_FILE = 32 * 1024 * 1024;
const fileUrl = (route, path) => route + '?path=' + encodeURIComponent(path);

async function requireOk(response) {
  if (response.ok) return response;
  const body = await response.json().catch(() => ({}));
  throw new Error(body.error || 'El servidor devolvió un error (' + response.status + ').');
}

export async function openCraft(path, editor) {
  const view = document.querySelector('#craft-view');
  const frame = document.querySelector('#craft-editor');
  const title = document.querySelector('#craft-title');
  const status = document.querySelector('#craft-message');
  const recovery = document.querySelector('#craft-recovery');
  const back = view.querySelector('a');
  view.hidden = false;
  const isWordCraft = editor?.id === "wordcraft";
  view.classList.toggle("craft-wordcraft", isWordCraft);
  document.querySelector('.hub-bar').inert = true;
  document.querySelector('.hub-layout').inert = true;

  const segments = path.split('/');
  const filename = segments.pop();
  const parent = segments.join('/');
  const localPath = name => parent ? parent + '/' + name : name;
  back.href = parent ? '/?folder=' + encodeURIComponent(parent) : '/';
  title.textContent = filename;
  frame.hidden = true;

  let statusTimer;
  function report(message, error = false) {
    clearTimeout(statusTimer);
    status.textContent = message;
    status.toggleAttribute('data-error', error);
    if (message.startsWith('Guardado en Mis archivos:')) statusTimer = setTimeout(() => report(''), 3800);
  }

  if (!editor?.available) {
    report('El editor para este archivo no está disponible en el servidor.', true);
    return;
  }

  report('Abriendo ' + editor.name + '…');
  const prefix = editor.id + ':';
  let dirty = false;
  let pending = 0;
  let failed = null;
  let queue = Promise.resolve();
  let ready = false;
  let revisions;
  let original;
  let currentName = filename;
  let requestSave;

  if (isWordCraft) {
    const launcher = view.querySelector('#craft-chat-open');
    const chat = mountChat(view.querySelector('#craft-chat'), {
      storage: {
        get(key, fallback) { try { return JSON.parse(localStorage.getItem(key)) ?? fallback; } catch { return fallback; } },
        set(key, value) { try { localStorage.setItem(key, JSON.stringify(value)); return true; } catch { return false; } },
      },
      icon: () => '<span aria-hidden="true">×</span>',
      documentOnly: true,
      onVisibility(open) {
        launcher.setAttribute('aria-expanded', String(open));
        launcher.hidden = open;
      },
      async getDocument(file) {
        if (file !== localPath(currentName)) throw new Error('Abre el chat del documento actual para usar su texto.');
        if (failed || dirty || pending) throw new Error('Guarda el documento en WordCraft y vuelve a pulsar Copiar consulta.');
        const response = await requireOk(await fetch(fileUrl('/api/craft/context', file), {
          headers: { 'If-Match': revisions.get(currentName) },
        }));
        const context = await response.json();
        if (!context.content.trim()) throw new Error('El documento guardado aún no contiene texto.');
        return context;
      },
    });
    launcher.onclick = () => {
      requestSave?.();
      chat.open(localPath(currentName));
    };
  }

  window.addEventListener('beforeunload', event => {
    if (dirty || pending || failed) {
      event.preventDefault();
      event.returnValue = '';
    }
  });

  recovery.onclick = () => {
    if (!failed) return;
    const url = URL.createObjectURL(new Blob([failed.bytes]));
    const link = document.createElement('a');
    link.href = url;
    link.download = failed.name;
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  };

  function send(kind, fields, transfer = []) {
    frame.contentWindow.postMessage({ type: prefix + kind, ...fields }, location.origin, transfer);
  }

  async function save({ name, bytes, id, saveAs }) {
    pending++;
    try {
      const destination = localPath(name);
      const revision = revisions.get(name);
      report('Guardando ' + name + '…');
      const response = await requireOk(await fetch(fileUrl(revision ? '/api/craft/file' : '/api/upload', destination), {
        method: 'PUT',
        headers: revision ? { 'If-Match': revision } : {},
        body: bytes,
      }));
      if (revision || name.toLowerCase().endsWith('.' + editor.extension)) {
        revisions.set(name, (await response.json()).revision);
      }
      if (saveAs && revisions.has(name) && name.toLowerCase().endsWith('.' + editor.extension)) {
        title.textContent = name;
        currentName = name;
      }
      if (failed?.name === name) {
        failed = null;
        recovery.hidden = true;
      }
      if (id !== undefined) send('saved', { id });
      report(failed ? 'Hay una copia sin guardar: ' + failed.name : 'Guardado en Mis archivos: ' + name, Boolean(failed));
    } catch (error) {
      failed = { name, bytes };
      recovery.hidden = false;
      if (id !== undefined) send('failed', { id, error: error.message });
      report(error.message, true);
    } finally {
      pending--;
    }
  }

  try {
    const response = await requireOk(await fetch(fileUrl('/api/craft/file', path)));
    const revision = response.headers.get('ETag');
    if (!revision) throw new Error('No se pudo obtener la versión del archivo.');
    original = await response.arrayBuffer();
    revisions = new Map([[filename, revision]]);
  } catch (error) {
    report(error.message, true);
    return;
  }

  const timeout = setTimeout(() => {
    report(editor.name + ' no respondió. Comprueba que el navegador admite WebGPU o WebGL2 y vuelve a abrir el archivo.', true);
  }, 45000);

  window.addEventListener('message', event => {
    if (event.origin !== location.origin || event.source !== frame.contentWindow) return;
    const data = event.data;
    if (typeof data?.type !== 'string' || !data.type.startsWith(prefix)) return;
    switch (data.type.slice(prefix.length)) {
      case 'ready':
        if (ready) return;
        ready = true;
        clearTimeout(timeout);
        send('open', { name: filename, bytes: original }, [original]);
        original = null;
        frame.hidden = false;
        if (isWordCraft) {
          view.querySelector('#craft-chat-open').hidden = false;
          const childDocument = frame.contentDocument;
          if (childDocument?.body) {
            const returnButton = childDocument.createElement("button");
            returnButton.type = "button";
            returnButton.textContent = "‹ Mis archivos";
            returnButton.setAttribute("aria-label", "Volver a Mis archivos");
            const light = document.documentElement.dataset.theme === "light";
            // shortcut: covers the disabled AutoSave slot in WordCraft 0.16, use a native toolbar action when the host exposes one.
            returnButton.style.cssText = "position:absolute;left:0;top:0;z-index:2;width:120px;height:35px;padding:0 10px;border:0;text-align:left;font:13px system-ui;cursor:pointer;color:" + (light ? "#222;background:#f3f3f3" : "#dedede;background:#232323");
            returnButton.onclick = () => location.assign(back.href);
            childDocument.body.append(returnButton);
            back.closest(".craft-bar").hidden = true;
            const canvas = childDocument.getElementById("wordcraft_canvas");
            if (canvas) {
              let composing = false;
              childDocument.addEventListener("compositionstart", () => { composing = true; });
              childDocument.addEventListener("compositionend", () => { composing = false; });
              requestSave = () => {
                if (!dirty || pending || failed || composing || !canvas.isConnected) return;
                // shortcut: WordCraft 0.16 listens for Save on the canvas, use a host command when the editor exposes one.
                const mac = /Mac/.test(navigator.platform);
                const saveKey = { key: "s", code: "KeyS", ctrlKey: !mac, metaKey: mac, bubbles: true, cancelable: true };
                canvas.dispatchEvent(new frame.contentWindow.KeyboardEvent("keydown", saveKey));
                canvas.dispatchEvent(new frame.contentWindow.KeyboardEvent("keyup", saveKey));
              };
              setInterval(() => {
                if (document.activeElement === frame) requestSave();
              }, WORDCRAFT_AUTOSAVE_MS);
            }
          }
        }
        report('');
        break;
      case 'dirty':
        dirty = data.dirty === true;
        break;
      case 'failed':
        clearTimeout(timeout);
        report(typeof data.error === 'string' ? data.error : 'No se pudo iniciar el editor.', true);
        break;
      case 'save': {
        if (!ready || !(data.bytes instanceof ArrayBuffer || ArrayBuffer.isView(data.bytes))) return;
        const name = data.name;
        if (typeof name !== 'string' || !name || name.startsWith('.') || /[/\\<>:"|?*\u0000-\u001f]/.test(name)) {
          report('El editor propuso un nombre de archivo no válido.', true);
          return;
        }
        if (data.bytes.byteLength > MAX_FILE) {
          failed = { name, bytes: data.bytes };
          recovery.hidden = false;
          report('El archivo supera 32 MiB.', true);
          return;
        }
        pending++;
        queue = queue.then(async () => {
          pending--;
          await save(data);
        });
        break;
      }
    }
  });
  frame.title = editor.name;
  frame.src = '/craft/' + editor.id + '/index.html?host=parent';
  frame.hidden = false;
}
