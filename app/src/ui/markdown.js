import { marked } from 'marked';
import DOMPurify from 'dompurify';
import { decodeMention, fileName, referenceLabel, referenceUrl } from './references.js';

export const escape = value => String(value).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
export const title = path => path.split('/').pop().replace(/\.(md|canvas|base)$/i, '');
export const wikiTargets = content => [...content.matchAll(/(?<!@)\[\[([^\]\n]+)\]\]/g)].map(m => m[1].split('|')[0].split('#')[0].trim()).filter(Boolean);

export function renderFileReference(path) {
  return `<a class="sc-reference" href="${escape(referenceUrl(path))}" target="_blank" rel="noopener" title="${escape(path)}"><span class="sc-reference-icon" aria-hidden="true">@</span><span class="sc-reference-text"><strong>${escape(fileName(path))}</strong><small>${escape(referenceLabel(path))}</small></span><span class="sc-reference-arrow" aria-hidden="true">↗</span></a>`;
}

export function renderMarkdown(content) {
  const code = [];
  const references = [];
  let text = content.replace(/^---\r?\n[\s\S]*?\r?\n---(?:\r?\n|$)/, '');
  // Protect fenced and inline code from wikilink / tag substitution.
  text = text.replace(/```[^\n]*\n[\s\S]*?```|`[^`\n]+`/g, value => `SC_CODE_${code.push(value) - 1}_END`);
  text = text.replace(/^>\s*\[!([^\]]+)\]\s*([^\n]*)$/gm, (_, kind, label) => '> **' + (label || ({note:'Nota',tip:'Idea',info:'Información',warning:'Atención',success:'Hecho'}[kind] || kind)) + '**\n>');
  text = text.replace(/(@)?\[\[([^\]\n]+)\]\]/g, (match, mention, raw) => {
    if (mention) {
      const path = decodeMention(raw);
      return path ? "SC_REF_" + (references.push(path) - 1) + "_END" : escape(match);
    }
    const [target, alias] = raw.split('|');
    return `<a class="internal-link" data-note="${escape(target)}" href="#">${escape(alias || target)}</a>`;
  });
  text = text.replace(/(^|\s)#([\p{L}\p{N}_/-]+)/gu, '$1<a class="tag" data-tag="$2" href="#">#$2</a>');
  text = text.replace(/SC_CODE_(\d+)_END/g, (_, n) => code[Number(n)]);
  const html = marked.parse(text, { gfm: true, breaks: false })
    .replace(/SC_REF_(\d+)_END/g, (token, index) => references[index] ? renderFileReference(references[index]) : token);
  return DOMPurify.sanitize(html, { ADD_ATTR: ['data-note', 'data-tag', 'target'], FORBID_TAGS: ['style', 'iframe', 'form'], FORBID_ATTR: ['style'] });
}
