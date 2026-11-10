import { EditorState, Compartment } from '@codemirror/state';
import { EditorView, keymap, Decoration, ViewPlugin, WidgetType, drawSelection, highlightActiveLine, placeholder } from '@codemirror/view';
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
import { markdown, markdownKeymap } from '@codemirror/lang-markdown';
import { syntaxHighlighting, HighlightStyle, indentOnInput, bracketMatching, syntaxTree } from '@codemirror/language';
import { tags } from '@lezer/highlight';
import { search, searchKeymap, highlightSelectionMatches } from '@codemirror/search';
import { autocompletion, completionKeymap } from '@codemirror/autocomplete';
import { atMentionStart, decodeMention, fileMention, pickReference } from './references.js';
import { renderFileReference } from './markdown.js';

class CheckWidget extends WidgetType {
  constructor(checked, pos) { super(); this.checked = checked; this.pos = pos; }
  eq(other) { return this.checked === other.checked && this.pos === other.pos; }
  toDOM(view) {
    const input = document.createElement('input'); input.type = 'checkbox'; input.checked = this.checked;
    input.setAttribute('aria-label', 'Completar tarea');
    input.onmousedown = e => e.preventDefault();
    input.onclick = () => view.dispatch({ changes: { from: this.pos, to: this.pos + 3, insert: this.checked ? '[ ]' : '[x]' } });
    return input;
  }
  ignoreEvent() { return true; }
}

class BulletWidget extends WidgetType {
  toDOM() { const span = document.createElement('span'); span.className = 'cm-list-bullet'; span.textContent = '•'; return span; }
  ignoreEvent() { return false; }
}

class FileReferenceWidget extends WidgetType {
  constructor(path) { super(); this.path = path; }
  eq(other) { return this.path === other.path; }
  toDOM() {
    const wrapper = document.createElement('span');
    wrapper.innerHTML = renderFileReference(this.path);
    return wrapper;
  }
  ignoreEvent() { return true; }
}

function inCode(state, position) {
  for (let node = syntaxTree(state).resolveInner(position, -1); node; node = node.parent) {
    if (node.name.includes('Code')) return true;
  }
  return false;
}

function liveDecorations(view) {
  const ranges = [];
  const active = new Set(view.state.selection.ranges.map(r => view.state.doc.lineAt(r.head).number));
  let fenced = false;
  for (let n = 1; n <= view.state.doc.lines; n++) {
    const line = view.state.doc.line(n), text = line.text, editing = view.hasFocus && active.has(n);
    if (/^```/.test(text)) { fenced = !fenced; ranges.push(Decoration.line({ class: 'cm-fenced' }).range(line.from)); if (!editing && text.length) ranges.push(Decoration.replace({}).range(line.from, line.to)); continue; }
    if (fenced) { ranges.push(Decoration.line({ class: 'cm-fenced' }).range(line.from)); continue; }
    const heading = /^(#{1,6})\s/.exec(text);
    if (heading) {
      ranges.push(Decoration.line({ class: `cm-heading cm-heading-${heading[1].length}` }).range(line.from));
      if (!editing) ranges.push(Decoration.replace({}).range(line.from, line.from + heading[0].length));
    }
    if (/^>\s/.test(text)) { ranges.push(Decoration.line({ class: 'cm-quote' }).range(line.from)); if (!editing) ranges.push(Decoration.replace({}).range(line.from, line.from + 2)); }
    for (const match of text.matchAll(/@\[\[([^\]\n]+)\]\]/g)) {
      const from = line.from + match.index, to = from + match[0].length;
      const path = decodeMention(match[1]);
      if (!path || inCode(view.state, from + 1)) continue;
      if (editing && view.state.selection.ranges.some(range => range.to >= from && range.from < to)) continue;
      ranges.push(Decoration.replace({ widget: new FileReferenceWidget(path) }).range(from, to));
    }
    if (editing) continue;
    for (const match of text.matchAll(/(?<!@)\[\[([^\]\n]+)\]\]/g)) {
      const from = line.from + match.index, inner = match[1], pipe = inner.indexOf('|');
      ranges.push(Decoration.replace({}).range(from, from + (pipe < 0 ? 2 : pipe + 3)));
      ranges.push(Decoration.mark({ class: 'cm-internal-link', attributes: { 'data-note': inner.split('|')[0] } }).range(from + (pipe < 0 ? 2 : pipe + 3), from + match[0].length - 2));
      ranges.push(Decoration.replace({}).range(from + match[0].length - 2, from + match[0].length));
    }
    for (const match of text.matchAll(/(\*\*|~~|`)(.+?)\1/g)) {
      const from = line.from + match.index, len = match[1].length;
      ranges.push(Decoration.replace({}).range(from, from + len));
      ranges.push(Decoration.mark({ class: match[1] === '**' ? 'cm-live-bold' : match[1] === '~~' ? 'cm-live-strike' : 'cm-live-code' }).range(from + len, from + len + match[2].length));
      ranges.push(Decoration.replace({}).range(from + len + match[2].length, from + match[0].length));
    }
    for (const match of text.matchAll(/(?<![\w*])([*_])(?!\1)([^\n]+?)\1(?![\w*])/g)) {
      const from = line.from + match.index;
      ranges.push(Decoration.replace({}).range(from, from + 1));
      ranges.push(Decoration.mark({ class: 'cm-live-italic' }).range(from + 1, from + match[0].length - 1));
      ranges.push(Decoration.replace({}).range(from + match[0].length - 1, from + match[0].length));
    }
    for (const match of text.matchAll(/(?:^|\s)(#[\p{L}\p{N}_/-]+)/gu)) {
      const from = line.from + match.index + match[0].length - match[1].length;
      ranges.push(Decoration.mark({ class: 'cm-tag', attributes: { 'data-tag': match[1].slice(1) } }).range(from, from + match[1].length));
    }
    const check = /^(\s*[-*] )\[([ xX])\]/.exec(text);
    if (check) { const pos = line.from + check[1].length; ranges.push(Decoration.replace({}).range(line.from, pos)); ranges.push(Decoration.replace({ widget: new CheckWidget(check[2] !== ' ', pos) }).range(pos, pos + 3)); }
    else { const bullet = /^(\s*)[-*] /.exec(text); if (bullet) ranges.push(Decoration.replace({ widget: new BulletWidget() }).range(line.from + bullet[1].length, line.from + bullet[0].length)); }
  }
  return Decoration.set(ranges, true);
}

const livePreview = ViewPlugin.fromClass(class {
  constructor(view) { this.decorations = liveDecorations(view); }
  update(update) { if (update.docChanged || update.selectionSet || update.focusChanged || update.viewportChanged) this.decorations = liveDecorations(update.view); }
}, { decorations: value => value.decorations });

const highlights = HighlightStyle.define([
  { tag: tags.heading, color: 'var(--text-normal)', fontWeight: '600' },
  { tag: tags.strong, fontWeight: '700' }, { tag: tags.emphasis, fontStyle: 'italic' },
  { tag: tags.link, color: 'var(--accent-text)' }, { tag: tags.url, color: 'var(--accent-text)' },
  { tag: tags.monospace, color: 'var(--code)' }, { tag: tags.processingInstruction, color: 'var(--text-faint)' },
  { tag: tags.meta, color: 'var(--text-faint)' }, { tag: tags.comment, color: 'var(--text-faint)' },
]);

export function createEditor(parent, content, mode, onChange, onLink, snapshot, getNotes = () => [], getPath = () => '') {
  const modeConfig = snapshot?.modeConfig || new Compartment();
  function mention(view) {
    const { from, to } = view.state.selection.main;
    const original = view.state.doc;
    pickReference({ currentPath: getPath(), onSelect: target => {
      if (!view.dom.isConnected || !view.state.doc.eq(original)) return;
      const insert = fileMention(target) + ' ';
      view.dispatch({ changes: { from, to, insert }, selection: { anchor: from + insert.length } });
      view.focus();
    } });
  }
  const wikiCompletion = context => {
    const match = context.matchBefore(/\[\[[^\]\n]*/);
    if (!match) return null;
    const typed = match.text.slice(2);
    return { from: match.from + 2, options: getNotes().map(path => ({ label: path.replace(/\.md$/i, ''), type: 'text', apply: (view, completion, from, to) => { const after = view.state.sliceDoc(to, to + 2); view.dispatch({ changes: { from, to, insert: completion.label + (after === ']]' ? '' : ']]') }, selection: { anchor: from + completion.label.length + 2 } }); } })), validFor: /^[^\]\n]*$/ };
  };
  const view = new EditorView({ parent, state: snapshot?.state || EditorState.create({ doc: content, extensions: [
    history(), drawSelection(), indentOnInput(), bracketMatching(), EditorView.lineWrapping,
    EditorState.phrases.of({ 'Find': 'Buscar', 'Replace': 'Reemplazar', 'next': 'siguiente', 'previous': 'anterior', 'all': 'todas', 'replace': 'reemplazar', 'replace all': 'reemplazar todo', 'close': 'cerrar', 'match case': 'Distinguir mayúsculas', 'regexp': 'Expresión regular', 'by word': 'Palabra completa', 'No matches': 'Sin coincidencias', 'No completions': 'Sin sugerencias' }),
    markdown(), syntaxHighlighting(highlights), placeholder('Empieza a escribir…'),
    search({ top: true }), highlightSelectionMatches(), autocompletion({ override: [wikiCompletion] }),
    keymap.of([...completionKeymap, ...defaultKeymap, ...historyKeymap, ...markdownKeymap, ...searchKeymap, indentWithTab]),
    modeConfig.of(mode === 'source' ? highlightActiveLine() : livePreview),
    EditorView.updateListener.of(update => { if (update.docChanged) onChange(update.state.doc.toString()); }),
    EditorView.domEventHandlers({ keydown(event, view) {
      if (event.key !== '@' || event.isComposing) return false;
      const { head } = view.state.selection.main;
      const line = view.state.doc.lineAt(head);
      if (inCode(view.state, head) || !atMentionStart(view.state.sliceDoc(line.from, head))) return false;
      event.preventDefault();
      mention(view);
      return true;
    }, click(event) { const link = event.target.closest('[data-note]'); if (link) { event.preventDefault(); onLink(link.dataset.note); return true; } } }),
  ] }) });
  if (snapshot && view.state.doc.toString() !== content.replace(/\r\n?/g, '\n')) view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: content } });
  view.dispatch({ effects: modeConfig.reconfigure(mode === 'source' ? highlightActiveLine() : livePreview) });
  return { view, mention: () => mention(view), snapshot: () => ({ state: view.state, modeConfig }), destroy: () => view.destroy(), mode: value => view.dispatch({ effects: modeConfig.reconfigure(value === 'source' ? highlightActiveLine() : livePreview) }),
    insert(before, after = before) { const { from, to } = view.state.selection.main; const selected = view.state.sliceDoc(from, to); view.dispatch({ changes: { from, to, insert: before + selected + after }, selection: { anchor: from + before.length, head: from + before.length + selected.length } }); view.focus(); },
    line(number) { const line = view.state.doc.line(Math.min(number, view.state.doc.lines)); view.dispatch({ selection: { anchor: line.from }, effects: EditorView.scrollIntoView(line.from, { y: 'start' }) }); view.focus(); },
  };
}
