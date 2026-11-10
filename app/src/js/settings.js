// Settings modal
import { openModal } from './modal.js';

const SECTIONS = [
  { id: 'about', label: 'About', icon: 'ℹ️' },
  { id: 'appearance', label: 'Appearance', icon: '🎨' },
  { id: 'editor', label: 'Editor', icon: '📝' },
  { id: 'files', label: 'Files', icon: '📁' },
  { id: 'hotkeys', label: 'Hotkeys', icon: '⌨' },
  { id: 'plugins', label: 'Core plugins', icon: '🧩' },
  { id: 'community', label: 'Community plugins', icon: '📦' },
  { id: 'advanced', label: 'Advanced', icon: '⚙' },
];

const HOTKEYS = [
  { cmd: 'File: New note', key: 'Ctrl+N' },
  { cmd: 'File: Open note', key: 'Ctrl+O' },
  { cmd: 'File: Save', key: 'Ctrl+S' },
  { cmd: 'Search: Global', key: 'Ctrl+Shift+F' },
  { cmd: 'Graph: Open', key: '' },
  { cmd: 'Canvas: Open', key: '' },
  { cmd: 'Settings: Open', key: 'Ctrl+,' },
];

export const openSettings = (activeTab = 'appearance') => {
  const nav = document.createElement('div');
  nav.style.width = '200px';
  nav.style.borderRight = '1px solid var(--border-color)';
  nav.style.display = 'flex';
  nav.style.flexDirection = 'column';

  SECTIONS.forEach(s => {
    const item = document.createElement('div');
    item.style.padding = '10px 12px';
    item.style.cursor = 'pointer';
    item.style.fontSize = '13px';
    item.style.color = s.id === activeTab ? 'var(--text-accent)' : 'var(--text-normal)';
    item.style.background = s.id === activeTab ? 'var(--bg-tertiary)' : 'transparent';
    item.style.borderRight = s.id === activeTab ? '2px solid var(--text-accent)' : 'none';
    item.style.display = 'flex';
    item.style.alignItems = 'center';
    item.style.gap = '8px';
    item.textContent = s.icon + ' ' + s.label;
    item.onclick = () => { activeTab = s.id; renderPanel(); renderNav(); };
    nav.appendChild(item);
  });

  const renderNav = () => {
    nav.innerHTML = '';
    SECTIONS.forEach(s => {
      const item = document.createElement('div');
      item.style.padding = '10px 12px';
      item.style.cursor = 'pointer';
      item.style.fontSize = '13px';
      item.style.color = s.id === activeTab ? 'var(--text-accent)' : 'var(--text-normal)';
      item.style.background = s.id === activeTab ? 'var(--bg-tertiary)' : 'transparent';
      item.style.borderRight = s.id === activeTab ? '2px solid var(--text-accent)' : 'none';
      item.style.display = 'flex';
      item.style.alignItems = 'center';
      item.style.gap = '8px';
      item.textContent = s.icon + ' ' + s.label;
      item.onclick = () => { activeTab = s.id; renderPanel(); renderNav(); };
      nav.appendChild(item);
    });
  };

  const renderPanel = () => {
    panel.innerHTML = '';
    const header = document.createElement('div');
    header.style.padding = '16px 24px';
    header.style.borderBottom = '1px solid var(--border-color)';
    header.style.fontSize = '18px';
    header.style.fontWeight = '600';
    header.style.color = 'var(--text-normal)';
    header.textContent = SECTIONS.find(s => s.id === activeTab).label;
    panel.appendChild(header);

    const content = document.createElement('div');
    content.style.padding = '24px';
    content.style.maxHeight = '500px';
    content.style.overflow = 'auto';

    if (activeTab === 'appearance') {
      content.innerHTML = `
        <div style="margin-bottom:20px"><label style="display:block;font-size:13px;color:var(--text-normal);margin-bottom:6px">Theme</label>
        <div style="display:flex;gap:8px">
          <button class="btn${activeTheme==='dark'?' primary':''}" onclick="setTheme('dark')">Dark</button>
          <button class="btn${activeTheme==='light'?' primary':''}" onclick="setTheme('light')">Light</button>
          <button class="btn${activeTheme==='auto'?' primary':''}" onclick="setTheme('auto')">System</button>
        </div></div>
        <div style="margin-bottom:20px"><label style="display:block;font-size:13px;color:var(--text-normal);margin-bottom:6px">Base font</label>
        <select class="input" style="width:200px;padding:6px"><option>Default</option><option>Serif</option><option>Mono</option></select></div>
        <div style="margin-bottom:20px"><label style="display:block;font-size:13px;color:var(--text-normal);margin-bottom:6px">Accent color</label>
        <input type="color" value="#7f6df2" style="width:40px;height:32px;border:none;cursor:pointer"/></div>
        <div style="margin-bottom:20px"><label style="display:block;font-size:13px;color:var(--text-normal);margin-bottom:6px">Text size</label>
        <input type="range" min="12" max="24" value="16" style="width:100%"/></div>
        <div style="margin-bottom:20px"><label style="display:block;font-size:13px;color:var(--text-normal);margin-bottom:6px">Line height</label>
        <input type="range" min="1.3" max="2" step="0.1" value="1.6" style="width:100%"/></div>
        <div style="margin-bottom:20px"><label style="display:block;font-size:13px;color:var(--text-normal);margin-bottom:6px">Custom CSS</label>
        <textarea class="input" style="width:100%;height:120px;font-family:monospace;font-size:12px"></textarea></div>
      `;
    } else if (activeTab === 'hotkeys') {
      const table = document.createElement('table');
      table.style.width = '100%';
      table.style.borderCollapse = 'collapse';
      HOTKEYS.forEach(hk => {
        const tr = document.createElement('tr');
        tr.style.borderBottom = '1px solid var(--border-color)';
        tr.innerHTML = `<td style="padding:8px;color:var(--text-normal)">${hk.cmd}</td><td style="padding:8px;color:var(--text-muted)">${hk.key}</td><td style="padding:8px"><button class="btn" style="font-size:11px;padding:4px 8px">Record</button><button class="btn" style="font-size:11px;padding:4px 8px;margin-left:4px">Clear</button></td>`;
        table.appendChild(tr);
      });
      content.appendChild(table);
    } else {
      content.innerHTML = `<div style="color:var(--text-muted);padding:40px 0;text-align:center">Settings for ${SECTIONS.find(s=>s.id===activeTab).label} coming soon</div>`;
    }

    panel.appendChild(content);
  };

  const panel = document.createElement('div');
  panel.style.flex = '1';
  panel.style.display = 'flex';
  panel.style.flexDirection = 'column';
  renderPanel();

  const container = document.createElement('div');
  container.style.display = 'flex';
  container.style.width = '700px';
  container.style.height = '500px';
  container.appendChild(nav);
  container.appendChild(panel);

  openModal({
    title: 'Settings',
    content: container,
    footer: [{ label: 'Close', primary: true, onclick: () => {} }],
    closeOnBackdrop: true,
  });
};

let activeTheme = 'dark';
window.setTheme = (t) => { activeTheme = t; document.documentElement.setAttribute('data-theme', t); };
