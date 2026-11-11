import * as Y from 'yjs';
import { unzipSync, zipSync, strFromU8, strToU8 } from 'fflate';
import { diffChars } from 'diff';

const LIMIT = 128 * 1024 * 1024;
const WORD = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main';
const DRAW = 'http://schemas.openxmlformats.org/drawingml/2006/main';
const SLIDES = 'http://schemas.openxmlformats.org/presentationml/2006/main';
const SHEET = 'http://schemas.openxmlformats.org/spreadsheetml/2006/main';
const CORE = 'http://purl.org/dc/terms/';
const serialize = node => node ? new XMLSerializer().serializeToString(node) : undefined;
const conflict = () => { throw new Error('Dos sesiones cambiaron la misma estructura. Conserva tu copia local para revisar el conflicto.'); };
const same = (a, b) => a === b || (a && b && a.length === b.length && a.every((value, index) => value === b[index]));
const pick = (base, local, remote, equal = (a, b) => a === b) => {
  if (equal(local, remote) || equal(local, base)) return remote;
  if (equal(remote, base)) return local;
  return conflict();
};

function unpack(buffer) {
  let total = 0;
  const seen = new Set();
  return unzipSync(new Uint8Array(buffer), { filter(file) {
    if (file.name.startsWith('/') || file.name.split('/').includes('..') || file.name.includes('\\') ||
        file.name === '__proto__' || seen.has(file.name)) conflict();
    seen.add(file.name);
    total += file.originalSize;
    if (total > LIMIT || seen.size > 8192) conflict();
    return true;
  } });
}

function parse(input) {
  const text = typeof input === 'string' ? input : strFromU8(input);
  if (/<!DOCTYPE/i.test(text)) conflict();
  const doc = new DOMParser().parseFromString(text, 'application/xml');
  if (doc.querySelector('parsererror') || !doc.documentElement) conflict();
  return doc;
}

export function mergeText(base, local, remote) {
  if (local === remote || local === base) return remote;
  if (remote === base) return local;
  const seed = new Y.Doc();
  const left = new Y.Doc();
  const right = new Y.Doc();
  try {
    seed.getText('text').insert(0, base);
    const initial = Y.encodeStateAsUpdate(seed);
    const localEdits = textEdits(base, local);
    const known = new Set(localEdits.map(edit => JSON.stringify(edit)));
    const remoteEdits = textEdits(base, remote).filter(edit => !known.has(JSON.stringify(edit)));
    for (const [branch, edits] of [[left, localEdits], [right, remoteEdits]]) {
      Y.applyUpdate(branch, initial);
      const text = branch.getText('text');
      branch.transact(() => {
        for (let index = edits.length - 1; index >= 0; index--) {
          const { start, remove, insert } = edits[index];
          if (remove) text.delete(start, remove);
          if (insert) text.insert(start, insert);
        }
      });
    }
    Y.applyUpdate(left, Y.encodeStateAsUpdate(right, Y.encodeStateVector(left)));
    return left.getText('text').toString();
  } finally {
    seed.destroy(); left.destroy(); right.destroy();
  }
}

function textEdits(base, changed) {
  const edits = [];
  let position = 0;
  let current;
  for (const part of diffChars(base, changed, { timeout: 250 }) || conflict()) {
    if (part.added || part.removed) {
      if (!current) edits.push(current = { start: position, remove: 0, insert: '' });
      if (part.added) current.insert += part.value;
      else current.remove += part.value.length;
    } else current = null;
    if (!part.added) position += part.value.length;
  }
  return edits;
}

function mergeEntries(base, local, remote, mergeItem) {
  const seed = new Y.Doc();
  const left = new Y.Doc();
  const right = new Y.Doc();
  try {
    const keys = new Set([...base.keys(), ...local.keys(), ...remote.keys()]);
    const seedMap = seed.getMap('items');
    seed.transact(() => { for (const [key, node] of base) seedMap.set(key, serialize(node)); });
    const initial = Y.encodeStateAsUpdate(seed);
    Y.applyUpdate(left, initial); Y.applyUpdate(right, initial);
    const apply = (doc, changes) => {
      const map = doc.getMap('items');
      doc.transact(() => {
        for (const [key, xml] of changes) {
          if (xml === undefined) map.delete(key);
          else map.set(key, xml);
        }
      });
    };
    const localChanges = new Map();
    const remoteChanges = new Map();
    for (const key of keys) {
      const b = serialize(base.get(key)), l = serialize(local.get(key)), r = serialize(remote.get(key));
      if (l === r) { if (l !== b) localChanges.set(key, l); }
      else if (l === b) remoteChanges.set(key, r);
      else if (r === b) localChanges.set(key, l);
      else localChanges.set(key, serialize(mergeItem(base.get(key), local.get(key), remote.get(key))));
    }
    apply(left, localChanges); apply(right, remoteChanges);
    Y.applyUpdate(left, Y.encodeStateAsUpdate(right, Y.encodeStateVector(left)));
    const items = left.getMap('items');
    return new Map([...keys].filter(key => items.has(key)).map(key => [key, parse(items.get(key)).documentElement]));
  } finally {
    seed.destroy(); left.destroy(); right.destroy();
  }
}

function index(elements, key) {
  const result = new Map();
  for (const element of elements) {
    const id = key(element);
    if (!id || result.has(id)) conflict();
    result.set(id, element);
  }
  return result;
}

const signature = node => node.namespaceURI + ':' + node.localName;
const attrs = node => new Map([...node.attributes].map(attr => [attr.name, attr]));
function mergeAttributes(output, base, local, remote) {
  const b = attrs(base), l = attrs(local), r = attrs(remote);
  for (const key of new Set([...b.keys(), ...l.keys(), ...r.keys()])) {
    const attribute = pick(b.get(key), l.get(key), r.get(key), (a, z) => a?.value === z?.value && a?.namespaceURI === z?.namespaceURI);
    if (attribute) output.setAttributeNS(attribute.namespaceURI, attribute.name, attribute.value);
    else output.removeAttribute(key);
  }
}

function keyedChildren(base, local, remote, key, mergeItem, sort) {
  const b = index(base.children, key), l = index(local.children, key), r = index(remote.children, key);
  const values = mergeEntries(b, l, r, mergeItem);
  if (sort) return [...values].sort(([a], [z]) => sort(a, z)).map(([, node]) => node);
  // An insertion or reorder on one side keeps that side's z-order. Competing orders need review.
  const lists = [b, l, r].map(map => [...map.keys()].filter(id => values.has(id)));
  const order = pick(...lists.map(list => JSON.stringify(list)));
  return JSON.parse(order).map(id => values.get(id));
}

function mergeRow(base, local, remote) {
  if (!local || !remote) return conflict();
  const original = base || local.cloneNode(false);
  if (!base && JSON.stringify([...attrs(local).keys()]) !== JSON.stringify([...attrs(remote).keys()])) conflict();
  return mergeNode(original, local, remote);
}

function mergeNode(base, local, remote) {
  const b = serialize(base), l = serialize(local), r = serialize(remote);
  if (l === r || l === b) return remote;
  if (r === b) return local;
  if (!base || !local || !remote || signature(base) !== signature(local) || signature(base) !== signature(remote)) return conflict();
  if (base.namespaceURI === CORE && base.localName === 'modified') return remote;
  if (base.namespaceURI === WORD && base.localName === 'p' && !base.getElementsByTagNameNS(WORD, 't').length) {
    const withoutText = node => {
      const copy = node.cloneNode(true);
      for (const text of copy.getElementsByTagNameNS(WORD, 't')) text.textContent = '';
      return copy;
    };
    const empty = withoutText(local);
    if (empty.getElementsByTagNameNS(WORD, 't').length && serialize(empty) === serialize(withoutText(remote))) {
      return mergeNode(empty, local, remote);
    }
  }
  const result = remote.cloneNode(false);
  mergeAttributes(result, base, local, remote);
  const nodes = [base, local, remote].map(node => [...node.children]);
  if (nodes.every(list => !list.length)) {
    const values = [base.textContent, local.textContent, remote.textContent];
    result.textContent = base.localName === 't' && [WORD, DRAW].includes(base.namespaceURI)
      ? mergeText(...values) : pick(...values);
    return result;
  }
  if ([base, local, remote].some(node => [...node.childNodes].some(child => child.nodeType === 3 && child.textContent.trim()))) conflict();
  let children;
  if (base.namespaceURI === SHEET && base.localName === 'sheetData') {
    children = keyedChildren(base, local, remote, node => node.getAttribute('r'), mergeRow, (a, z) => Number(a) - Number(z));
  } else if (base.namespaceURI === SHEET && base.localName === 'row') {
    const column = ref => [...ref.match(/^[A-Z]+/)?.[0] || ''].reduce((number, ch) => number * 26 + ch.charCodeAt(0) - 64, 0);
    children = keyedChildren(base, local, remote, node => node.getAttribute('r'), conflict, (a, z) => column(a) - column(z));
  } else if (base.namespaceURI === SLIDES && base.localName === 'spTree') {
    const key = node => ['nvGrpSpPr', 'grpSpPr', 'extLst'].includes(node.localName)
      ? node.localName : node.getElementsByTagNameNS(SLIDES, 'cNvPr')[0]?.getAttribute('id');
    children = keyedChildren(base, local, remote, key, mergeNode);
  } else if ((base.namespaceURI === SHEET && base.localName === 'worksheet') ||
      (base.namespaceURI === SLIDES && ['sld', 'cSld'].includes(base.localName)) ||
      (base.namespaceURI === WORD && base.localName === 'document')) {
    children = keyedChildren(base, local, remote, signature, mergeNode);
  } else {
    if (nodes.some(list => list.length !== nodes[0].length)) conflict();
    children = nodes[0].map((node, i) => mergeNode(node, nodes[1][i], nodes[2][i]));
  }
  for (const child of children) if (child) result.append(result.ownerDocument.importNode(child, true));
  return result;
}

function normalizeSheets(parts) {
  const shared = parts['xl/sharedStrings.xml'];
  const strings = shared ? [...parse(shared).getElementsByTagNameNS(SHEET, 'si')] : [];
  const workbookPath = 'xl/workbook.xml';
  if (parts[workbookPath]) {
    const workbook = parse(parts[workbookPath]);
    for (const view of workbook.getElementsByTagNameNS(SHEET, 'workbookView')) {
      view.removeAttribute('activeTab');
      view.removeAttribute('firstSheet');
    }
    parts[workbookPath] = strToU8(serialize(workbook));
  }
  for (const path of Object.keys(parts).filter(name => /^xl\/worksheets\/[^/]+\.xml$/.test(name))) {
    const sheet = parse(parts[path]);
    // Excel recalculates these hints. They are different for every edited range or selection.
    for (const name of ['dimension', 'selection']) {
      for (const node of [...sheet.getElementsByTagNameNS(SHEET, name)]) node.remove();
    }
    for (const row of sheet.getElementsByTagNameNS(SHEET, 'row')) row.removeAttribute('spans');
    for (const cell of sheet.getElementsByTagNameNS(SHEET, 'c')) {
      if (cell.getAttribute('t') !== 's') continue;
      const value = [...cell.children].find(node => node.namespaceURI === SHEET && node.localName === 'v');
      const text = value && strings[Number(value.textContent)];
      if (!text || !/^\d+$/.test(value.textContent)) conflict();
      const inline = sheet.createElementNS(SHEET, cell.prefix ? cell.prefix + ':is' : 'is');
      for (const item of text.children) inline.append(sheet.importNode(item, true));
      cell.replaceChild(inline, value);
      cell.setAttribute('t', 'inlineStr');
    }
    parts[path] = strToU8(serialize(sheet));
  }
}

function coreWithoutTimestamps(bytes) {
  const document = parse(bytes);
  for (const name of ['created', 'modified']) {
    for (const node of [...document.getElementsByTagNameNS(CORE, name)]) node.remove();
  }
  return serialize(document.documentElement);
}

export function mergeOffice(base, local, remote, editor) {
  const [b, l, r] = [base, local, remote].map(unpack);
  if (editor === 'gridcraft') {
    if ([b, l, r].some(parts => parts['xl/calcChain.xml'])) conflict();
    for (const parts of [b, l, r]) normalizeSheets(parts);
  }
  const critical = {
    wordcraft: /^(word\/(?:styles|numbering)\.xml|word\/_rels\/document\.xml\.rels)$/,
    deckcraft: /^ppt\/(?:presentation\.xml|_rels\/presentation\.xml\.rels)$/,
    gridcraft: /^xl\/(?:styles\.xml|workbook\.xml|_rels\/workbook\.xml\.rels)$/,
  }[editor];
  if (!critical) conflict();
  const model = {
    wordcraft: /^word\/(?:document|header\d+|footer\d+)\.xml$/,
    deckcraft: /^ppt\/(?:slides\/slide\d+|notesSlides\/notesSlide\d+)\.xml$/,
    gridcraft: /^xl\/worksheets\/[^/]+\.xml$/,
  }[editor];
  const output = Object.create(null);
  for (const name of new Set([...Object.keys(b), ...Object.keys(l), ...Object.keys(r)])) {
    if (critical.test(name) && !same(l[name], r[name])) conflict();
    let part;
    if (editor === 'gridcraft' && name === 'xl/sharedStrings.xml') part = r[name];
    else if (same(l[name], r[name]) || same(l[name], b[name])) part = r[name];
    else if (same(r[name], b[name])) part = l[name];
    else if (name === 'docProps/core.xml' && l[name] && r[name] &&
        coreWithoutTimestamps(l[name]) === coreWithoutTimestamps(r[name])) part = r[name];
    else if (b[name] && l[name] && r[name] && (model.test(name) || name === 'docProps/core.xml')) {
      const [base, local, remote] = [b[name], l[name], r[name]].map(bytes => parse(bytes).documentElement);
      part = strToU8(serialize(mergeNode(base, local, remote)));
    } else conflict();
    if (part) output[name] = part;
  }
  if (editor === 'gridcraft') {
    const workbook = parse(output['xl/workbook.xml']);
    const calculation = workbook.getElementsByTagNameNS(SHEET, 'calcPr')[0];
    if (calculation) {
      calculation.setAttribute('fullCalcOnLoad', '1');
      output['xl/workbook.xml'] = strToU8(serialize(workbook));
    }
  }
  const zipped = zipSync(output, { level: 3 });
  if (zipped.byteLength > 32 * 1024 * 1024) conflict();
  return zipped;
}
