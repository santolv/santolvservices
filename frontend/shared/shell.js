// Shared by Inicio, Drive, Fotos and Cuenta.
(() => {
  'use strict';
  const key = 'santolv.theme';
  const sessionKey = 'santolv.session-ended';
  const topbarKey = 'santolv.topbar-hidden';
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
  function applyTopbar() {
    let hidden = false;
    try { hidden = sessionStorage.getItem(topbarKey) === 'true'; } catch {}
    document.documentElement.toggleAttribute('data-topbar-hidden', hidden);
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
  function avatar(element, profile) {
    const words = (profile.display_name || profile.username).trim().split(/\s+/u);
    const letters = words.length > 1 ? [Array.from(words[0])[0], Array.from(words.at(-1))[0]] : Array.from(words[0]).slice(0, 2);
    element.textContent = letters.join('').toLocaleUpperCase('es');
    element.dataset.avatarColor = profile.avatar_color || 'blue';
  }
  window.SantolvShell = {
    setTheme,
    avatar,
    session(expiresAt) {
      if (!Number.isFinite(expiresAt)) return;
      clearTimeout(sessionTimer);
      sessionTimer = setTimeout(returnHome, Math.max(0, expiresAt * 1000 - Date.now()));
    },
    sessionEnded() {
      window.SantolvCalendar?.session(null);
      try { localStorage.setItem(sessionKey, String(Date.now())); } catch {}
    },
    account(profile, handlers) {
      account = handlers;
      window.SantolvCalendar?.session(profile);
      const trigger = document.querySelector('.shell-profile');
      if (!trigger) return;
      trigger.hidden = !profile;
      if (profile) {
        document.getElementById('shell-username').textContent = profile.display_name || profile.username;
        avatar(trigger, profile);
      }
    },
    toggleTheme() { setTheme(document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark'); },
    preference() { return preference; }
  };
  function bind() {
    const hideTopbar = document.getElementById('shell-hide-topbar');
    const showTopbar = document.getElementById('shell-show-topbar');
    function toggleTopbar(hidden, event) {
      try { sessionStorage.setItem(topbarKey, String(hidden)); } catch {}
      document.documentElement.toggleAttribute('data-topbar-hidden', hidden);
      if (event.detail === 0) (hidden ? showTopbar : hideTopbar).focus();
    }
    hideTopbar?.addEventListener('click', event => toggleTopbar(true, event));
    showTopbar?.addEventListener('click', event => toggleTopbar(false, event));
    for (const button of document.querySelectorAll('[data-shell-theme]')) {
      button.addEventListener('click', window.SantolvShell.toggleTheme);
    }
    for (const button of document.querySelectorAll('[data-shell-sidebar]')) {
      button.addEventListener('click', () => {
        const app = button.closest('.app');
        const opening = button.dataset.shellSidebar === 'show';
        if (window.matchMedia('(max-width:620px)').matches) {
          app.classList.remove('sidebar-hidden');
          app.classList.toggle('mobile-sidebar', opening);
        } else {
          app.classList.toggle('sidebar-hidden', !opening);
        }
        requestAnimationFrame(() => {
          const next = opening ? 'hide' : 'show';
          app.querySelector(`[data-shell-sidebar="${next}"]`)?.focus();
        });
      });
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
  applyTopbar();
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
