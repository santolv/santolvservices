// Command Palette - Ctrl+P
import { openModal, closeAllModals } from './modal.js';
import { api, fuzzyScore, highlightMatch } from './utils.js';

const COMMANDS = [
  { id: 'file.new', category: 'File', label: 'New note', shortcut: 'Ctrl+N', icon: '📄', action: () => openQuickSwitcher({ create: true }) },
  { id: 'file.new-folder', category: 'File', label: 'New folder', shortcut: 'Ctrl+Shift+N', icon: '📁', action: () => { } },
  { id: 'file.open', category: 'File', label: 'Open note', shortcut: 'Ctrl+O', icon: '📂', action: () => openQuickSwitcher() },
  { id: 'file.save', category: 'File', label: 'Save', shortcut: 'Ctrl+S', icon: '💾', action: () => { if (window.saveCurrentNote) window.saveCurrentNote(); } },
  { id: 'file.close', category: 'File', label: 'Close tab', shortcut: 'Ctrl+W', icon: '✕', action: () => { } },
  { id: 'edit.undo', category: 'Edit', label: 'Undo', shortcut: 'Ctrl+Z', icon: '↶', action: () => { } },
  { id: 'edit.redo', category: 'Edit', label: 'Redo', shortcut: 'Ctrl+Y', icon: '↷', action: () => { } },
  { id: 'view.toggle-sidebar', category: 'View', label: 'Toggle left sidebar', shortcut: 'Ctrl+\\', icon: '◀', action: () => { } },
  { id: 'view.toggle-preview', category: 'View', label: 'Toggle preview', shortcut: 'Ctrl+E', icon: '👁', action: () => { } },
  { id: 'search.global', category: 'Search', label: 'Search in all files', shortcut: 'Ctrl+Shift+F', icon: '🔍', action: () => openGlobalSearch() },
  { id: 'search.current', category: 'Search', label: 'Search in current file', shortcut: 'Ctrl+F', icon: '🔎', action: () => { } },
  { id: 'graph.open', category: 'Graph', label: 'Open graph view', shortcut: '', icon: '𕠀', action: () => { if (window.openGraph) window.openGraph(); } },
  { id: 'canvas.open', category: 'Canvas', label: 'Open canvas', shortcut: '', icon: '🖼', action: () => { if (window.openCanvas) window.openCanvas(); } },
  { id: 'settings.open', category: 'Settings', label: 'Settings', shortcut: 'Ctrl+,', icon: '⚙', action: () => openSettings() },
  { id: 'help.shortcuts', category: 'Help', label: 'Keyboard shortcuts', shortcut: '', icon: '⌨', action: () => openSettings('hotkeys') },
];

export const openCommandPalette = () => {
  closeAllModals();
  const input = document.createElement('input');
  input.type = 'text';
  input.placeholder = 'Commands...';
  input.className = 'input';
  input.style.width = '100%';
  input.style.fontSize = '14px';
  input.style.padding = '10px 12px';
  input.style.borderRadius = '8px 8px 0 0';
  input.style.border = 'none';
  input.style.borderBottom = '1px solid var(--border-color)';
  input.style.background = 'var(--bg-secondary)';
  input.style.color = 'var(--text-normal)';
  input.style.outline = 'none';

  const list = document.createElement('div');
  list.style.maxHeight = '300px';
  list.style.overflow = 'auto';
  list.style.display = 'flex';
  list.style.flexDirection = 'column';

  let selectedIdx = 0;

  const render = (filter) => {
    list.innerHTML = '';
    const filtered = COMMANDS.filter(c => !filter || fuzzyScore(filter, c.label) >= 0 || fuzzyScore(filter, c.category) >= 0 || fuzzyScore(filter, c.id) >= 0);
    filtered.forEach((cmd, i) => {
      const item = document.createElement('div');
      item.className = 'menu-item';
      item.style.padding = '8px 12px';
      item.style.borderRadius = '0';
      item.style.borderBottom = '1px solid var(--border-color)';
      item.style.cursor = 'pointer';
      item.style.display = 'flex';
      item.style.justifyContent = 'space-between';
      item.style.alignItems = 'center';
      if (i === selectedIdx) { item.style.background = 'var(--bg-hover)'; }

      const left = document.createElement('div');
      left.style.display = 'flex';
      left.style.alignItems = 'center';
      left.style.gap = '8px';
      left.style.flex = '1';
      const icon = document.createElement('span');
      icon.textContent = cmd.icon;
      icon.style.fontSize = '14px';
      const label = document.createElement('span');
      label.innerHTML = filter ? highlightMatch(cmd.label, filter) : escapeHtml(cmd.label);
      left.appendChild(icon);
      left.appendChild(label);

      const right = document.createElement('span');
      right.textContent = cmd.shortcut;
      right.style.fontSize = '11px';
      right.style.color = 'var(--text-muted)';
      right.style.marginLeft = 'auto';

      const cat = document.createElement('span');
      cat.textContent = cmd.category;
      cat.style.fontSize = '11px';
      cat.style.color = 'var(--text-faint)';
      cat.style.marginRight = '8px';

      item.appendChild(left);
      item.appendChild(cat);
      item.appendChild(right);
      item.onclick = () => { closeAllModals(); cmd.action(); };
      list.appendChild(item);
    });
    if (filtered.length > 0) {
      const first = list.children[selectedIdx];
      if (first) first.style.background = 'var(--bg-hover)';
    }
  };

  const footer = document.createElement('div');
  footer.style.padding = '8px 12px';
  footer.style.borderTop = '1px solid var(--border-color)';
  footer.style.fontSize = '11px';
  footer.style.color = 'var(--text-muted)';
  footer.textContent = '↑↓ navigate • Enter execute • Esc close';

  const { el, close } = openModal({
    title: 'Commands',
    content: [input, list, footer],
    closeOnBackdrop: true,
  });

  input.focus();
  render('');

  input.oninput = (e) => { render(e.target.value); selectedIdx = 0; render(e.target.value); };
  input.onkeydown = (e) => {
    if (e.key === 'ArrowDown') { e.preventDefault(); selectedIdx = Math.min(selectedIdx + 1, list.children.length - 1); render(input.value); }
    if (e.key === 'ArrowUp') { e.preventDefault(); selectedIdx = Math.max(0, selectedIdx - 1); render(input.value); }
    if (e.key === 'Enter') { e.preventDefault(); const item = list.children[selectedIdx]; if (item) item.onclick(); }
    if (e.key === 'Escape') { close(); }
  };

  return { el, close };
};

export const openQuickSwitcher = ({ create } = {}) => {
  closeAllModals();
  const input = document.createElement('input');
  input.type = 'text';
  input.placeholder = create ? 'Create new note...' : 'Open note...';
  input.className = 'input';
  input.style.width = '100%';
  input.style.fontSize = '14px';
  input.style.padding = '10px 12px';
  input.style.borderRadius = '8px 8px 0 0';
  input.style.border = 'none';
  input.style.borderBottom = '1px solid var(--border-color)';
  input.style.background = 'var(--bg-secondary)';
  input.style.color = 'var(--text-normal)';
  input.style.outline = 'none';

  const list = document.createElement('div');
  list.style.maxHeight = '300px';
  list.style.overflow = 'auto';
  list.style.display = 'flex';
  list.style.flexDirection = 'column';

  let files = [];
  let selectedIdx = 0;

  const loadFiles = async () => {
    try { files = await api('/api/files'); } catch { files = []; }
    render(input.value);
  };

  const render = (filter) => {
    list.innerHTML = '';
    const q = filter.toLowerCase();
    const filtered = files.filter(f => !f.is_dir && (!q || fuzzyScore(q, f.path) >= 0 || fuzzyScore(q, f.name) >= 0));
    filtered.sort((a, b) => {
      const sa = fuzzyScore(q, a.path), sb = fuzzyScore(q, b.path);
      return sb - sa;
    }).slice(0, 50).forEach((f, i) => {
      const item = document.createElement('div');
      item.className = 'tree-item';
      item.style.height = '32px';
      item.style.padding = '4px 12px';
      item.style.borderRadius = '0';
      item.style.borderBottom = '1px solid var(--border-color)';
      item.style.cursor = 'pointer';
      item.style.display = 'flex';
      item.style.alignItems = 'center';
      item.style.gap = '8px';
      if (i === selectedIdx) item.style.background = 'var(--bg-hover)';

      const icon = document.createElement('span');
      icon.textContent = '📄';
      const name = document.createElement('span');
      name.innerHTML = filter ? highlightMatch(f.name, filter) : escapeHtml(f.name);
      name.style.flex = '1';
      const path = document.createElement('span');
      path.textContent = f.path;
      path.style.fontSize = '11px';
      path.style.color = 'var(--text-muted)';
      path.style.marginLeft = 'auto';

      item.appendChild(icon);
      item.appendChild(name);
      item.appendChild(path);
      item.onclick = () => { closeAllModals(); window.openNote && window.openNote(f.path); };
      list.appendChild(item);
    });

    if (create && filtered.length === 0 && q) {
      const item = document.createElement('div');
      item.className = 'tree-item';
      item.style.height = '32px';
      item.style.padding = '4px 12px';
      item.style.borderRadius = '0';
      item.style.borderBottom = '1px solid var(--border-color)';
      item.style.cursor = 'pointer';
      item.style.display = 'flex';
      item.style.alignItems = 'center';
      item.style.gap = '8px';
      item.style.background = 'var(--bg-hover)';

      const icon = document.createElement('span');
      icon.textContent = '✚';
      const name = document.createElement('span');
      name.textContent = `Create "${q}"`;
      name.style.flex = '1';
      item.appendChild(icon);
      item.appendChild(name);
      item.onclick = () => {
        closeAllModals();
        const path = q.endsWith('.md') ? q : q + '.md';
        api('/api/file', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ path, content: '# ' + q + '\n\n' })
        }).then(() => { window.openNote && window.openNote(path); });
      };
      list.appendChild(item);
    }

    if (filtered.length > 0) list.children[selectedIdx].style.background = 'var(--bg-hover)';
  };

  const footer = document.createElement('div');
  footer.style.padding = '8px 12px';
  footer.style.borderTop = '1px solid var(--border-color)';
  footer.style.fontSize = '11px';
  footer.style.color = 'var(--text-muted)';
  footer.textContent = '↑↓ navigate • Enter open • Shift+Enter new pane • Esc close';

  const { el, close } = openModal({
    title: create ? 'New note' : 'Quick switcher',
    content: [input, list, footer],
    closeOnBackdrop: true,
  });

  input.focus();
  loadFiles();

  input.oninput = (e) => { render(e.target.value); selectedIdx = 0; render(e.target.value); };
  input.onkeydown = (e) => {
    if (e.key === 'ArrowDown') { e.preventDefault(); selectedIdx = Math.min(selectedIdx + 1, list.children.length - 1); render(input.value); }
    if (e.key === 'ArrowUp') { e.preventDefault(); selectedIdx = Math.max(0, selectedIdx - 1); render(input.value); }
    if (e.key === 'Enter') { e.preventDefault(); const item = list.children[selectedIdx]; if (item) item.onclick(); }
    if (e.key === 'Escape') { close(); }
  };

  return { el, close };
};
