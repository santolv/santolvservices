// Layout: Tab bar, workspace, split panes, status bar
export const tabs = [];
let activeTabId = null;

export const initLayout = (containerEl) => {
  const tabBar = document.createElement('div');
  tabBar.className = 'tab-bar';
  containerEl.appendChild(tabBar);

  const editorArea = document.createElement('div');
  editorArea.className = 'editor-area';
  editorArea.style.flex = '1';
  editorArea.style.overflow = 'hidden';
  editorArea.style.display = 'flex';
  editorArea.style.flexDirection = 'column';
  containerEl.appendChild(editorArea);

  const escapeHtml = (s) => String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');

  const addTab = (path, title) => {
    const existing = tabs.find(t => t.path === path);
    if (existing) { setActiveTab(existing.id); return existing; }

    const id = 'tab-' + Date.now();
    const tab = { id, path, title: title || path, modified: false };
    tabs.push(tab);

    const el = document.createElement('div');
    el.className = 'tab';
    el.style.height = '32px';
    el.style.display = 'flex';
    el.style.alignItems = 'center';
    el.style.padding = '0 8px';
    el.style.gap = '4px';
    el.style.borderBottom = activeTabId === id ? '2px solid var(--text-accent)' : '1px solid transparent';
    el.style.background = activeTabId === id ? 'var(--bg-active)' : 'transparent';
    el.style.color = activeTabId === id ? 'var(--text-normal)' : 'var(--text-muted)';
    el.style.fontSize = '13px';
    el.style.cursor = 'pointer';
    el.style.whiteSpace = 'nowrap';
    el.style.overflow = 'hidden';
    el.innerHTML = `<span>📄</span><span style="flex:1;overflow:hidden;text-overflow:ellipsis">${escapeHtml(tab.title)}${tab.modified ? ' •' : ''}</span><button class="btn icon" style="font-size:10px;padding:2px;margin-left:4px">✕</button>`;

    el.onclick = (e) => {
      if (e.target.tagName === 'BUTTON') { e.stopPropagation(); closeTab(id); return; }
      setActiveTab(id);
    };

    tabBar.appendChild(el);
    tab.el = el;
    return tab;
  };

  const closeTab = (id) => {
    const idx = tabs.findIndex(t => t.id === id);
    if (idx < 0) return;
    const tab = tabs[idx];
    if (tab.el) tab.el.remove();
    tabs.splice(idx, 1);
    if (activeTabId === id) {
      setActiveTab(tabs.length > 0 ? tabs[Math.max(0, idx - 1)].id : null);
    }
  };

  const setActiveTab = (id) => {
    activeTabId = id;
    tabs.forEach(t => {
      if (t.el) {
        const active = t.id === id;
        t.el.style.borderBottom = active ? '2px solid var(--text-accent)' : '1px solid transparent';
        t.el.style.background = active ? 'var(--bg-active)' : 'transparent';
        t.el.style.color = active ? 'var(--text-normal)' : 'var(--text-muted)';
      }
    });
    if (id) {
      const tab = tabs.find(t => t.id === id);
      if (tab && window.openNote) window.openNote(tab.path);
    } else {
      editorArea.innerHTML = '<div style="padding:40px;text-align:center;color:var(--text-muted)">Open a note to start</div>';
    }
  };

  // New tab button
  const newBtn = document.createElement('button');
  newBtn.className = 'btn icon';
  newBtn.textContent = '+';
  newBtn.style.fontSize = '16px';
  newBtn.style.padding = '0 6px';
  newBtn.onclick = () => { window.openQuickSwitcher && window.openQuickSwitcher({ create: true }); };
  tabBar.appendChild(newBtn);

  return { addTab, setActiveTab, tabs, tabBar, editorArea, closeTab };
};

// Named exports for index.html imports
export { addTab, setActiveTab as setActive };
