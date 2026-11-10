// Status bar
export const initStatusBar = (containerEl) => {
  containerEl.innerHTML = '';
  const left = document.createElement('div');
  left.style.display = 'flex';
  left.style.gap = '16px';
  left.style.fontSize = '12px';
  left.style.color = 'var(--text-muted)';
  left.innerHTML = '<span id="status-words">📝 0 words</span><span id="status-lines">📄 0 lines</span>';
  containerEl.appendChild(left);

  const right = document.createElement('div');
  right.style.marginLeft = 'auto';
  right.style.display = 'flex';
  right.style.gap = '16px';
  right.style.fontSize = '12px';
  right.style.color = 'var(--text-muted)';
  right.innerHTML = '<span>UTF-8</span><span>LF</span><span id="status-time">📍 00:00</span>';
  containerEl.appendChild(right);

  const updateTime = () => {
    document.getElementById('status-time').textContent = '📍 ' + new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  };
  setInterval(updateTime, 10000);
  updateTime();

  return {
    update: (words, lines) => {
      document.getElementById('status-words').textContent = '📝 ' + words + ' words';
      document.getElementById('status-lines').textContent = '📄 ' + lines + ' lines';
    },
  };
};

export const updateStatus = (msg) => {
  const el = document.getElementById('status-text');
  if (el) el.textContent = msg;
};
