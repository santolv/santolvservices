import { mountChat } from './chat.js';
import { mountCollaboration } from './accounts.js';

const AUTOSAVE_MS = 1400;
const LIVE_SAVE_MS = 85;
const INPUT_GRACE_MS = 150;
const MAX_FILE = 32 * 1024 * 1024;
const fileUrl = (route, path) => route + '?path=' + encodeURIComponent(path);

async function requireOk(response) {
  if (response.ok) return response;
  const body = await response.json().catch(() => ({}));
  throw Object.assign(new Error(body.error || 'El servidor devolvió un error (' + response.status + ').'),
    { status: response.status });
}

export async function openCraft(path, editor, account) {
  const view = document.querySelector('#craft-view');
  const frame = document.querySelector('#craft-editor');
  const title = document.querySelector('#craft-title');
  const status = document.querySelector('#craft-message');
  const recovery = document.querySelector('#craft-recovery');
  const back = view.querySelector('a');
  view.hidden = false;
  const isWordCraft = editor?.id === "wordcraft";
  const collaborative = ['wordcraft', 'deckcraft', 'gridcraft'].includes(editor?.id);
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
  let presence;
  let editorBase;
  let waiting;
  let lastInput = 0;
  let composing = false;
  let watcher;
  let applyTimer;
  let live = false;
  let liveTimer;
  let flushing = false;
  let applying;
  let applyId = 0;
  let epoch = 0;

  if (isWordCraft) {
    const session=Array.from(crypto.getRandomValues(new Uint8Array(16)), byte=>byte.toString(16).padStart(2,'0')).join('');
    presence=(name=currentName, leave=false)=>fetch(fileUrl('/api/craft/presence',localPath(name))+'&session='+session+(leave?'&leave=true':''),{method:'POST',keepalive:leave}).catch(()=>{});
    await presence();
    setInterval(()=>presence(),12_000);
    window.addEventListener('pagehide',()=>presence(currentName,true));
    window.addEventListener('pageshow',event=>{if(event.persisted)presence();});
    const launcher = view.querySelector('#craft-chat-open');
    const storageKey = key => account ? 'sc-account:' + account.name + ':' + key : key;
    const chat = mountChat(view.querySelector('#craft-chat'), {
      storage: {
        get(key, fallback) { try { return JSON.parse(localStorage.getItem(storageKey(key))) ?? fallback; } catch { return fallback; } },
        set(key, value) { try { localStorage.setItem(storageKey(key), JSON.stringify(value)); return true; } catch { return false; } },
      },
      icon: () => '<span aria-hidden="true">×</span>',
      documentOnly: true,
      onVisibility(open) {
        launcher.setAttribute('aria-expanded', String(open));
        launcher.hidden = open;
      },
    });
    launcher.onclick = () => chat.open(localPath(currentName));
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

  async function snapshot(name, known, signal) {
    const route = signal ? '/api/craft/changes' : '/api/craft/file';
    const response = await fetch(fileUrl(route, localPath(name)), {
      cache: 'no-store',
      headers: known ? { 'If-None-Match': known } : {},
      signal,
    });
    if (response.status === 304) return null;
    await requireOk(response);
    const revision = response.headers.get('ETag');
    if (!revision) throw new Error('No se pudo obtener la versión del archivo.');
    return { revision, bytes: new Uint8Array(await response.arrayBuffer()) };
  }

  function applyWaiting() {
    clearTimeout(applyTimer);
    if (!waiting || !ready || pending || dirty || failed || composing) return;
    if (isWordCraft) {
      if (!live || applying) return;
      const bytes = waiting.bytes.slice().buffer;
      applying = { ...waiting, id: ++applyId };
      send('apply', { id: applying.id, name: currentName, bytes }, [bytes]);
      return;
    }
    const delay = INPUT_GRACE_MS - (Date.now() - lastInput);
    if (delay > 0) {
      applyTimer = setTimeout(applyWaiting, delay);
      return;
    }
    stopWatching();
    editorBase = waiting;
    waiting = null;
    revisions.set(currentName, editorBase.revision);
    original = editorBase.bytes.slice().buffer;
    ready = false;
    requestSave = null;
    report('Actualizando ' + currentName + '…');
    loadFrame();
  }

  function stopWatching() {
    watcher?.abort();
    watcher = null;
  }

  function scheduleLiveSave() {
    if (liveTimer) return;
    liveTimer = setTimeout(() => {
      liveTimer = null;
      requestSave?.();
    }, LIVE_SAVE_MS);
  }

  async function watchChanges() {
    if (!collaborative || !ready || pending || failed || watcher) return;
    const controller = new AbortController();
    watcher = controller;
    const name = currentName;
    const base = editorBase;
    let retry = 0;
    try {
      const fresh = await snapshot(name, waiting?.revision || base.revision, controller.signal);
      if (name === currentName && base === editorBase && !pending && !failed) {
        if (fresh) waiting = fresh;
        applyWaiting();
        if (waiting && dirty) requestSave?.();
      }
    } catch (error) {
      if ([401, 403, 404, 415].includes(error.status)) {
        if (watcher === controller) report(error.message, true);
        retry = null;
      } else retry = 1000;
    } finally {
      if (watcher === controller) {
        watcher = null;
        if (retry !== null) setTimeout(watchChanges, retry);
      }
    }
  }

  async function saveCollaborative(name, bytes) {
    const base = editorBase;
    const local = new Uint8Array(bytes instanceof ArrayBuffer ? bytes : bytes.buffer, bytes.byteOffset || 0, bytes.byteLength);
    let expected = base.revision;
    let output = local;
    let merged = false;
    for (let attempt = 0; attempt < 4; attempt++) {
      if (!expected) {
        const remote = await snapshot(name);
        const { mergeOffice } = await import('./craft-merge.bundle.js');
        output = mergeOffice(base.bytes, local, remote.bytes, editor.id);
        expected = remote.revision;
        merged = true;
      }
      const response = await fetch(fileUrl('/api/craft/file', localPath(name)), {
        method: 'PUT', headers: { 'If-Match': expected }, body: output,
      });
      if (response.status === 412) { expected = null; continue; }
      await requireOk(response);
      const { revision } = await response.json();
      revisions.set(name, revision);
      // The iframe still contains the version it serialized until the merged copy is applied.
      editorBase = { revision: merged ? null : revision, bytes: local };
      waiting = merged ? { revision, bytes: output } : null;
      return;
    }
    throw new Error('El archivo sigue cambiando. Descarga tu copia local para conservar tus cambios.');
  }

  async function save({ name, bytes, id, saveAs }, automatic) {
    pending++;
    try {
      const destination = localPath(name);
      const revision = revisions.get(name);
      if (!automatic) report('Guardando ' + name + '…');
      if (collaborative && name === currentName && editorBase) {
        await saveCollaborative(name, bytes);
      } else {
        const response = await requireOk(await fetch(fileUrl(revision ? '/api/craft/file' : '/api/upload', destination), {
          method: 'PUT',
          headers: revision ? { 'If-Match': revision } : {},
          body: bytes,
        }));
        if (revision || name.toLowerCase().endsWith('.' + editor.extension)) {
          revisions.set(name, (await response.json()).revision);
        }
      }
      if ((saveAs || editor.id === 'gridcraft') && currentName !== name && revisions.has(name) && name.toLowerCase().endsWith('.' + editor.extension)) {
        title.textContent = name;
        if (presence && currentName!==name) {
          await presence(name);
          presence(currentName,true);
        }
        currentName = name;
        if (collaborative) {
          stopWatching();
          editorBase = { revision: revisions.get(name), bytes: bytes instanceof ArrayBuffer ? new Uint8Array(bytes) : new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength) };
          waiting = null;
        }
      }
      if (failed?.name === name) {
        failed = null;
        recovery.hidden = true;
      }
      if (id !== undefined) send('saved', { id });
      if (!automatic || failed) {
        report(failed ? 'Hay una copia sin guardar: ' + failed.name : 'Guardado en Mis archivos: ' + name, Boolean(failed));
      }
    } catch (error) {
      failed = { name, bytes };
      recovery.hidden = false;
      if (id !== undefined) send('failed', { id, error: error.message });
      report(error.message, true);
    } finally {
      pending--;
      applyWaiting();
      watchChanges();
      if (live && dirty) scheduleLiveSave();
    }
  }

  try {
    const initial = await snapshot(filename);
    if (collaborative) editorBase = initial;
    original = initial.bytes.slice().buffer;
    revisions = new Map([[filename, initial.revision]]);
  } catch (error) {
    report(error.message, true);
    return;
  }
  if (collaborative && account) mountCollaboration({ account, path: () => localPath(currentName), report });

  let timeout;
  function loadFrame() {
    clearTimeout(timeout);
    timeout = setTimeout(() => report(editor.name + ' no respondió. Comprueba que el navegador admite WebGPU o WebGL2 y vuelve a abrir el archivo.', true), 45000);
    frame.src = '/craft/' + editor.id + '/index.html?host=parent' + (isWordCraft ? '&live=1' : '') + (collaborative ? '&session=' + (++epoch) : '');
  }

  if (collaborative) {
    setInterval(() => requestSave?.(), AUTOSAVE_MS);
    window.addEventListener('focus', watchChanges);
    window.addEventListener('pagehide', stopWatching);
    window.addEventListener('pageshow', watchChanges);
  }

  window.addEventListener('message', event => {
    if (event.origin !== location.origin || event.source !== frame.contentWindow) return;
    const data = event.data;
    if (typeof data?.type !== 'string' || !data.type.startsWith(prefix)) return;
    switch (data.type.slice(prefix.length)) {
      case 'live-ready':
        if (isWordCraft) live = true;
        break;
      case 'changed':
        if (live) {
          dirty = true;
          scheduleLiveSave();
        }
        break;
      case 'flushed':
        flushing = false;
        break;
      case 'live-error':
        report(String(data.error), true);
        break;
      case 'applied': {
        if (!applying || data.id !== applying.id) break;
        const applied = applying;
        applying = null;
        if (data.outcome === 'applied') {
          stopWatching();
          editorBase = { revision: applied.revision, bytes: applied.bytes };
          revisions.set(currentName, applied.revision);
          if (waiting?.revision === applied.revision) waiting = null;
          applyWaiting();
          watchChanges();
        } else if (data.outcome === 'busy') {
          dirty = true;
          scheduleLiveSave();
        } else if (data.outcome === 'invalid') {
          report('WordCraft no pudo aplicar la revisión remota.', true);
        }
        break;
      }
      case 'ready':
        if (ready) return;
        ready = true;
        clearTimeout(timeout);
        send('open', { name: currentName, bytes: original }, [original]);
        original = null;
        frame.hidden = false;
        if (isWordCraft) {
          view.querySelector('#craft-chat-open').hidden = false;
          const childDocument = frame.contentDocument;
          if (childDocument?.body) {
            const returnButton = childDocument.createElement("button");
            returnButton.type = "button";
            returnButton.innerHTML = '<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m14 6-6 6 6 6"/></svg><span>Mis archivos</span>';
            returnButton.setAttribute("aria-label", "Volver a Mis archivos");
            const light = document.documentElement.dataset.theme === "light";
            // shortcut: covers the disabled AutoSave slot in WordCraft 0.16, use a native toolbar action when the host exposes one.
            const palette=getComputedStyle(document.documentElement);
            const returnStyle=childDocument.createElement('style');
            returnButton.className='sc-craft-back';
            const slot=childDocument.createElement('div');
            slot.className='sc-craft-back-slot';
            returnStyle.textContent=`.sc-craft-back-slot{position:absolute;top:0;left:0;z-index:2;display:flex;align-items:center;padding-left:6px;width:120px;height:35px;box-sizing:border-box;background:${light?'#f3f3f3':'#232323'}}
              .sc-craft-back{display:inline-flex;align-items:center;justify-content:center;gap:3px;height:29px;padding:0 10px 0 6px;border:1px solid ${light?'#0002':'#ffffff26'};border-radius:999px;background:${palette.getPropertyValue('--ss-raised')};color:${palette.getPropertyValue('--ss-text-strong')};box-shadow:inset 0 1px 0 #ffffff24,0 2px 5px #0002;backdrop-filter:blur(12px);font:550 12px/1 -apple-system,BlinkMacSystemFont,"Segoe UI",system-ui,sans-serif;cursor:pointer}
              .sc-craft-back:hover{filter:brightness(${light?'.97':'1.16'})}.sc-craft-back:active{box-shadow:inset 0 1px 4px #0003}.sc-craft-back:focus-visible{outline:2px solid ${palette.getPropertyValue('--ss-accent')};outline-offset:1px}`;
            childDocument.head.append(returnStyle);
            returnButton.onclick = () => location.assign(back.href);
            slot.append(returnButton);
            childDocument.body.append(slot);
            back.closest(".craft-bar").hidden = true;
          }
        }
        if (collaborative) {
          const child = frame.contentDocument;
          const canvas = child?.getElementById(editor.id + '_canvas');
          if (canvas) {
            composing = false;
            const track = event => { if (event.isTrusted) lastInput = Date.now(); };
            child.addEventListener('keydown', track, true);
            child.addEventListener('pointerdown', track, true);
            child.addEventListener('compositionstart', () => { composing = true; });
            child.addEventListener('compositionend', () => {
              composing = false;
              lastInput = Date.now();
              applyWaiting();
              if (live && dirty) scheduleLiveSave();
            });
            requestSave = () => {
              if (live) {
                if (!ready || !dirty || pending || failed || composing || flushing) return;
                flushing = true;
                send('flush', { name: currentName });
                return;
              }
              if (!ready || !dirty || pending || failed || composing || !canvas.isConnected || Date.now() - lastInput < 700) return;
              // The installed web builds expose Save through their keyboard command.
              const mac = /Mac/.test(navigator.platform);
              const key = { key: 's', code: 'KeyS', ctrlKey: !mac, metaKey: mac, bubbles: true, cancelable: true };
              canvas.dispatchEvent(new frame.contentWindow.KeyboardEvent('keydown', key));
              canvas.dispatchEvent(new frame.contentWindow.KeyboardEvent('keyup', key));
            };
          }
        }
        report(isWordCraft && !live ? 'Instala la compilación de WordCraft con edición en directo para recibir cambios.' : '');
        watchChanges();
        break;
      case 'dirty':
        if (ready) {
          dirty = data.dirty === true;
          applyWaiting();
        }
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
        const automatic = live && flushing && name === currentName && !data.saveAs;
        pending++;
        queue = queue.then(async () => {
          pending--;
          await save(data, automatic);
        });
        break;
      }
    }
  });
  frame.title = editor.name;
  loadFrame();
  frame.hidden = false;
}
