// Left Sidebar - File Explorer, Search, Tags, Bookmarks
import { api } from './utils.js';

let openNoteCallback = null;

const escapeHtml = (s) => String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');

export const initSidebarLeft = (containerEl, onOpenNote) => {
  openNoteCallback = onOpenNote;
  containerEl.innerHTML = '';

  // Vault header
  const header = document.createElement('div');
  header.className = 'sidebar-header';
  header.innerHTML = `
    <div class="vault-name">Super Cerebro</div>
    <div class="actions">
      <button class="btn icon" title="New note">✚</button>
      <button class="btn icon" title="New folder">📁</button>
      <button class="btn icon" title="More">⋯</button>
    </div>
  `;
  containerEl.appendChild(header);

  // Search input
  const searchInput = document.createElement('input');
  searchInput.type = 'text';
  searchInput.placeholder = 'Buscar notas...';
  searchInput.className = 'input';
  searchInput.style.width = 'calc(100% - 16px)';
  searchInput.style.margin = '8px';
  searchInput.style.borderRadius = '6px';
  containerEl.appendChild(searchInput);

  // Tree container
  const tree = document.createElement('div');
  tree.id = 'tree';
  tree.style.flex = '1';
  tree.style.overflow = 'auto';
  tree.style.padding = '0 4px';
  containerEl.appendChild(tree);

  let files = [];

  const loadTree = async () => {
    try { files = await api('/api/files'); } catch { files = []; }
    renderTree(searchInput.value);
  };

  const renderTree = (filter) => {
    tree.innerHTML = '';
    const q = filter.toLowerCase();
    const filtered = files.filter(f => !q || f.path.toLowerCase().includes(q) || f.name.toLowerCase().includes(q));

    // Build nested structure
    const buildNodes = (items, prefix = '') => {
      const nodes = [];
      const dirs = {};
      const fileNodes = [];

      items.forEach(f => {
        if (f.is_dir) {
          const parts = f.path.split('/');
          const top = parts[0];
          if (!dirs[top]) dirs[top] = [];
          dirs[top].push({ ...f, path: parts.slice(1).join('/') });
        } else {
          fileNodes.push(f);
        }
      });

      // Sort: dirs first, then files
      Object.keys(dirs).sort().forEach(d => {
        const dirNode = document.createElement('div');
        dirNode.className = 'tree-item dir';
        dirNode.innerHTML = `<span>📁</span><span style="flex:1">${escapeHtml(d)}</span>`;
        dirNode.style.cursor = 'pointer';
        dirNode.onclick = () => { /* toggle expand */ };
        tree.appendChild(dirNode);

        const children = document.createElement('div');
        children.style.paddingLeft = '16px';
        children.style.display = 'none';
        buildNodes(dirs[d], prefix + d + '/').forEach(c => children.appendChild(c));
        tree.appendChild(children);
      });

      fileNodes.sort((a, b) => a.name.localeCompare(b.name)).forEach(f => {
        const item = document.createElement('div');
        item.className = 'tree-item';
        item.innerHTML = `<span>📄</span><span style="flex:1">${escapeHtml(f.name)}</span>`;
        item.title = f.path;
        item.style.cursor = 'pointer';
        item.onclick = (e) => {
          e.stopPropagation();
          if (openNoteCallback) openNoteCallback(f.path);
        };
        // Context menu
        item.oncontextmenu = (e) => {
          e.preventDefault();
          showContextMenu(e.clientX, e.clientY, f);
        };
        tree.appendChild(item);
      });

      return nodes;
    };

    // Flat render for now
    filtered.forEach(f => {
      if (f.is_dir) return; // dirs handled above
      const item = document.createElement('div');
      item.className = 'tree-item';
      item.innerHTML = `<span>📄</span><span style="flex:1">${escapeHtml(f.name)}</span>`;
      item.title = f.path;
      item.style.cursor = 'pointer';
      item.onclick = (e) => {
        e.stopPropagation();
        if (openNoteCallback) openNoteCallback(f.path);
      };
      item.oncontextmenu = (e) => {
        e.preventDefault();
        showContextMenu(e.clientX, e.clientY, f);
      };
      tree.appendChild(item);
    });
  };

  const showContextMenu = (x, y, file) => {
    // Remove existing
    document.querySelectorAll('.context-menu').forEach(m => m.remove());
    const menu = document.createElement('div');
    menu.className = 'context-menu';
    menu.style.left = x + 'px';
    menu.style.top = y + 'px';
    const items = [
      { label: 'Open in new pane', icon: '🖼', action: () => { window.openNote && window.openNote(file.path); } },
      { label: 'Rename', icon: '✎', shortcut: 'F2', action: () => { } },
      { label: 'Delete', icon: '🗑', danger: true, action: () => {
        if (confirm('Delete ' + file.path + '?')) {
          fetch('/api/file?path=' + encodeURIComponent(file.path), { method: 'DELETE' }).then(() => loadTree());
        }
      } },
      { label: 'Copy path', icon: '📋', action: () => { navigator.clipboard.writeText(file.path); } },
      { separator: true },
      { label: 'Add to favorites', icon: '★', action: () => { } },
    ];
    items.forEach(it => {
      if (it.separator) {
        const sep = document.createElement('div');
        sep.className = 'menu-separator';
        menu.appendChild(sep);
        return;
      }
      const mi = document.createElement('div');
      mi.className = 'menu-item';
      mi.style.display = 'flex';
      mi.style.alignItems = 'center';
      mi.style.gap = '8px';
      mi.innerHTML = `<span>${it.icon}</span><span style="flex:1">${escapeHtml(it.label)}</span>${it.shortcut ? '<span style="font-size:11px;color:var(--text-muted);margin-left:auto">'+it.shortcut+'</span>' : ''}`;
      if (it.danger) mi.style.color = 'var(--text-error)';
      mi.onclick = (e) => { e.stopPropagation(); it.action(); menu.remove(); };
      menu.appendChild(mi);
    });
    document.body.appendChild(menu);
    const esc = (e) => { if (e.key === 'Escape') { menu.remove(); document.removeEventListener('keydown', esc); } };
    document.addEventListener('keydown', esc);
    document.addEventListener('click', (e) => { if (!menu.contains(e.target)) menu.remove(); }, { once: true });
  };

  searchInput.oninput = (e) => renderTree(e.target.value);

  // Header actions
  header.querySelector('button[title="New note"]').onclick = async () => {
    const name = prompt('Note name:');
    if (name) {
      const path = name.endsWith('.md') ? name : name + '.md';
      await api('/api/file', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path, content: '# ' + name.replace(/\.md$/, '') + '\n\n' })
      });
      loadTree();
      if (openNoteCallback) openNoteCallback(path);
    }
  };

  header.querySelector('button[title="New folder"]').onclick = async () => {
    const name = prompt('Folder name:');
    if (name) {
      await api('/api/folder', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path: name }) });
      loadTree();
    }
  };

  loadTree();
  return { refresh: loadTree };
};
