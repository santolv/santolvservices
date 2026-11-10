// Graph view modal
import { openModal } from './modal.js';
import { api } from './utils.js';

export const openGraph = () => {
  const container = document.createElement('div');
  container.style.width = '800px';
  container.style.height = '600px';
  container.style.display = 'flex';
  container.style.flexDirection = 'column';

  const toolbar = document.createElement('div');
  toolbar.style.display = 'flex';
  toolbar.style.gap = '8px';
  toolbar.style.padding = '8px 12px';
  toolbar.style.borderBottom = '1px solid var(--border-color)';
  toolbar.style.background = 'var(--bg-secondary)';
  toolbar.innerHTML = `
    <input class="input" style="width:200px;font-size:12px;padding:4px 8px" placeholder="Filter nodes...">
    <select class="input" style="font-size:12px;padding:4px 8px"><option>Global</option><option>Local</option></select>
    <button class="btn" style="font-size:11px;padding:4px 8px">Layout</button>
    <button class="btn" style="font-size:11px;padding:4px 8px">Export PNG</button>
  `;
  container.appendChild(toolbar);

  const canvasWrap = document.createElement('div');
  canvasWrap.style.flex = '1';
  canvasWrap.style.overflow = 'auto';
  canvasWrap.style.background = 'var(--bg-primary)';
  const canvas = document.createElement('canvas');
  canvas.width = 800;
  canvas.height = 600;
  canvasWrap.appendChild(canvas);
  container.appendChild(canvasWrap);

  openModal({
    title: 'Graph view',
    content: container,
    closeOnBackdrop: true,
  });

  // Fetch graph data and draw
  api('/api/graph').then(data => drawGraph(canvas, data)).catch(() => {});
};

const drawGraph = (canvas, data) => {
  const ctx = canvas.getContext('2d');
  const w = canvas.width, h = canvas.height;
  ctx.fillStyle = 'var(--bg-primary)';
  ctx.fillRect(0, 0, w, h);

  const nodes = data.nodes || [];
  const edges = data.edges || [];

  // Simple force layout: place nodes in a circle
  const cx = w / 2, cy = h / 2, r = Math.min(w, h) * 0.35;
  const positions = {};
  nodes.forEach((n, i) => {
    const angle = (i / nodes.length) * Math.PI * 2;
    positions[n.id] = { x: cx + Math.cos(angle) * r, y: cy + Math.sin(angle) * r };
  });

  // Draw edges
  ctx.strokeStyle = 'var(--border-color)';
  ctx.lineWidth = 1;
  ctx.globalAlpha = 0.5;
  edges.forEach(e => {
    const a = positions[e.source], b = positions[e.target];
    if (a && b) {
      ctx.beginPath();
      ctx.moveTo(a.x, a.y);
      ctx.lineTo(b.x, b.y);
      ctx.stroke();
    }
  });
  ctx.globalAlpha = 1;

  // Draw nodes
  nodes.forEach(n => {
    const p = positions[n.id];
    const isCurrent = n.id === window.currentPath;
    ctx.fillStyle = isCurrent ? 'var(--text-accent)' : 'var(--text-muted)';
    ctx.beginPath();
    ctx.arc(p.x, p.y, isCurrent ? 10 : 6, 0, Math.PI * 2);
    ctx.fill();
    ctx.strokeStyle = 'var(--bg-primary)';
    ctx.lineWidth = 2;
    ctx.stroke();
    if (isCurrent) {
      ctx.strokeStyle = 'var(--text-accent)';
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.arc(p.x, p.y, 14, 0, Math.PI * 2);
      ctx.stroke();
    }
    ctx.fillStyle = 'var(--text-normal)';
    ctx.font = '11px sans-serif';
    ctx.textAlign = 'center';
    ctx.fillText(n.title || n.id, p.x, p.y + 20);
  });
};
