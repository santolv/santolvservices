import { parse, stringify } from 'yaml';
import { marked } from 'marked';

export function createId() {
  return crypto.randomUUID?.() ?? Array.from(crypto.getRandomValues(new Uint8Array(16)),
    byte => byte.toString(16).padStart(2, '0')).join('');
}

export function readPage(content) {
  const match = /^(?:\uFEFF)?---\r?\n([\s\S]*?)\r?\n---(?:\r?\n|$)/.exec(content);
  if (!match) return { meta: {}, body: content, prefix: '', invalid: false };
  try {
    const meta = parse(match[1]) ?? {};
    if (typeof meta !== 'object' || Array.isArray(meta)) throw new Error('Las propiedades deben ser un mapa YAML.');
    return { meta, body: content.slice(match[0].length), prefix: match[0], invalid: false };
  } catch { return { meta: {}, body: content, prefix: '', invalid: true }; }
}

export function writePage(meta, body) {
  return Object.keys(meta).length ? `---\n${stringify(meta, { lineWidth: 0 }).trimEnd()}\n---\n\n${body.replace(/^\n+/, '')}` : body;
}

export function setProperties(content, patch) {
  const page = readPage(content);
  if (page.invalid) throw new Error('El YAML de la página no es válido. Corrígelo en modo fuente antes de cambiar propiedades.');
  const meta = { ...page.meta, ...patch };
  for (const key of Object.keys(meta)) if (meta[key] === undefined) delete meta[key];
  return writePage(meta, page.body);
}

export function splitBlocks(content) {
  const page = readPage(content);
  if (page.invalid) return { ...page, blocks: [{ type: 'raw', raw: content }] };
  const blocks = marked.lexer(page.body).filter(token => token.type !== 'space').flatMap(token => {
    if (token.type === 'list') return token.items.map(item => ({ type: item.task ? 'task' : token.ordered ? 'number' : 'bullet', raw: item.raw.replace(/\n+$/, '') }));
    const type = token.type === 'heading' ? `h${token.depth}` : token.type === 'blockquote' ? (/^>\s*\[!/.test(token.raw) ? 'callout' : 'quote') : token.type === 'hr' ? 'divider' : token.type === 'code' ? 'code' : token.type === 'html' && /^<details/i.test(token.raw) ? 'toggle' : token.type === 'paragraph' ? 'text' : 'raw';
    return [{ type, raw: token.raw.replace(/\n+$/, '') }];
  });
  return { ...page, blocks: blocks.length ? blocks : [{ type: 'text', raw: '' }] };
}

export const blockBody = blocks => blocks.map(block => block.raw).join('\n\n') + (blocks.some(b => b.raw) ? '\n' : '');
export function mergeBlocks(original, blocks) { const page = readPage(original); return page.prefix + blockBody(blocks); }
export function blockText(block) {
  if (/^h[1-6]$/.test(block.type)) return block.raw.replace(/^#{1,6}\s?/, '');
  if (block.type === 'task') return block.raw.replace(/^\s*[-*]\s+\[[ xX]\]\s?/, '');
  if (block.type === 'bullet') return block.raw.replace(/^\s*[-*+]\s?/, '');
  if (block.type === 'number') return block.raw.replace(/^\s*\d+[.)]\s?/, '');
  if (block.type === 'quote') return block.raw.replace(/^>\s?/gm, '');
  if (block.type === 'code') { const match = /^(`{3,}|~{3,})([^\n]*)\n([\s\S]*?)\n?\1$/.exec(block.raw); return match ? match[3] : block.raw.replace(/^ {4}/gm, ''); }
  return block.raw;
}
export function updateBlockText(block, text) {
  if (/^h[1-6]$/.test(block.type)) return '#'.repeat(Number(block.type[1])) + ' ' + text;
  if (block.type === 'task') return '- [' + (/^\s*[-*]\s+\[[xX]\]/.test(block.raw) ? 'x' : ' ') + '] ' + text;
  if (block.type === 'bullet') return '- ' + text;
  if (block.type === 'number') return (block.raw.match(/^\s*(\d+)/)?.[1] || '1') + '. ' + text;
  if (block.type === 'quote') return text.split('\n').map(line => '> ' + line).join('\n');
  if (block.type === 'code') { const language = block.raw.match(/^(`{3,}|~{3,})([^\n]*)/)?.[2] || ''; const runs = [...text.matchAll(/`+/g)].map(match => match[0].length); const fence = '`'.repeat(Math.max(3, ...runs.map(length => length + 1))); return fence + language + '\n' + text + '\n' + fence; }
  return text;
}
export const isPropertyKey = key => /^[\p{L}\p{N}_-]{1,50}$/u.test(key) && !['__proto__','prototype','constructor'].includes(key);
export const propertyKey = label => label.normalize('NFD').replace(/\p{Diacritic}/gu, '').toLowerCase().replace(/[^a-z0-9]+/g, '_').replace(/^_|_$/g, '');
export const pageReservedKeys = new Set(['title','icon','cover','editor','parent','template','_database','archived']);
export function databaseKey(label) { const key=propertyKey(label);return pageReservedKeys.has(key)?'prop_'+key:key; }
export const remapPath = (path, from, to) => typeof path === 'string' && (path === from || path.startsWith(from + '/')) ? to + path.slice(from.length) : path;
export function rewriteKnowledgeReferences(content, from, to, oldPaths) {
  const page = readPage(content);
  const rewrite = text => text.replace(/```[\s\S]*?```|~~~[\s\S]*?~~~|`[^`\n]*`|\[\[([^\]\n]+)\]\]/g, (match, inner) => {
    if (!inner) return match;
    const cut = inner.search(/[#|]/), target = cut < 0 ? inner : inner.slice(0, cut), suffix = cut < 0 ? '' : inner.slice(cut);
    const resolved = oldPaths.find(path => path === target || path === target + '.md') || oldPaths.find(path => path.split('/').pop().replace(/\.md$/, '') === target);
    if (!resolved) return match;
    const mapped = remapPath(resolved,from,to);if(mapped === resolved)return match;
    return `[[${target.endsWith('.md') ? mapped : mapped.replace(/\.md$/, '')}${suffix}]]`;
  });
  if(page.invalid)return rewrite(content);
  const meta={...page.meta};let metaChanged=false;
  for(const key of Object.keys(meta)) {
    if(['parent','_database'].includes(key)){const next=remapPath(meta[key],from,to);if(next!==meta[key]){meta[key]=next;metaChanged=true;}}
    else if(typeof meta[key]==='string'){const next=rewrite(meta[key]);if(next!==meta[key]){meta[key]=next;metaChanged=true;}}
  }
  const body=rewrite(page.body);
  return metaChanged ? writePage(meta,body) : page.prefix + body;
}
export const localDate = (date = new Date()) => `${date.getFullYear()}-${String(date.getMonth()+1).padStart(2,'0')}-${String(date.getDate()).padStart(2,'0')}`;
export function parseCsv(input) {
  const text=input.replace(/^\uFEFF/,'');
  const header=text.split(/\r?\n/)[0]||'';
  let quoted=false;const counts={',':0,';':0,'\t':0};
  for(let i=0;i<header.length;i++){if(header[i]==='"'){if(quoted&&header[i+1]==='"')i++;else quoted=!quoted;}else if(!quoted&&header[i] in counts)counts[header[i]]++;}
  const delimiter=Object.keys(counts).sort((a,b)=>counts[b]-counts[a])[0];
  const rows=[];let row=[],value='',inQuotes=false;
  for(let i=0;i<text.length;i++){
    const c=text[i];
    if(c==='"'){if(inQuotes&&text[i+1]==='"'){value+='"';i++;}else if(inQuotes||!value)inQuotes=!inQuotes;else value+=c;}
    else if(c===delimiter&&!inQuotes){row.push(value);value='';}
    else if((c==='\n'||c==='\r')&&!inQuotes){if(c==='\r'&&text[i+1]==='\n')i++;row.push(value);if(row.some(cell=>cell.trim()))rows.push(row);row=[];value='';}
    else value+=c;
  }
  if(inQuotes)throw new Error('El CSV contiene una celda entre comillas sin cerrar.');
  row.push(value);if(row.some(cell=>cell.trim()))rows.push(row);
  return rows;
}
export function relations(value) {
  const values = Array.isArray(value) ? value : [String(value || '')];
  const paths = values.flatMap(value => {
    const matches = [...String(value).matchAll(/\[\[([^\]]+)\]\]/g)];
    return matches.map(match => match[1]);
  });
  if (Array.isArray(value)) for (const item of value) if (typeof item === 'string' && !item.includes('[[') && item.trim()) paths.push(item.trim());
  return [...new Set(paths.map(path => path.split('|')[0].split('#')[0].trim().replace(/\.md$/i, '') + '.md'))];
}

export function fieldValue(row, field, allRows = []) {
  if (field.type === 'formula') {
    const a = Number(row.meta[field.source]) || 0, b = Number(row.meta[field.second]) || 0;
    if (field.operation === 'sum') return a + b;
    if (field.operation === 'difference') return a - b;
    if (field.operation === 'multiply') return a * b;
    if (field.operation === 'relation_count') return relations(row.meta[field.source]).length;
    if (field.operation === 'days_until') { const date = String(row.meta[field.source] || ''); const difference = (Date.parse(date + 'T00:00:00Z') - Date.parse(localDate() + 'T00:00:00Z')) / 86400000; return /^\d{4}-\d{2}-\d{2}$/.test(date) && Number.isFinite(difference) ? difference : ''; }
    return '';
  }
  if (field.type === 'rollup') {
    const paths = relations(row.meta[field.source]);
    const related = allRows.filter(r => paths.includes(r.path));
    if (field.operation === 'count') return related.length;
    const values = related.map(r => Number(r.meta[field.target])).filter(Number.isFinite);
    const sum = values.reduce((a,b) => a+b,0);
    return field.operation === 'average' ? values.length ? Math.round(sum / values.length * 100) / 100 : 0 : sum;
  }
  return field.key === 'title' ? row.title : row.meta[field.key] ?? '';
}

export function queryRows(rows, config, allRows = rows) {
  const view = config.views?.find(v => v.id === config.activeView) || config.views?.[0] || {};
  const query = String(config.search || '').toLocaleLowerCase();
  const fields = [{ key: 'title', type: 'title' }, ...(config.fields || [])];
  let result = rows.filter(row => !!row.meta.archived === !!config.showArchived);
  if (query) result = result.filter(row => `${row.title} ${Object.values(row.meta).flat().join(' ')}`.toLocaleLowerCase().includes(query));
  for (const filter of view.filters || []) {
    const field = fields.find(f => f.key === filter.key); if (!field) continue;
    result = result.filter(row => {
      const value = fieldValue(row, field, allRows), expected = filter.value;
      if (filter.op === 'empty') return value === '' || value == null || Array.isArray(value) && !value.length;
      if (filter.op === 'gte') return Number(value) >= Number(expected);
      if (filter.op === 'lte') return Number(value) <= Number(expected);
      if (filter.op === 'neq') return String(value) !== String(expected);
      if (filter.op === 'eq') return String(value) === String(expected);
      return String(value).toLocaleLowerCase().includes(String(expected).toLocaleLowerCase());
    });
  }
  const sort = view.sort;
  if (sort?.key) { const field = fields.find(f => f.key === sort.key); if (field) result = [...result].sort((a,b) => {
    const av = fieldValue(a,field,allRows), bv = fieldValue(b,field,allRows);
    const compare = ['number','formula','rollup'].includes(field.type) ? Number(av)-Number(bv) : String(av).localeCompare(String(bv), 'es', { numeric:true });
    return (sort.direction === 'desc' ? -1 : 1) * compare;
  }); }
  return result;
}

export function newDatabase(name, path) {
  return { version: 1, name, icon: 'Columns2', description: 'Un lugar para organizar lo que importa.', folder: path.replace(/\.base$/i, ''), fields: [
    { key: 'estado', name: 'Estado', type: 'select', options: ['Por empezar','En curso','Completado'] },
    { key: 'prioridad', name: 'Prioridad', type: 'select', options: ['Baja','Media','Alta'] },
    { key: 'fecha', name: 'Fecha', type: 'date' },
  ], views: [
    { id:'table',name:'Tabla',type:'table',filters:[],sort:null },
    { id:'board',name:'Tablero',type:'board',groupBy:'estado',filters:[],sort:null },
    { id:'gallery',name:'Galería',type:'gallery',filters:[],sort:null },
    { id:'calendar',name:'Calendario',type:'calendar',dateBy:'fecha',filters:[],sort:null },
    { id:'list',name:'Lista',type:'list',filters:[],sort:null },
  ], activeView:'table', templates: [] };
}

export const pageTemplates = [
  { id:'blank', name:'Página en blanco', icon:'FileText', description:'El comienzo de cualquier gran idea.', body:'' },
  { id:'project', name:'Proyecto', icon:'Gem', description:'De la visión al siguiente paso.', body:'> [!note] La visión\n> ¿Qué quieres construir y por qué importa?\n\n## El objetivo\n\nDescribe el resultado que quieres alcanzar.\n\n## Próximos pasos\n\n- [ ] Definir el alcance\n- [ ] Establecer los hitos\n- [ ] Dar el primer paso\n\n## Notas y referencias\n\nConecta aquí tus ideas con [[enlaces internos]].\n' },
  { id:'meeting', name:'Reunión', icon:'Files', description:'Conversaciones que se convierten en acción.', body:'## Contexto\n\nFecha: '+ localDate() +'\n\nParticipantes:\n\n## Orden del día\n\n- Primer tema\n- Segundo tema\n\n## Decisiones\n\n> Escribe lo que hemos acordado.\n\n## Acciones\n\n- [ ] Acción y responsable\n' },
  { id:'journal', name:'Diario', icon:'BookOpen', description:'Un registro de lo que merece permanecer.', body:'## Lo esencial de hoy\n\nEscribe una idea que quieras conservar.\n\n## Tres cosas por las que dar gracias\n\n- \n- \n- \n\n## Una intención para mañana\n\n- [ ] \n' },
  { id:'reading', name:'Biblioteca de lectura', icon:'Bookmark', description:'Lecturas, citas e ideas que conectan.', body:'## La obra\n\nAutor:\n\n## Ideas esenciales\n\n- Primera idea\n\n## Una cita para recordar\n\n> \n\n## Conexiones\n\n¿Con qué otras ideas se relaciona?\n' },
];
