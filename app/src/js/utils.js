// Utils: helpers genéricos para la app
export const $ = (sel, root = document) => root.querySelector(sel);
export const $$ = (sel, root = document) => Array.from(root.querySelectorAll(sel));

export const escapeHtml = (s) =>
  String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

export const debounce = (fn, wait) => {
  let t;
  return (...args) => {
    clearTimeout(t);
    t = setTimeout(() => fn(...args), wait);
  };
};

export const api = async (path, opts = {}) => {
  const res = await fetch(path, opts);
  if (!res.ok) {
    const ct = res.headers.get('content-type') || '';
    if (ct.includes('json')) {
      const j = await res.json();
      throw new Error(j.error || res.statusText);
    }
    throw new Error(await res.text());
  }
  const ct = res.headers.get('content-type') || '';
  return ct.includes('json') ? res.json() : res.text();
};

export const formatTime = (d = new Date()) =>
  d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

export const fuzzyScore = (query, text) => {
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  let qi = 0;
  for (let ti = 0; ti < t.length; ti++) {
    if (t[ti] === q[qi]) { qi++; if (qi === q.length) break; }
  }
  return qi === q.length ? qi : -1;
};

export const highlightMatch = (text, query) => {
  if (!query) return escapeHtml(text);
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  let out = '';
  let last = 0;
  let qi = 0;
  for (let i = 0; i < t.length; i++) {
    if (t[i] === q[qi]) {
      out += escapeHtml(text.slice(last, i)) + '<mark>' + escapeHtml(text[i]) + '</mark>';
      last = i + 1;
      qi++;
      if (qi === q.length) break;
    }
  }
  out += escapeHtml(text.slice(last));
  return out;
};
