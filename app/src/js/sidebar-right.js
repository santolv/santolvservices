// Right Sidebar - Backlinks, Outline, Properties, Graph
import { api } from './utils.js';

export const initSidebarRight = (containerEl, onOpenNote) => {
  containerEl.innerHTML = '';

  let currentPathGlobal = () => window.currentPath || null;
  let currentNoteName = () => {
    const p = window.currentPath || null;
    return p ? p.split('/').pop().replace(/\.md$/, '') : null;
  };

  // Vertical icon tabs
  const tabBar = document.createElement('div');
  tabBar.style.width = '36px';
  tabBar.style.display = 'flex';
  tabBar.style.flexDirection = 'column';
  tabBar.style.borderRight = '1px solid var(--border-color)';
  tabBar.style.background = 'var(--bg-secondary)';

  const tabs = [
    { id: 'backlinks', icon: '🔗', label: 'Backlinks' },
    { id: 'outline', icon: '📋', label: 'Outline' },
    { id: 'properties', icon: '🏷', label: 'Properties' },
    { id: 'graph', icon: '🕸', label: 'Graph' },
  ];

  let activeTab = 'backlinks';

  const panel = document.createElement('div');
  panel.style.flex = '1';
  panel.style.overflow = 'auto';
  panel.style.background = 'var(--bg-secondary)';
  panel.style.padding = '12px';

  const renderTab = (id) => {
    panel.innerHTML = '';
    tabBar.querySelectorAll('.tab-btn').forEach(b => {
      b.style.background = b.dataset.id === id ? 'var(--bg-active)' : 'transparent';
      b.style.borderRight = b.dataset.id === id ? '3px solid var(--text-accent)' : 'none';
    });

    if (id === 'backlinks') renderBacklinks();
    if (id === 'outline') renderOutline();
    if (id === 'properties') renderProperties();
    if (id === 'graph') renderGraph();
  };

  tabs.forEach(t => {
    const btn = document.createElement('button');
    btn.className = 'tab-btn';
    btn.dataset.id = t.id;
    btn.style.width = '36px';
    btn.style.height = '36px';
    btn.style.display = 'flex';
    btn.style.alignItems = 'center';
    btn.style.justifyContent = 'center';
    btn.style.fontSize = '18px';
    btn.style.cursor = 'pointer';
    btn.style.border = 'none';
    btn.style.background = activeTab === t.id ? 'var(--bg-active)' : 'transparent';
    btn.style.color = 'var(--text-muted)';
    btn.style.borderRight = activeTab === t.id ? '3px solid var(--text-accent)' : 'none';
    btn.title = t.label;
    btn.textContent = t.icon;
    btn.onclick = () => { activeTab = t.id; renderTab(t.id); };
    tabBar.appendChild(btn);
  });

  containerEl.appendChild(tabBar);
  containerEl.appendChild(panel);

  const escapeHtml = (s) => String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');

  // Backlinks
  const renderBacklinks = async () => {
    panel.innerHTML = '';
    const header = document.createElement('div');
    header.style.display = 'flex';
    header.style.justifyContent = 'space-between';
    header.style.alignItems = 'center';
    header.style.marginBottom = '8px';
    header.innerHTML = '<div style="font-weight:600;font-size:13px">Backlinks</div><input class="input" style="width:120px;font-size:11px;padding:2px 6px" placeholder="Filter">';
    panel.appendChild(header);

    const list = document.createElement('div');
    list.style.display = 'flex';
    list.style.flexDirection = 'column';
    list.style.gap = '4px';
    panel.appendChild(list);

    const name = currentNoteName();
    if (!name) {
      list.innerHTML = '<div style="padding:12px;color:var(--text-muted);text-align:center">Open a note to see backlinks</div>';
      return;
    }

    try {
      const links = await api('/api/backlinks?name=' + encodeURIComponent(name));
      if (!links || links.length === 0) {
        list.innerHTML = '<div style="padding:12px;color:var(--text-muted);text-align:center">Sin backlinks</div>';
        return;
      }
      links.forEach(l => {
        const item = document.createElement('div');
        item.style.padding = '6px 8px';
        item.style.borderRadius = '4px';
        item.style.cursor = 'pointer';
        item.style.fontSize = '12px';
        item.style.color = 'var(--text-normal)';
        item.innerHTML = `<span>📄</span><span style="margin-left:6px">${escapeHtml(l)}</span>`;
        item.onmouseenter = () => item.style.background = 'var(--bg-hover)';
        item.onmouseleave = () => item.style.background = 'transparent';
        item.onclick = () => { onOpenNote && onOpenNote(l); };
        list.appendChild(item);
      });
    } catch { list.innerHTML = '<div style="color:var(--text-error)">Error loading backlinks</div>'; }
  };

  // Outline
  const renderOutline = async () => {
    panel.innerHTML = '';
    const header = document.createElement('div');
    header.style.marginBottom = '8px';
    header.innerHTML = '<div style="font-weight:600;font-size:13px">Outline</div>';
    panel.appendChild(header);

    const list = document.createElement('div');
    list.style.display = 'flex';
    list.style.flexDirection = 'column';
    list.style.gap = '2px';
    panel.appendChild(list);

    const path = currentPathGlobal();
    if (!path) {
      list.innerHTML = '<div style="padding:12px;color:var(--text-muted);text-align:center">Open a note to see outline</div>';
      return;
    }

    try {
      const headings = await api('/api/outline?path=' + encodeURIComponent(path));
      if (!headings || headings.length === 0) {
        list.innerHTML = '<div style="padding:12px;color:var(--text-muted);text-align:center">No headings</div>';
        return;
      }
      headings.forEach(h => {
        const item = document.createElement('div');
        const indent = (h.level - 1) * 16;
        item.style.padding = '2px 8px';
        item.style.paddingLeft = (8 + indent) + 'px';
        item.style.borderRadius = '4px';
        item.style.cursor = 'pointer';
        item.style.fontSize = '12px';
        item.style.color = 'var(--text-muted)';
        item.style.borderLeft = '2px solid transparent';
        item.innerHTML = '#'.repeat(h.level) + ' ' + escapeHtml(h.text);
        item.onmouseenter = () => { item.style.background = 'var(--bg-hover)'; item.style.color = 'var(--text-normal)'; };
        item.onmouseleave = () => { item.style.background = 'transparent'; item.style.color = 'var(--text-muted)'; };
        item.onclick = () => { onOpenNote && onOpenNote(path); };
        list.appendChild(item);
      });
    } catch { list.innerHTML = '<div style="color:var(--text-error)">Error loading outline</div>'; }
  };

  // Properties
  const renderProperties = async () => {
    panel.innerHTML = '';
    const header = document.createElement('div');
    header.style.marginBottom = '8px';
    header.innerHTML = '<div style="font-weight:600;font-size:13px">Properties</div>';
    panel.appendChild(header);

    const path = currentPathGlobal();
    if (!path) {
      panel.innerHTML += '<div style="padding:12px;color:var(--text-muted);text-align:center">Open a note to see properties</div>';
      return;
    }

    try {
      const props = await api('/api/properties?path=' + encodeURIComponent(path));
      const keys = Object.keys(props || {});
      if (keys.length === 0) {
        panel.innerHTML += '<div style="padding:12px;color:var(--text-muted);text-align:center">No properties</div>';
        return;
      }
      keys.forEach(k => {
        const row = document.createElement('div');
        row.style.display = 'flex';
        row.style.alignItems = 'center';
        row.style.padding = '4px 8px';
        row.style.borderBottom = '1px solid var(--border-color)';
        row.style.fontSize = '12px';
        const key = document.createElement('span');
        key.textContent = k;
        key.style.color = 'var(--text-muted)';
        key.style.minWidth = '100px';
        key.style.fontSize = '11px';
        const val = document.createElement('span');
        val.textContent = Array.isArray(props[k]) ? props[k].join(', ') : String(props[k]);
        val.style.flex = '1';
        val.style.color = 'var(--text-normal)';
        row.appendChild(key);
        row.appendChild(val);
        panel.appendChild(row);
      });
    } catch { panel.innerHTML += '<div style="color:var(--text-error)">Error loading properties</div>'; }
  };

  // Graph
  const renderGraph = () => {
    panel.innerHTML = '';
    const header = document.createElement('div');
    header.style.display = 'flex';
    header.style.justifyContent = 'space-between';
    header.style.alignItems = 'center';
    header.style.marginBottom = '8px';
    header.innerHTML = '<div style="font-weight:600;font-size:13px">Graph</div><button class="btn" style="font-size:11px;padding:4px 8px">⚙</button>';
    panel.appendChild(header);

    const canvas = document.createElement('canvas');
    canvas.width = 260;
    canvas.height = 300;
    canvas.style.width = '100%';
    canvas.style.height = '100%';
    canvas.style.background = 'var(--bg-primary)';
    canvas.style.borderRadius = '6px';
    panel.appendChild(canvas);

    drawGraph(canvas);
  };

  const drawGraph = (canvas) => {
    const ctx = canvas.getContext('2d');
    ctx.fillStyle = 'var(--bg-primary)';
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.strokeStyle = 'var(--border-color)';
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.arc(canvas.width/2, canvas.height/2, 60, 0, Math.PI*2);
    ctx.stroke();
    ctx.fillStyle = 'var(--text-accent)';
    ctx.beginPath();
    ctx.arc(canvas.width/2, canvas.height/2, 10, 0, Math.PI*2);
    ctx.fill();
    ctx.fillStyle = 'var(--text-muted)';
    ctx.font = '11px sans-serif';
    ctx.textAlign = 'center';
    ctx.fillText('Graph', canvas.width/2, canvas.height/2 + 80);
  };

  renderTab(activeTab);

  return { refresh: () => renderTab(activeTab), setPath: (p) => { currentPathGlobal = () => p; } };
};
