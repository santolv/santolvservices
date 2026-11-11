const login = document.querySelector('#account-login');
async function request(url, body, options) {
  const response = await fetch(url, {
    ...(body === undefined ? {} : {
      method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body),
    }),
    ...options,
  });
  if (response.status === 204) return null;
  const result = await response.json().catch(() => ({}));
  if (!response.ok) throw Object.assign(new Error(result.error || 'No se pudo completar la acción.'), { status: response.status });
  return result;
}

if (login) login.onsubmit = async event => {
  event.preventDefault();
  const button = event.submitter;
  const error = document.querySelector('#account-error');
  button.disabled = true;
  error.textContent = '';
  try {
    await request('/api/session/login', Object.fromEntries(new FormData(login)));
    const next = new URLSearchParams(location.search).get('next') || '/';
    const target = new URL(next, location.origin);
    location.replace(target.origin === location.origin && ['/', '/index.html', '/cerebro', '/cerebro/'].includes(target.pathname)
      ? target.pathname + target.search : '/');
  } catch (failure) {
    error.textContent = failure.message;
    login.elements.password.select();
  } finally { button.disabled = false; }
};

export function mountAccount(account) {
  if (!account) return;
  const badge = document.querySelector('#account-name');
  const logout = document.querySelector('#account-logout');
  badge.textContent = account.name;
  badge.hidden = logout.hidden = false;
  logout.onclick = async () => {
    try { await request('/api/session/logout', {}); location.replace('/login'); }
    catch (error) { document.querySelector('#message').textContent = error.message; }
  };
}

export function mountCollaboration({ account, path, report }) {
  const toolbar = document.querySelector('#craft-collaboration');
  const button = document.querySelector('#craft-share');
  const participants = document.querySelector('#craft-participants');
  const panel = document.querySelector('#share-dialog');
  const options = document.querySelector('#share-users');
  const error = document.querySelector('#share-error');
  const shareUrl = () => '/api/craft/share?path=' + encodeURIComponent(path());
  const presenceUrl = () => '/api/craft/participants?path=' + encodeURIComponent(path());
  let polling = false;
  toolbar.hidden = false;
  button.hidden = !account.owner;
  participants.textContent = account.name;
  document.querySelector('#share-close').onclick = () => panel.close();

  async function loadShare() {
    const { users, editors } = await request(shareUrl());
    options.replaceChildren();
    if (!users.length) options.textContent = 'Todavía no hay cuentas de colaboradores.';
    for (const user of users) {
      const label = document.createElement('label');
      const toggle = document.createElement('input');
      toggle.type = 'checkbox';
      toggle.checked = editors?.includes(user) === true;
      toggle.onchange = async () => {
        toggle.disabled = true;
        error.textContent = '';
        try { await request(shareUrl(), { user, allowed: toggle.checked }); }
        catch (failure) { toggle.checked = !toggle.checked; error.textContent = failure.message; }
        finally { toggle.disabled = false; }
      };
      label.append(toggle, document.createTextNode(user));
      options.append(label);
    }
  }

  button.onclick = async () => {
    error.textContent = '';
    options.replaceChildren();
    document.querySelector('#share-link').value = new URL('/?edit=' + encodeURIComponent(path()), location.origin).href;
    panel.showModal();
    try { await loadShare(); }
    catch (failure) { error.textContent = failure.message; }
  };

  const create = document.querySelector('#share-create-user');
  create.onsubmit = async event => {
    event.preventDefault();
    const submit = event.submitter;
    submit.disabled = true;
    error.textContent = '';
    try {
      const fields = Object.fromEntries(new FormData(create));
      await request('/api/accounts', fields);
      create.reset();
      document.querySelector('#share-new-user').open = false;
      await loadShare();
      error.textContent = 'Cuenta creada. Activa su permiso de edición en la lista.';
    } catch (failure) { error.textContent = failure.message; }
    finally { submit.disabled = false; }
  };

  document.querySelector('#share-copy').onclick = async () => {
    const field = document.querySelector('#share-link');
    try { await navigator.clipboard.writeText(field.value); error.textContent = 'Enlace copiado.'; }
    catch { field.select(); }
  };

  async function update() {
    if (polling) return;
    polling = true;
    try {
      const { users } = await request(presenceUrl(), {});
      participants.textContent = users.join(', ');
      participants.title = users.join(', ');
    } catch (error) {
      if (error.status === 401 || error.status === 403) {
        participants.textContent = '';
        report(error.message, true);
      }
    } finally { polling = false; }
  }
  setInterval(update, 4000);
  window.addEventListener('pagehide', () => request(presenceUrl() + '&leave=true', {}, { keepalive: true }).catch(() => {}));
  window.addEventListener('focus', update);
  update();
}
