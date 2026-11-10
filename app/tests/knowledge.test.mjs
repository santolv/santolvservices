import test from 'node:test';
import assert from 'node:assert/strict';
import { readPage, writePage, setProperties, splitBlocks, mergeBlocks, fieldValue, queryRows, newDatabase, relations, rewriteKnowledgeReferences, blockText, updateBlockText, parseCsv, databaseKey } from '../src/ui/knowledge.js';

test('page properties preserve the Markdown body and nested user metadata', () => {
  const body='# Una idea\n\n```md\n---\n```\n\n[[Otra nota|alias]]\n';
  const content=writePage({tags:['lectura','ideas'],custom:{nested:true},editor:'blocks'},body);
  const edited=readPage(setProperties(content,{estado:'En curso',horas:3}));
  assert.equal(edited.body.trim(),body.trim());
  assert.deepEqual(edited.meta.custom,{nested:true});
  assert.deepEqual(edited.meta.tags,['lectura','ideas']);
  assert.equal(edited.meta.horas,3);
});

test('malformed frontmatter cannot silently lose properties', () => {
  const content='---\ntags: [broken\n---\n\nUna nota importante.';
  assert.equal(readPage(content).invalid,true);
  assert.throws(()=>setProperties(content,{status:'ok'}),/YAML/);
  assert.equal(splitBlocks(content).blocks[0].raw,content);
});

test('blocks preserve fenced code, links, headings and task content', () => {
  const content=writePage({custom:'conservar'},'# Título\n\n- [ ] Tarea\n- [x] Terminada\n\n```js\nconst x = "[[literal]]";\n```\n\n[[Ideas/Nota|alias]]\n');
  const parsed=splitBlocks(content);
  assert.equal(parsed.blocks.filter(b=>b.type==='task').length,2);
  const merged=mergeBlocks(content,parsed.blocks);
  assert.equal(readPage(merged).meta.custom,'conservar');
  assert.match(merged,/const x = "\[\[literal\]\]";/);
  assert.match(merged,/\[\[Ideas\/Nota\|alias\]\]/);
  assert.match(merged,/- \[x\] Terminada/);
});

test('each database view keeps its own filters and numeric sorting', () => {
  const config=newDatabase('Proyectos','Proyectos.base');
  config.fields.push({key:'horas',name:'Horas',type:'number'});
  config.views[0].filters=[{key:'estado',op:'eq',value:'En curso'}];
  config.views[0].sort={key:'horas',direction:'desc'};
  const rows=[{title:'A',meta:{estado:'En curso',horas:2}},{title:'B',meta:{estado:'Por empezar',horas:10}},{title:'C',meta:{estado:'En curso',horas:12}},{title:'Archivo',meta:{archived:true,estado:'En curso',horas:50}}];
  assert.deepEqual(queryRows(rows,config).map(r=>r.title),['C','A']);
  config.activeView='board';
  assert.equal(queryRows(rows,config).length,3);
  config.showArchived=true;
  assert.deepEqual(queryRows(rows,config).map(r=>r.title),['Archivo']);
});

test('relations and rollups use the actual linked Markdown pages', () => {
  const row={meta:{relations:'[[Ideas/A]], [[Ideas/B]]',hours:2,rate:4}};
  const all=[{path:'Ideas/A.md',meta:{hours:3}},{path:'Ideas/B.md',meta:{hours:7}},{path:'Other.md',meta:{hours:100}}];
  assert.deepEqual(relations(row.meta.relations),['Ideas/A.md','Ideas/B.md']);
  assert.deepEqual(relations(['[[Ideas/A|alias]]','Ideas/B.md','[[Ideas/A]]']),['Ideas/A.md','Ideas/B.md']);
  assert.equal(fieldValue(row,{type:'formula',operation:'multiply',source:'hours',second:'rate'},all),8);
  assert.equal(fieldValue(row,{type:'rollup',source:'relations',target:'hours',operation:'sum'},all),10);
  assert.equal(fieldValue(row,{type:'rollup',source:'relations',target:'hours',operation:'average'},all),5);
  assert.equal(fieldValue(row,{type:'formula',source:'relations',operation:'relation_count'},all),2);
});

test('property removal keeps unrelated values and dates as strings', () => {
  const page=setProperties(writePage({fecha:'2026-11-07',old:'remove',keep:42},'Texto'),{old:undefined});
  assert.deepEqual(readPage(page).meta,{fecha:'2026-11-07',keep:42});
});

test('renaming a collection preserves record membership and code examples',()=>{
  const content=writePage({_database:'Proyectos.base',parent:'Inicio.md'},'[[Proyectos.base]] y `[[Proyectos.base]]`');
  const renamed=rewriteKnowledgeReferences(content,'Proyectos.base','Misiones.base',['Proyectos.base','Inicio.md']);
  assert.equal(readPage(renamed).meta._database,'Misiones.base');
  assert.equal(readPage(renamed).meta.parent,'Inicio.md');
  assert.equal(readPage(renamed).body.trim(),'[[Misiones.base]] y `[[Proyectos.base]]`');
});

test('editing block text preserves its structural type and task state',()=>{
  const heading={type:'h2',raw:'## Antes'};
  assert.equal(blockText(heading),'Antes');
  assert.equal(updateBlockText(heading,'Después'),'## Después');
  const task={type:'task',raw:'- [x] Terminada'};
  assert.equal(blockText(task),'Terminada');
  assert.equal(updateBlockText(task,'Renombrada'),'- [x] Renombrada');
  const code={type:'code',raw:'```js\nconst a = 1;\n```'};
  assert.equal(blockText(code),'const a = 1;');
  assert.equal(updateBlockText(code,'```interior```'),'````js\n```interior```\n````');
});

test('CSV imports preserve quoted delimiters, Unicode, newlines and escaped quotes',()=>{
  assert.deepEqual(parseCsv('\uFEFFNombre,Notas\r\n"Café, libros","Una línea\ny otra ""cita"""\r\n'),[['Nombre','Notas'],['Café, libros','Una línea\ny otra "cita"']]);
  assert.deepEqual(parseCsv('Nombre;Estado\nAtlas;En curso'),[['Nombre','Estado'],['Atlas','En curso']]);
  assert.throws(()=>parseCsv('Nombre,Nota\nA,"sin cerrar'),/sin cerrar/);
  assert.equal(databaseKey('Title'),'prop_title');
  assert.equal(databaseKey('Parent'),'prop_parent');
});
