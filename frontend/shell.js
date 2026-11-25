// Shared by Inicio, Drive and Fotos, including other open tabs on this origin.
(() => {
  'use strict';
  const key = 'santolv.theme';
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
    toggleTheme() { setTheme(document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark'); },
    preference() { return preference; }
  };
  function bind() {
    for (const button of document.querySelectorAll('[data-shell-theme]')) {
      button.addEventListener('click', window.SantolvShell.toggleTheme);
    }
    apply();
  }
  apply();
  document.addEventListener('DOMContentLoaded', bind, { once: true });
  window.addEventListener('pageshow', () => { preference = saved(); apply(); });
  window.addEventListener('storage', event => {
    if (event.key === key || event.key === null) { preference = saved(); apply(); }
  });
  media.addEventListener('change', () => { if (preference === 'system') apply(); });
})();
