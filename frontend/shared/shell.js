// Shared by Inicio, Drive and Fotos, including other open tabs on this origin.
(() => {
  'use strict';
  const key = 'santolv.theme';
  const sessionKey = 'santolv.session-ended';
  const API_ORIGIN=location.protocol==='https:'?location.origin:'http://localhost:8443';
  let account, sessionTimer;
  function returnHome() {
    document.body.hidden = true;
    location.replace('/');
  }
  const media = window.matchMedia('(prefers-color-scheme: dark)');
  const valid = value => ['light', 'dark', 'system'].includes(value) ? value : 'light';
  function saved() {
    try { return valid(localStorage.getItem(key)); } catch { return 'light'; }
  }
  let preference = saved();
  function apply() {
    const dark = preference === 'dark' || (preference === 'system' && media.matches);
    document.documentElement.dataset.theme = dark ? 'dark' : 'light';
    const label = dark ? 'Cambiar todo a modo claro' : 'Cambiar todo a modo oscuro';
    for (const button of document.querySelectorAll('[data-shell-theme]')) {
      button.setAttribute('aria-label', label);
      button.setAttribute('aria-pressed', String(dark));
      button.title = label;
    }
    for (const select of document.querySelectorAll('[data-shell-theme-select]')) select.value = preference;
  }
  function setTheme(value) {
    preference = valid(value);
    try { localStorage.setItem(key, preference); } catch {}
    apply();
  }
  window.SantolvShell = {
    setTheme,
    session(expiresAt) {
      if (!Number.isFinite(expiresAt)) return;
      clearTimeout(sessionTimer);
      sessionTimer = setTimeout(returnHome, Math.max(0, expiresAt * 1000 - Date.now()));
    },
    sessionEnded() {
      try { localStorage.setItem(sessionKey, String(Date.now())); } catch {}
    },
    account(name, handlers) {
      account = handlers;
      const trigger = document.querySelector('.shell-profile');
      if (!trigger) return;
      trigger.hidden = !name;
      document.getElementById('shell-username').textContent = name || '';
      document.getElementById('shell-initials').textContent = (name || '').slice(0, 2).toUpperCase();
      document.querySelector('[data-shell-account="profile"]').hidden = !handlers?.profile;
      if (!name) document.getElementById('shell-account-menu').hidePopover();
    },
    toggleTheme() { setTheme(document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark'); },
    preference() { return preference; }
  };
  function bind() {
    for (const button of document.querySelectorAll('[data-shell-theme]')) {
      button.addEventListener('click', window.SantolvShell.toggleTheme);
    }
    document.getElementById?.('shell-apps-menu')?.addEventListener('beforetoggle', async event => {
      if (event.newState !== 'open') return;
      const status = document.getElementById('shell-apps-status');
      const links = [...document.querySelectorAll('[data-shell-service]')];
      for (const link of links) link.hidden = true;
      status.hidden = false;
      status.textContent = 'Cargando aplicaciones…';
      try {
        const response = await fetch(API_ORIGIN + '/api/v1/services', {credentials: 'include'});
        if (response.status === 401) { returnHome(); return; }
        if (!response.ok) throw new Error();
        const {items} = await response.json();
        for (const link of links) link.hidden = !items.some(item => item.enabled && item.id === link.dataset.shellService);
        status.hidden = links.some(link => !link.hidden);
        status.textContent = 'No hay aplicaciones activas.';
      } catch { status.textContent = 'No se pudieron consultar las aplicaciones.'; }
    });
    for (const button of document.querySelectorAll('[data-shell-account]')) {
      button.addEventListener('click', async () => {
        const action = account?.[button.dataset.shellAccount];
        if (!action) return;
        const menu = document.getElementById('shell-account-menu');
        const error = document.getElementById('shell-account-error');
        error.hidden = true;
        button.disabled = true;
        try {
          if (button.dataset.shellAccount === 'profile') menu.hidePopover();
          await action();
          menu.hidePopover();
        } catch (failure) {
          error.textContent = failure.message || 'No se pudo completar la operación.';
          error.hidden = false;
        } finally { button.disabled = false; }
      });
    }
    apply();
  }
  apply();
  document.addEventListener('DOMContentLoaded', bind, { once: true });
  window.addEventListener('pageshow', event => {
    if (event?.persisted) { document.body.hidden = true; location.reload(); return; }
    preference = saved(); apply();
  });
  window.addEventListener('storage', event => {
    if (event.key === sessionKey) { returnHome(); return; }
    if (event.key === key || event.key === null) { preference = saved(); apply(); }
  });
  media.addEventListener('change', () => { if (preference === 'system') apply(); });
})();
