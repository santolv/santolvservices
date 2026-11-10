// Canvas view - infinite canvas with cards
export const openCanvas = () => {
  const container = document.createElement('div');
  container.style.width = '900px';
  container.style.height = '700px';
  container.style.display = 'flex';
  container.style.flexDirection = 'column';

  const toolbar = document.createElement('div');
  toolbar.style.display = 'flex';
  toolbar.style.gap = '6px';
  toolbar.style.padding = '6px 12px';
  toolbar.style.borderBottom = '1px solid var(--border-color)';
  toolbar.style.background = 'var(--bg-secondary)';
  const tools = [
    { label: '📄 Note', title: 'Note card' },
    { label: '🔗 Edge', title: 'Create edge' },
    { label: '📝 Text', title: 'Text card' },
    { label: '🖼 Image', title: 'Image card' },
    { label: '🌐 Web', title: 'Web card' },
    { label: '🎨 Group', title: 'Group' },
    { label: '🔍', title: 'Search' },
    { label: '📐 Grid', title: 'Toggle grid' },
    { label: '📥', title: 'Import' },
    { label: '📤', title: 'Export' },
    { label: '⚙', title: 'Settings' },
  ];
  tools.forEach(t => {
    const btn = document.createElement('button');
    btn.className = 'btn icon';
    btn.textContent = t.label;
    btn.title = t.title;
    btn.style.fontSize = '14px';
    btn.style.padding = '4px 8px';
    toolbar.appendChild(btn);
  });
  container.appendChild(toolbar);

  const wrap = document.createElement('div');
  wrap.style.flex = '1';
  wrap.style.overflow = 'hidden';
  wrap.style.background = 'var(--bg-primary)';
  wrap.style.position = 'relative';

  const canvas = document.createElement('div');
  canvas.style.width = '3000px';
  canvas.style.height = '3000px';
  canvas.style.position = 'relative';
  canvas.style.cursor = 'grab';
  canvas.style.transformOrigin = 'top left';
  wrap.appendChild(canvas);
  container.appendChild(wrap);

  // Add a sample card
  const addCard = (x, y, text) => {
    const card = document.createElement('div');
    card.style.position = 'absolute';
    card.style.left = x + 'px';
    card.style.top = y + 'px';
    card.style.width = '200px';
    card.style.background = 'var(--bg-secondary)';
    card.style.border = '1px solid var(--border-color)';
    card.style.borderRadius = '8px';
    card.style.padding = '12px';
    card.style.color = 'var(--text-normal)';
    card.style.fontSize = '13px';
    card.style.cursor = 'grab';
    card.innerHTML = `<div style="font-weight:600;margin-bottom:6px">${text}</div><div style="color:var(--text-muted);font-size:11px">Card</div>`;
    canvas.appendChild(card);

    let dragging = false;
    card.onmousedown = (e) => {
      if (e.target !== card) return;
      dragging = true;
      card.style.cursor = 'grabbing';
      const startX = e.clientX - card.offsetLeft;
      const startY = e.clientY - card.offsetTop;
      const move = (ev) => {
        if (!dragging) return;
        card.style.left = (ev.clientX - startX) + 'px';
        card.style.top = (ev.clientY - startY) + 'px';
      };
      const up = () => { dragging = false; card.style.cursor = 'grab'; document.removeEventListener('mousemove', move); document.removeEventListener('mouseup', up); };
      document.addEventListener('mousemove', move);
      document.addEventListener('mouseup', up);
    };
    return card;
  };

  addCard(100, 100, 'Welcome');
  addCard(350, 100, 'Super Cerebro');
  addCard(100, 300, 'Canvas view');

  // Pan
  let panning = false;
  wrap.onmousedown = (e) => {
    if (e.target !== wrap) return;
    panning = true;
    wrap.style.cursor = 'grabbing';
    const startX = e.clientX;
    const startY = e.clientY;
    const startTransform = canvas.style.transform;
    const move = (ev) => {
      if (!panning) return;
      const dx = ev.clientX - startX;
      const dy = ev.clientY - startY;
      canvas.style.transform = `translate(${dx}px, ${dy}px)`;
    };
    const up = () => { panning = false; wrap.style.cursor = 'default'; document.removeEventListener('mousemove', move); document.removeEventListener('mouseup', up); };
    document.addEventListener('mousemove', move);
    document.addEventListener('mouseup', up);
  };

  openModal({
    title: 'Canvas',
    content: container,
    closeOnBackdrop: true,
  });
};
