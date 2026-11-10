// Editor: Markdown source + live preview
export let currentPath = null;
let autoSaveTimer = null;

export const initEditor = (containerEl, onPathChange) => {
  containerEl.innerHTML = '';

  // Toolbar
  const toolbar = document.createElement('div');
  toolbar.style.display = 'flex';
  toolbar.style.gap = '4px';
  toolbar.style.padding = '6px 12px';
  toolbar.style.borderBottom = '1px solid var(--border-color)';
  toolbar.style.background = 'var(--bg-secondary)';
  const btns = [
    { label: 'B', wrap: '**' },
    { label: 'I', wrap: '_' },
    { label: 'H1', prefix: '# ' },
    { label: 'H2', prefix: '## ' },
    { label: 'List', prefix: '- ' },
    { label: 'Code', wrap: '`' },
    { label: 'Link', insert: '[[note]]' },
  ];
  btns.forEach(b => {
    const btn = document.createElement('button');
    btn.className = 'btn';
    btn.textContent = b.label;
    btn.style.fontSize = '12px';
    btn.style.padding = '4px 8px';
    btn.onclick = () => {
      const start = editor.selectionStart, end = editor.selectionEnd;
      const sel = editor.value.slice(start, end);
      let repl = sel;
      if (b.wrap) repl = b.wrap + sel + b.wrap;
      if (b.prefix) repl = b.prefix + sel;
      if (b.insert) repl = b.insert;
      editor.value = editor.value.slice(0, start) + repl + editor.value.slice(end);
      editor.selectionStart = editor.selectionEnd = start + repl.length;
      editor.focus();
      updatePreview();
      triggerAutoSave();
    };
    toolbar.appendChild(btn);
  });
  containerEl.appendChild(toolbar);

  // Editor + Preview split
  const split = document.createElement('div');
  split.style.display = 'flex';
  split.style.flex = '1';
  split.style.overflow = 'hidden';
  containerEl.appendChild(split);

  const editor = document.createElement('textarea');
  editor.className = 'editor';
  editor.spellcheck = false;
  editor.style.flex = '1';
  editor.style.background = 'var(--bg-primary)';
  editor.style.color = 'var(--text-normal)';
  editor.style.fontFamily = 'var(--font-mono)';
  editor.style.fontSize = '15px';
  editor.style.lineHeight = '1.6';
  editor.style.padding = '24px';
  editor.style.resize = 'none';
  editor.style.outline = 'none';
  editor.style.border = 'none';
  editor.style.minHeight = '0';
  split.appendChild(editor);

  const preview = document.createElement('div');
  preview.className = 'preview';
  preview.style.flex = '1';
  preview.style.background = 'var(--bg-secondary)';
  preview.style.padding = '24px';
  preview.style.overflow = 'auto';
  preview.style.borderLeft = '1px solid var(--border-color)';
  split.appendChild(preview);

  let previewVisible = true;

  const updatePreview = () => {
    if (!previewVisible) return;
    let html = escapeMd(editor.value);
    // Code blocks
    html = html.replace(/```([\s\S]*?)```/g, (_, c) => `<pre><code>${escapeMd(c)}</code></pre>`);
    // Headings
    html = html.replace(/^### (.*)$/gm, '<h3>$1</h3>');
    html = html.replace(/^## (.*)$/gm, '<h2>$1</h2>');
    html = html.replace(/^# (.*)$/gm, '<h1>$1</h1>');
    // Bold/italic
    html = html.replace(/\*\*(.*?)\*\*/g, '<strong>$1</strong>');
    html = html.replace(/_(.*?)_/g, '<em>$1</em>');
    html = html.replace(/`(.*?)`/g, '<code>$1</code>');
    // Wiki links
    html = html.replace(/\[\[(.+?)\]\]/g, (_, n) => `<a class="wikilink" data-note="${escapeHtml(n)}">${escapeHtml(n)}</a>`);
    // Lists
    html = html.replace(/^- (.*)$/gm, '<li>$1</li>');
    html = html.replace(/(<li>.*<\/li>\n?)+/g, '<ul>$&</ul>');
    // Paragraphs
    html = html.replace(/\n\n/g, '</p><p>');
    html = '<p>' + html + '</p>';
    preview.innerHTML = html;

    preview.querySelectorAll('.wikilink').forEach(a => {
      a.style.color = 'var(--text-accent)';
      a.style.cursor = 'pointer';
      a.style.textDecoration = 'none';
      a.onclick = (e) => { e.preventDefault(); window.openNote && window.openNote(a.dataset.note + '.md'); };
    });
  };

  const escapeMd = (s) => String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');

  editor.addEventListener('input', () => {
    updatePreview();
    triggerAutoSave();
  });

  editor.addEventListener('keydown', (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === 's') { e.preventDefault(); save(); }
  });

  const togglePreview = () => {
    previewVisible = !previewVisible;
    preview.style.display = previewVisible ? 'block' : 'none';
    editor.style.width = previewVisible ? '50%' : '100%';
  };

  const save = async () => {
    if (!currentPath) return;
    try {
      await api('/api/file', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: currentPath, content: editor.value })
      });
      updateStatus('Guardado ✓');
    } catch (e) { updateStatus('Error al guardar'); }
  };

  const triggerAutoSave = () => {
    clearTimeout(autoSaveTimer);
    autoSaveTimer = setTimeout(save, 800);
  };

  const updateStatus = (msg) => {
    const el = document.getElementById('status-text');
    if (el) el.textContent = msg;
    setTimeout(() => { if (el) el.textContent = ''; }, 2000);
  };

  const loadNote = async (path) => {
    currentPath = path;
    try {
      const data = await api('/api/file?path=' + encodeURIComponent(path));
      editor.value = data.content || '';
      document.title = path.split('/').pop() + ' - Super Cerebro';
      if (onPathChange) onPathChange(path);
      updatePreview();
    } catch (e) {
      editor.value = '';
      updateStatus('No se pudo cargar');
    }
  };

  window.openNote = loadNote;
  window.saveCurrentNote = save;
  window.togglePreview = togglePreview;

  return { loadNote, save, updatePreview, editor, preview };
};
