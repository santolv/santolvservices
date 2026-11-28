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

// Shared macOS-style context menu. Each service decides which items apply and
// passes its own handlers, so every option runs the service's existing logic.
(() => {
  'use strict';
  const paths = {
    open: '<path d="M9 2.5h4.5V7M13.5 2.5 7.5 8.5M12 9.5v3a1 1 0 0 1-1 1H3.5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h3"/>',
    edit: '<path d="m10.5 3 2.5 2.5L5.5 13H3v-2.5z"/>',
    info: '<circle cx="8" cy="8" r="6"/><path d="M8 7.3V11M8 5v.1"/>',
    trash: '<path d="M2.5 4h11M6 4V2.5h4V4M4 4l.6 9.2h6.8L12 4M6.7 6.5v4.5M9.3 6.5v4.5"/>',
    archive: '<rect x="2" y="2.5" width="12" height="3" rx="1"/><path d="M3 5.5V13a1 1 0 0 0 1 1h8a1 1 0 0 0 1-1V5.5M6.5 8.5h3"/>',
    restore: '<path d="M3 8a5 5 0 1 0 1.6-3.7M3 2.5v3h3"/>',
    star: '<path d="m8 2 1.8 3.8 4.1.5-3 2.9.8 4.1L8 11.3l-3.7 2 .8-4.1-3-2.9 4.1-.5z"/>',
    heart: '<path d="M8 13.5S2.5 10.3 2.5 6.3A2.8 2.8 0 0 1 8 4.8a2.8 2.8 0 0 1 5.5 1.5c0 4-5.5 7.2-5.5 7.2z"/>',
    mail: '<rect x="2" y="3.5" width="12" height="9" rx="1.5"/><path d="m2.5 4.5 5.5 4 5.5-4"/>',
    phone: '<path d="M5.5 2.5 4 2.5A1.5 1.5 0 0 0 2.5 4c0 5.2 4.3 9.5 9.5 9.5A1.5 1.5 0 0 0 13.5 12v-1.5l-2.6-1-1.3 1.3a7 7 0 0 1-3.4-3.4L7.5 6.1z"/>',
    download: '<path d="M8 2.5v8M5 7.5l3 3 3-3M2.5 11v2.5h11V11"/>',
    copy: '<rect x="5" y="5" width="8.5" height="8.5" rx="1.5"/><path d="M3 10.5V3.5A1 1 0 0 1 4 2.5h7"/>',
    check: '<circle cx="8" cy="8" r="6"/><path d="m5.5 8 1.8 1.8 3.2-3.5"/>',
    circle: '<circle cx="8" cy="8" r="6"/>',
    plus: '<path d="M8 3v10M3 8h10"/>',
    calendar: '<rect x="2" y="3" width="12" height="10.5" rx="2"/><path d="M2 6.5h12M5.5 2v2M10.5 2v2"/>',
    folder: '<path d="M1.8 4.5A1.5 1.5 0 0 1 3.3 3h2.7l1.5 1.7h5.2a1.5 1.5 0 0 1 1.5 1.5v5.3a1.5 1.5 0 0 1-1.5 1.5H3.3a1.5 1.5 0 0 1-1.5-1.5z"/>',
    share: '<path d="M8 10V2.5M5 5.2l3-2.7 3 2.7M4 7.5H3.5a1 1 0 0 0-1 1v4a1 1 0 0 0 1 1h9a1 1 0 0 0 1-1v-4a1 1 0 0 0-1-1H12"/>',
    refresh: '<path d="M13 8a5 5 0 1 1-1.6-3.7M13 2.5v3h-3"/>',
    album: '<rect x="2.5" y="4" width="11" height="9.5" rx="1.5"/><path d="M4.5 2.5h7M5 11l2.2-2.6 1.6 1.8 1.2-1.3L12 11"/>',
    reply: '<path d="M6.5 4 2.5 8l4 4M2.5 8H10a3.5 3.5 0 0 1 3.5 3.5V13"/>'
  };
  const icon = name => paths[name]
    ? `<svg class="santolv-menu-icon" viewBox="0 0 16 16" aria-hidden="true">${paths[name]}</svg>`
    : '<span class="santolv-menu-icon" aria-hidden="true"></span>';
  let current = null;
  function close(restore = true) {
    if (!current) return;
    const { menu, opener } = current;
    current = null;
    menu.classList.remove('open');
    const remove = () => menu.remove();
    if (matchMedia('(prefers-reduced-motion: reduce)').matches) remove();
    else { menu.addEventListener('transitionend', remove, { once: true }); setTimeout(remove, 200); }
    if (restore && opener?.isConnected) opener.focus({ preventScroll: true });
  }
  function open(event, items, options = {}) {
    const entries = items.filter(Boolean).filter(item => item === '-' || !item.hidden);
    if (!entries.some(item => item !== '-')) return false;
    event?.preventDefault?.();
    close(false);
    const menu = document.createElement('div');
    menu.className = 'santolv-menu';
    menu.setAttribute('role', 'menu');
    menu.setAttribute('aria-label', options.label || 'Acciones');
    let previousSeparator = true;
    entries.forEach((item, index) => {
      if (item === '-') {
        const last = index === entries.length - 1;
        if (!previousSeparator && !last) menu.append(Object.assign(document.createElement('hr'), { role: 'separator' }));
        previousSeparator = true;
        return;
      }
      previousSeparator = false;
      const button = document.createElement('button');
      button.type = 'button';
      button.setAttribute('role', 'menuitem');
      button.innerHTML = icon(item.icon);
      button.append(Object.assign(document.createElement('span'), { textContent: item.label }));
      if (item.danger) button.classList.add('danger');
      if (item.disabled) button.disabled = true;
      button.addEventListener('click', () => {
        close(false);
        try { const result = item.action?.(); result?.catch?.(error => console.error(error)); }
        catch (error) { console.error(error); }
      });
      menu.append(button);
    });
    document.body.append(menu);
    const anchor = event?.target?.getBoundingClientRect?.();
    const x = event?.clientX || anchor?.left || 12;
    const y = event?.clientY || anchor?.bottom || 12;
    const left = Math.max(8, Math.min(x, innerWidth - menu.offsetWidth - 8));
    const top = Math.max(8, Math.min(y, innerHeight - menu.offsetHeight - 8));
    menu.style.left = left + 'px';
    menu.style.top = top + 'px';
    menu.style.transformOrigin = `${x - left}px ${y - top}px`;
    current = { menu, opener: document.activeElement };
    requestAnimationFrame(() => menu.classList.add('open'));
    menu.querySelector('button:not(:disabled)')?.focus({ preventScroll: true });
    return true;
  }
  document.addEventListener('pointerdown', event => { if (current && !current.menu.contains(event.target)) close(false); }, true);
  document.addEventListener('keydown', event => {
    if (!current) return;
    const buttons = [...current.menu.querySelectorAll('button:not(:disabled)')];
    let index = buttons.indexOf(document.activeElement);
    if (event.key === 'Escape') { event.preventDefault(); close(); }
    else if (event.key === 'Tab') { event.preventDefault(); }
    else if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      event.preventDefault();
      index = event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1
        : (index + (event.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length;
      buttons[index]?.focus();
    }
  }, true);
  for (const type of ['resize', 'blur']) window.addEventListener(type, () => close(false));
  document.addEventListener('scroll', () => close(false), true);
  /** Opens the menu built by `build(event)`; returning nothing keeps the browser menu. */
  function bind(root, build, options) {
    // The keyboard menu key and Shift+F10 also fire "contextmenu", without coordinates.
    const handler = event => {
      if (event.target.closest('input,textarea,select,[contenteditable="true"]')) return;
      const items = build(event);
      if (items && open(event, items, options)) event.preventDefault();
    };
    root.addEventListener('contextmenu', handler);
  }
  window.SantolvShell = Object.assign(window.SantolvShell || {}, { contextMenu: open, closeContextMenu: close, bindContextMenu: bind });
})();

// Shared Selector. Every native <select> in the suite is kept (value, form, validation and
// listeners keep working) but shown through one custom trigger and listbox. The menu lives
// in the top layer (popover), so no overflow:hidden container can clip it, and it opens over
// the trigger with the trigger's width, upwards when there is no room below.
(() => {
  'use strict';
  if (!('HTMLSelectElement' in window)) return;
  const proto = HTMLSelectElement.prototype;
  const valueProperty = Object.getOwnPropertyDescriptor(proto, 'value');
  const indexProperty = Object.getOwnPropertyDescriptor(proto, 'selectedIndex');
  const reduced = () => matchMedia('(prefers-reduced-motion: reduce)').matches;
  let open = null, uid = 0;
  const chevron = '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="m4 6 4 4 4-4"/></svg>';

  function labelOf(select) {
    return select.getAttribute('aria-label') || select.labels?.[0]?.textContent.trim() || select.name || 'Seleccionar';
  }
  function enhance(select) {
    if (select.dataset.selector || select.multiple || select.size > 1 || select.dataset.native !== undefined) return;
    select.dataset.selector = 'true';
    const parentWidth = select.parentElement?.clientWidth || 0;
    const wide = !select.offsetWidth || select.offsetWidth >= parentWidth - 4;
    const trigger = document.createElement('button');
    trigger.type = 'button';
    trigger.className = 'santolv-select' + (wide ? ' santolv-select-wide' : '');
    trigger.id = 'santolv-select-' + (++uid);
    trigger.setAttribute('role', 'combobox');
    trigger.setAttribute('aria-haspopup', 'listbox');
    trigger.setAttribute('aria-expanded', 'false');
    if (!wide && select.offsetWidth) trigger.style.minWidth = select.offsetWidth + 'px';
    trigger.innerHTML = '<span class="santolv-select-value"></span>' + chevron;
    select.after(trigger);
    select.classList.add('santolv-select-native');
    select.tabIndex = -1;
    select.setAttribute('aria-hidden', 'true');
    const sync = () => {
      const option = select.options[select.selectedIndex];
      const value = trigger.querySelector('.santolv-select-value');
      value.textContent = option ? option.textContent : '';
      value.classList.toggle('placeholder', !option || option.value === '' && !option.textContent.trim());
      trigger.setAttribute('aria-label', labelOf(select) + ': ' + (option ? option.textContent : ''));
      trigger.hidden = select.hidden;
      trigger.disabled = select.disabled;
      if (open?.select === select) render();
    };
    Object.defineProperty(select, 'value', {configurable: true, get() { return valueProperty.get.call(this); }, set(value) { valueProperty.set.call(this, value); sync(); }});
    Object.defineProperty(select, 'selectedIndex', {configurable: true, get() { return indexProperty.get.call(this); }, set(value) { indexProperty.set.call(this, value); sync(); }});
    select._santolvSync = sync;
    select.addEventListener('change', sync);
    select.addEventListener('input', sync);
    select.addEventListener('focus', () => trigger.focus());
    select.form?.addEventListener('reset', () => setTimeout(sync));
    new MutationObserver(sync).observe(select, {childList: true, subtree: true, characterData: true, attributes: true, attributeFilter: ['hidden', 'disabled', 'selected', 'label']});
    trigger.addEventListener('click', () => (open?.select === select ? close() : show(select, trigger)));
    trigger.addEventListener('keydown', event => {
      if (['ArrowDown', 'ArrowUp', 'Enter', ' '].includes(event.key) && open?.select !== select) {
        event.preventDefault();
        show(select, trigger);
      }
    });
    sync();
  }

  function enabledOptions() { return [...open.menu.querySelectorAll('[role=option]:not([aria-disabled=true])')]; }
  function render() {
    const {select, menu} = open;
    menu.replaceChildren(...[...select.options].map((option, index) => {
      const item = document.createElement('div');
      item.className = 'santolv-select-option';
      item.id = open.trigger.id + '-option-' + index;
      item.setAttribute('role', 'option');
      item.dataset.index = index;
      item.textContent = option.textContent;
      item.setAttribute('aria-selected', String(index === select.selectedIndex));
      if (option.disabled || option.hidden) item.setAttribute('aria-disabled', 'true');
      if (option.hidden) item.hidden = true;
      return item;
    }));
  }
  function activate(item) {
    for (const other of open.menu.querySelectorAll('.active')) other.classList.remove('active');
    if (!item) return;
    item.classList.add('active');
    open.trigger.setAttribute('aria-activedescendant', item.id);
    item.scrollIntoView({block: 'nearest'});
  }
  function place() {
    const {trigger, menu} = open;
    const rect = trigger.getBoundingClientRect();
    menu.style.width = rect.width + 'px';
    menu.style.left = rect.left + 'px';
    menu.style.maxHeight = 'none';
    const below = innerHeight - rect.top - 8, above = rect.bottom - 8;
    const natural = menu.offsetHeight;
    const up = natural > below && above > below;
    menu.style.maxHeight = Math.min(natural, up ? above : below, 320) + 'px';
    menu.style.top = (up ? rect.bottom - menu.offsetHeight : rect.top) + 'px';
    menu.dataset.direction = up ? 'up' : 'down';
  }
  function show(select, trigger) {
    close(false);
    if (trigger.disabled) return;
    const menu = document.createElement('div');
    menu.className = 'santolv-select-menu';
    menu.id = trigger.id + '-listbox';
    menu.setAttribute('role', 'listbox');
    menu.setAttribute('aria-label', labelOf(select));
    menu.popover = 'manual';
    (select.closest('dialog[open]') || document.body).append(menu);
    open = {select, trigger, menu};
    render();
    menu.showPopover();
    place();
    trigger.setAttribute('aria-expanded', 'true');
    trigger.setAttribute('aria-controls', menu.id);
    activate(menu.querySelector('[aria-selected=true]:not([aria-disabled=true])') || enabledOptions()[0]);
    menu.addEventListener('pointermove', event => {
      const item = event.target.closest('[role=option]:not([aria-disabled=true])');
      if (item && !item.classList.contains('active')) activate(item);
    });
    menu.addEventListener('click', event => {
      const item = event.target.closest('[role=option]:not([aria-disabled=true])');
      if (item) choose(Number(item.dataset.index));
    });
    requestAnimationFrame(() => menu.classList.add('open'));
  }
  function choose(index) {
    const {select, trigger} = open;
    close();
    if (index === select.selectedIndex) return;
    indexProperty.set.call(select, index);
    select._santolvSync();
    select.dispatchEvent(new Event('input', {bubbles: true}));
    select.dispatchEvent(new Event('change', {bubbles: true}));
    trigger.focus({preventScroll: true});
  }
  function close(restore = true) {
    if (!open) return;
    const {trigger, menu} = open;
    open = null;
    trigger.setAttribute('aria-expanded', 'false');
    trigger.removeAttribute('aria-activedescendant');
    menu.classList.remove('open');
    const remove = () => { try { menu.hidePopover(); } catch {} menu.remove(); };
    if (reduced()) remove(); else setTimeout(remove, 120);
    if (restore) trigger.focus({preventScroll: true});
  }
  document.addEventListener('keydown', event => {
    if (!open) return;
    const items = enabledOptions();
    const current = items.indexOf(open.menu.querySelector('.active'));
    const step = {ArrowDown: 1, ArrowUp: -1}[event.key];
    if (step) { event.preventDefault(); activate(items[Math.min(items.length - 1, Math.max(0, current + step))]); }
    else if (event.key === 'Home' || event.key === 'End') { event.preventDefault(); activate(items[event.key === 'Home' ? 0 : items.length - 1]); }
    else if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); const item = items[current]; if (item) choose(Number(item.dataset.index)); }
    else if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(); }
    else if (event.key === 'Tab') close(false);
    else if (event.key.length === 1) {
      const letter = event.key.toLocaleLowerCase('es');
      const match = items.slice(current + 1).concat(items.slice(0, current + 1)).find(item => item.textContent.trim().toLocaleLowerCase('es').startsWith(letter));
      if (match) activate(match);
    }
  }, true);
  document.addEventListener('pointerdown', event => {
    if (open && !open.menu.contains(event.target) && !open.trigger.contains(event.target)) close(false);
  }, true);
  addEventListener('resize', () => close(false));
  document.addEventListener('scroll', event => { if (open && !open.menu.contains(event.target)) close(false); }, true);
  document.addEventListener('close', () => close(false), true);

  const scan = root => { for (const select of (root.matches?.('select') ? [root] : root.querySelectorAll?.('select') || [])) enhance(select); };
  function start() {
    scan(document);
    new MutationObserver(records => {
      for (const record of records) for (const node of record.addedNodes) if (node.nodeType === 1) scan(node);
    }).observe(document.body, {childList: true, subtree: true});
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start, {once: true}); else start();
  window.SantolvShell = Object.assign(window.SantolvShell || {}, {enhanceSelect: enhance});
})();
