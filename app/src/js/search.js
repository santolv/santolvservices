// Global Search - Ctrl+Shift+F
import { openModal } from './modal.js';
import { api, fuzzyScore, highlightMatch } from './utils.js';

export const openGlobalSearch = () => {
  const input = document.createElement('input');
  input.type = 'text';
  input.placeholder = 'Search all files...';
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

  const filterBar = document.createElement('div');
  filterBar.style.display = 'flex';
  filterBar.style.gap = '6px';
  filterBar.style.padding = '6px 12px';
  filterBar.style.borderBottom = '1px solid var(--border-color)';
  filterBar.style.background = 'var(--bg-tertiary)';
  filterBar.style.borderRadius = '0 0 6px 6px';

  const makeFilterBtn = (label) => {
    const btn = document.createElement('button');
    btn.className = 'btn icon';
    btn.textContent = label;
    btn.style.fontSize = '12px';
    btn.style.padding = '4px 8px';
    return btn;
  };

  filterBar.appendChild(makeFilterBtn('📁'));
  filterBar.appendChild(makeFilterBtn('🏷'));
  filterBar.appendChild(makeFilterBtn('📝'));
  filterBar.appendChild(makeFilterBtn('⚙'));

  const results = document.createElement('div');
  results.style.maxHeight = '350px';
  results.style.overflow = 'auto';
  results.style.display = 'flex';
  results.style.flexDirection = 'column';

  let allResults = [];

  const render = (filter) => {
    results.innerHTML = '';
    const q = filter.toLowerCase();
    const filtered = allResults.filter(r => !q || fuzzyScore(q, r.path) >= 0 || (r.snippet && fuzzyScore(q, r.snippet) >= 0));

    if (filtered.length === 0) {
      const empty = document.createElement('div');
      empty.style.padding = '20px';
      empty.style.color = 'var(--text-muted)';
      empty.style.textAlign = 'center';
      empty.textContent = 'No results';
      results.appendChild(empty);
      return;
    }

    let currentFile = null;
    filtered.forEach(r => {
      if (currentFile !== r.path) {
        currentFile = r.path;
        const fileHeader = document.createElement('div');
        fileHeader.style.padding = '6px 12px';
        fileHeader.style.borderBottom = '1px solid var(--border-color)';
        fileHeader.style.display = 'flex';
        fileHeader.style.alignItems = 'center';
        fileHeader.style.gap = '8px';
        fileHeader.style.cursor = 'pointer';
        fileHeader.style.fontSize = '13px';
        fileHeader.style.color = 'var(--text-normal)';
        fileHeader.innerHTML = `<span>📄</span><span style="flex:1">${escapeHtml(r.path)}</span><span style="font-size:11px;color:var(--text-muted);margin-left:auto">${r.matchCount || 1} match${(r.matchCount||1)!==1?'es':''}</span>`;
        fileHeader.onclick = () => { window.openNote && window.openNote(r.path); };
        results.appendChild(fileHeader);
      }

      const item = document.createElement('div');
      item.style.padding = '4px 12px 4px 36px';
      item.style.borderBottom = '1px solid var(--border-color)';
      item.style.cursor = 'pointer';
      item.style.fontSize = '12px';
      item.style.color = 'var(--text-normal)';
      item.innerHTML = `<div style="color:var(--text-muted);font-size:11px">${escapeHtml(r.matchLine || '')}</div><div>${highlightMatch(r.snippet || '', filter)}</div>`;
      item.onclick = () => { window.openNote && window.openNote(r.path); };
      results.appendChild(item);
    });
  };

  const { el, close } = openModal({
    title: 'Search',
    content: [input, filterBar, results],
    closeOnBackdrop: true,
  });

  input.focus();

  const doSearch = async () => {
    try {
      const q = input.value;
      if (!q) { allResults = []; render(''); return; }
      const data = await api('/api/search?q=' + encodeURIComponent(q));
      allResults = data || [];
      render(q);
    } catch (e) {
      allResults = [];
      render('');
    }
  };

  input.oninput = debounce(doSearch, 150);
  doSearch();

  return { el, close };
};

const debounce = (fn, wait) => {
  let t;
  return (...args) => { clearTimeout(t); t = setTimeout(() => fn(...args), wait); };
};
