// Base Modal Component - Obsidian style
export const openModal = ({ title, content, footer, onClose, closeOnBackdrop = true }) => {
  const backdrop = document.createElement('div');
  backdrop.className = 'modal-backdrop';
  backdrop.style.opacity = '0';

  const modal = document.createElement('div');
  modal.className = 'modal';

  const header = document.createElement('div');
  header.className = 'modal-header';
  const hTitle = document.createElement('div');
  hTitle.className = 'modal-title';
  hTitle.textContent = title;
  const closeBtn = document.createElement('button');
  closeBtn.className = 'btn icon';
  closeBtn.innerHTML = '&times;';
  closeBtn.setAttribute('aria-label', 'Cerrar');
  closeBtn.onclick = () => close();
  header.appendChild(hTitle);
  header.appendChild(closeBtn);

  const contentEl = document.createElement('div');
  contentEl.className = 'modal-content';
  if (typeof content === 'string') contentEl.innerHTML = content;
  else contentEl.appendChild(content);

  const footerEl = document.createElement('div');
  footerEl.className = 'modal-footer';
  if (footer) {
    if (Array.isArray(footer)) {
      footer.forEach(b => {
        const btn = document.createElement('button');
        btn.className = 'btn' + (b.primary ? ' primary' : '') + (b.secondary ? ' secondary' : '');
        btn.textContent = b.label;
        btn.onclick = (e) => { e.stopPropagation(); if (b.onclick) b.onclick(e); if (!b.keepOpen) close(); };
        footerEl.appendChild(btn);
      });
    } else {
      footerEl.appendChild(footer);
    }
  }

  modal.appendChild(header);
  modal.appendChild(contentEl);
  modal.appendChild(footerEl);
  backdrop.appendChild(modal);
  document.getElementById('modals').appendChild(backdrop);

  // Animation
  requestAnimationFrame(() => {
    backdrop.style.opacity = '1';
  });

  const close = () => {
    backdrop.style.opacity = '0';
    setTimeout(() => {
      if (backdrop.parentNode) backdrop.parentNode.removeChild(backdrop);
      if (onClose) onClose();
    }, 200);
  };

  backdrop.addEventListener('click', (e) => {
    if (closeOnBackdrop && e.target === backdrop) close();
  });

  const escHandler = (e) => { if (e.key === 'Escape') close(); };
  document.addEventListener('keydown', escHandler);

  return { el: backdrop, close, modal };
};

export const closeAllModals = () => {
  document.querySelectorAll('.modal-backdrop').forEach(b => b.remove());
  document.removeEventListener('keydown', (e) => {});
};
