import { escape, title, renderMarkdown } from './markdown.js';
import { atMentionStart, fileMention, pickReference } from './references.js';
import { createId as uuid, readPage, writePage, setProperties, splitBlocks, mergeBlocks, blockBody, blockText, updateBlockText, parseCsv, newDatabase, pageTemplates, fieldValue, queryRows, propertyKey, databaseKey, pageReservedKeys, isPropertyKey, relations, localDate } from './knowledge.js';

const $ = (selector, root = document) => root.querySelector(selector);
const $$ = (selector, root = document) => [...root.querySelectorAll(selector)];
const fieldTypes = { text:'Texto', select:'Selección', multi:'Selección múltiple', date:'Fecha', number:'Número', checkbox:'Casilla', relation:'Relación', formula:'Fórmula', rollup:'Acumulación', url:'URL' };
const fieldIcons = { text:'List', select:'ChevronDown', multi:'Tags', date:'CalendarDays', number:'Hash', checkbox:'Check', relation:'Links', formula:'CodeXml', rollup:'ListFilter', url:'ExternalLink' };
const viewIcons = { table:'List', board:'Columns2', gallery:'LayoutDashboard', calendar:'CalendarDays', list:'ListFilter' };
const covers = { none:'Sin portada', tapestry:'Tapiz violeta', forest:'Niebla', parchment:'Papel', wine:'Terciopelo' };
const dateOnly = value => typeof value === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(value);
const normalized = value => value.toLocaleLowerCase().normalize('NFD').replace(/\p{Diacritic}/gu,'');
const emojis = [
  ['😀','Sonrisa smile'],['😂','Risa laugh'],['🙂','Feliz happy'],['😍','Enamorado love'],['👍','Pulgar arriba ok'],['👋','Saludo hola wave'],
  ['❤️','Corazón heart'],['✨','Brillo sparkles'],['⭐','Estrella star'],['🔥','Fuego fire'],['🎉','Fiesta party'],['✅','Hecho check'],
  ['❌','Error cruz cross'],['⚠️','Alerta aviso warning'],['💡','Idea bombilla'],['📅','Calendario fecha calendar'],['📌','Chincheta pin'],
  ['📎','Clip adjunto'],['📚','Libros books'],['📝','Nota memo'],['📈','Gráfico chart'],['🔍','Buscar lupa search'],['🔗','Enlace link'],
  ['🎯','Objetivo target'],['🚀','Cohete rocket'],['⏰','Reloj alarma clock'],['☕','Café coffee'],['🌙','Luna moon'],
];

function prefixBlock(value) {
  const heading=/^(#{1,6}) $/.exec(value);
  if(heading)return 'h'+heading[1].length;
  if(/^[-*+] $/.test(value))return 'bullet';
  if(value==='1. ')return 'number';
  if(value==='[] ')return 'task';
  if(value==='> ')return 'toggle';
  if(value==='" ')return 'quote';
  return null;
}

function chartMarkup(text) {
  const rows=[];
  for (const line of text.split('\n').filter(line=>line.trim())) {
    const match=/^(.+):\s*(-?\d+(?:[.,]\d+)?)\s*$/.exec(line);
    const value=match&&Number(match[2].replace(',','.'));
    if (!match||!match[1].trim()||!Number.isFinite(value)) return '<p class="empty-hint">Escribe una etiqueta y un número por línea. Ejemplo: Enero: 12</p>';
    rows.push({label:match[1].trim(),value});
  }
  if (!rows.length) return '<p class="empty-hint">Haz clic para añadir datos al gráfico.</p>';
  const min=rows.reduce((min,row)=>Math.min(min,row.value),0);
  const max=rows.reduce((max,row)=>Math.max(max,row.value),0);
  const span=Math.max(Math.abs(min),max)||1, range=max/span-min/span||1;
  const percent=value=>100*(value/span-min/span)/range;
  const zero=percent(0), number=new Intl.NumberFormat('es');
  return `<figure class="n-chart" aria-label="Gráfico de barras"><figcaption>Gráfico de barras <small>Clic para editar los datos</small></figcaption><ul style="--chart-zero:${zero}%">${rows.map(({label,value})=>`<li><span>${escape(label)}</span><span class="n-chart-track" aria-hidden="true"><i class="${value<0?'negative':''}" style="left:${Math.min(zero,percent(value))}%;width:${Math.abs(percent(value)-zero)}%"></i></span><strong>${escape(number.format(value))}</strong></li>`).join('')}</ul></figure>`;
}

export function createNotion(ctx) {
  const I = ctx.icon;
  const actionButton = (icon, label, action, classes = '') => `<button type="button" class="${classes}" data-n-action="${action}">${I(icon,15)}<span>${escape(label)}</span></button>`;
  const postFile = (path, content) => ctx.post('/api/file', { path, content });
  const invoke = action => { try { ctx.run(action()); } catch (error) { ctx.report(error); } };
  const choosePath = (name, extension) => {
    let path = name.trim(); if (!path.toLowerCase().endsWith(extension)) path += extension;
    if (!ctx.validPath(path)) throw new Error('El nombre contiene caracteres no válidos.');
    if (ctx.state.files.some(file => file.path === path)) throw new Error('Ya existe una página con ese nombre.');
    return path;
  };
  async function allPages() {
    const files = ctx.state.files.filter(file => !file.is_dir && file.path.endsWith('.md'));
    return Promise.all(files.map(async file => { const doc = await ctx.loadDocument(file.path); return { path:file.path, title:title(file.path), doc, ...readPage(doc.content) }; }));
  }
  function update(doc, content, origin) { ctx.changed(doc, content, origin); }
  async function newPage(template = pageTemplates[0], parentDoc = null, onCreated = null, suggestedName = '') {
    if (typeof template === 'string') template = pageTemplates.find(t => t.id === template) || pageTemplates[0];
    const folder = parentDoc ? parentDoc.path.replace(/\.md$/, '') + '/' : '';
    ctx.promptName(parentDoc ? 'Crear subpágina' : 'Crear página', suggestedName || (template.id === 'blank' ? 'Sin título' : template.name), 'Nombre de la página', async name => {
      const path = choosePath(folder + name, '.md');
      const meta = { icon: template.icon || 'FileText', cover:template.cover || 'none', editor:'blocks', ...(template.meta || {}) };
      if (parentDoc) meta.parent = parentDoc.path;
      const content = writePage(meta, template.body || '');
      await postFile(path, content);
      if (onCreated) await onCreated(path);
      else if (parentDoc) { const page = readPage(parentDoc.content); update(parentDoc, writePage(page.meta, page.body.trimEnd() + `\n\n[[${path.replace(/\.md$/,'')}]]\n`)); await ctx.saveDocument(parentDoc); }
      await ctx.refreshFiles(); for (const part of path.split('/').slice(0,-1)) ctx.state.folders.add(part);
      ctx.closeOverlay(); await ctx.openNote(path, { mode:'blocks' });
    });
  }

  const remindersFor = page => (Array.isArray(page.meta.recordatorios) ? page.meta.recordatorios : [])
    .filter(item=>typeof item?.id==='string' && typeof item.label==='string' && typeof item.when==='string' && Number.isFinite(Date.parse(item.when)));

  async function calendarEntries() {
    const [pages,collections]=await Promise.all([allPages(),Promise.all(ctx.state.files.filter(file=>!file.is_dir&&file.path.endsWith('.base')).map(async file=>{
      try {
        const config=JSON.parse((await ctx.loadDocument(file.path)).content);
        const fields=config.fields.filter(field=>field.type==='date');
        const views=config.views.filter(view=>view.type==='calendar');
        return [file.path,[...new Set((views.length?views.map(view=>fields.find(field=>field.key===view.dateBy)||fields[0]):fields).filter(Boolean).map(field=>field.key))]];
      } catch { return [file.path,[]]; }
    }))]);
    const dateFields=new Map(collections);
    return pages.filter(page=>!page.meta.archived&&!page.meta.template).flatMap(page=>{
      const dates=new Set([page.meta.fecha_calendario,page.title,...(dateFields.get(page.meta._database)||['fecha']).map(key=>page.meta[key])].filter(dateOnly));
      return [
        ...[...dates].map(date=>({date,path:page.path,title:page.title})),
        ...remindersFor(page).map(item=>({date:item.when.slice(0,10),path:page.path,title:item.label,when:item.when,id:item.id})),
      ];
    }).sort((a,b)=>a.date.localeCompare(b.date)||String(a.when||'').localeCompare(String(b.when||''))||a.title.localeCompare(b.title,'es'));
  }

  async function dueReminders() {
    const now=Date.now();
    return (await allPages()).filter(page=>!page.meta.archived&&!page.meta.template).flatMap(page=>remindersFor(page)
      .filter(item=>Date.parse(item.when)<=now && Date.parse(item.when)>now-86_400_000)
      .map(item=>({...item,path:page.path})));
  }

  function pickDate(heading, type, initial, onSave, allowClear=false, label='') {
    ctx.dialog(heading,`<form id="scDateForm"><label class="field-label" for="scDateValue">Fecha${type==='datetime-local'?' y hora':''}</label><input id="scDateValue" class="text-input" type="${type}" value="${escape(initial)}" ${allowClear?'':'required'}>${label?`<label class="field-label" for="scDateLabel">Título del recordatorio</label><input class="text-input" id="scDateLabel" value="${escape(label)}" required maxlength="160">`:''}<div class="form-actions"><button class="primary-button" type="submit">Guardar</button></div></form>`);
    const input=$('#scDateValue');
    $('#scDateForm').onsubmit=event=>{
      event.preventDefault();
      const date=input.value, name=$('#scDateLabel')?.value.trim();
      ctx.closeOverlay();
      invoke(()=>onSave(date,name));
    };
  }

  function scheduleNote(doc) {
    const date=readPage(doc.content).meta.fecha_calendario;
    pickDate('Programar nota','date',dateOnly(date)?date:localDate(),async value=>{
      update(doc,setProperties(doc.content,{fecha_calendario:value||undefined}));
      await ctx.saveDocument(doc);
      ctx.notify(value?'Nota programada en el calendario.':'Fecha de la nota eliminada.');
    },true);
  }

  async function changeReminder(event,patch) {
    const doc=await ctx.loadDocument(event.path);
    const current=remindersFor(readPage(doc.content));
    const reminders=patch?current.map(item=>item.id===event.id?{...item,...patch}:item):current.filter(item=>item.id!==event.id);
    update(doc,setProperties(doc.content,{recordatorios:reminders.length?reminders:undefined}));
    await ctx.saveDocument(doc);
  }

  function mountCalendar(container,tab) {
    let alive=true, version=0, timer;
    let selected=dateOnly(tab.date)?tab.date:localDate();
    function setDay(date,focus=false) {
      selected=date;tab.date=date;ctx.persist();
      invoke(async()=>{await draw();if(focus)$(`[data-day="${date}"]`,container)?.focus();});
    }
    async function draw() {
      const request=++version, events=await calendarEntries();
      if(!alive||request!==version||!container.isConnected)return;
      const [year,month]=selected.split('-').map(Number), first=new Date(year,month-1,1);
      const offset=(first.getDay()+6)%7, days=new Date(year,month,0).getDate(), prefix=selected.slice(0,7)+'-';
      const byDay=new Map();
      for(const event of events) {
        if(!event.date.startsWith(prefix))continue;
        const items=byDay.get(event.date)||[];
        items.push(event);byDay.set(event.date,items);
      }
      const today=localDate(), dayEvents=byDay.get(selected)||[];
      const dateLabel=new Date(selected+'T12:00:00').toLocaleDateString('es',{weekday:'long',day:'numeric',month:'long'});
      container.innerHTML=`<div class="sc-calendar">
        <header class="sc-calendar-head"><div><h1>Calendario</h1><p>Notas, colecciones y recordatorios de la bóveda.</p></div><button class="primary-button" data-n-action="event">${I('Plus',16)} Nuevo evento</button></header>
        <div class="sc-calendar-layout"><section class="sc-calendar-grid" aria-label="Mes">
          <div class="n-calendar-controls"><strong>${escape(first.toLocaleDateString('es',{month:'long',year:'numeric'}))}</strong><span></span><button data-step="-1" aria-label="Mes anterior">${I('ChevronLeft',17)}</button><button data-today>Hoy</button><button data-step="1" aria-label="Mes siguiente">${I('ChevronRight',17)}</button></div>
          <div class="n-calendar-week">${['Lun','Mar','Mié','Jue','Vie','Sáb','Dom'].map(day=>`<span>${day}</span>`).join('')}</div>
          <div class="n-calendar">${Array.from({length:Math.ceil((offset+days)/7)*7},(_,i)=>{
            const day=i-offset+1;
            if(day<1||day>days)return '<div class="n-calendar-day outside"></div>';
            const date=prefix+String(day).padStart(2,'0'), items=byDay.get(date)||[];
            return `<button type="button" class="n-calendar-day ${date===today?'today':''}" data-day="${date}" aria-label="${date}${items.length?', '+items.length+' eventos':''}" aria-pressed="${date===selected}"><strong>${day}</strong>${items.slice(0,2).map(item=>`<span>${escape(item.title)}</span>`).join('')}${items.length>2?`<small>+${items.length-2}</small>`:''}</button>`;
          }).join('')}</div>
        </section><section class="sc-calendar-agenda" aria-label="Agenda del día"><h2>${escape(dateLabel)}</h2>
          ${dayEvents.length?dayEvents.map((item,index)=>`<div class="sc-agenda-row"><button class="sc-agenda-event" data-path="${escape(item.path)}">${I(item.when?'CalendarDays':'FileText',16)}<span>${escape(item.title)}<small>${item.when?escape(item.when.slice(11,16)):'Todo el día'}</small></span></button>${item.id?`<button class="icon-button" type="button" aria-label="Opciones del recordatorio" data-reminder="${index}">${I('MoreHorizontal',16)}</button>`:''}</div>`).join(''):'<p>No hay eventos para este día.</p>'}
          <button class="secondary-button" data-n-action="daily">${I('BookOpen',15)} Abrir diario</button>
        </section></div></div>`;
      $$('[data-day]',container).forEach(button=>{
        button.onclick=event=>setDay(button.dataset.day,event.detail===0);
        button.onkeydown=event=>{
          const delta={ArrowLeft:-1,ArrowRight:1,ArrowUp:-7,ArrowDown:7}[event.key];
          if(!delta)return;
          event.preventDefault();
          setDay(localDate(new Date(year,month-1,Number(button.dataset.day.slice(-2))+delta)),true);
        };
      });
      $$('[data-step]',container).forEach(button=>button.onclick=()=>setDay(localDate(new Date(year,month-1+Number(button.dataset.step),1))));
      $('[data-today]',container).onclick=()=>setDay(today);
      $$('[data-path]',container).forEach(button=>button.onclick=()=>ctx.run(ctx.openNote(button.dataset.path)));
      $$('[data-reminder]',container).forEach(button=>button.onclick=event=>{
        const reminder=dayEvents[Number(button.dataset.reminder)];
        ctx.menu(event,[
          {icon:'Pencil',label:'Editar recordatorio',action:()=>pickDate('Editar recordatorio','datetime-local',reminder.when,(when,label)=>changeReminder(reminder,{when,label}),false,reminder.title)},
          {icon:'Trash2',label:'Quitar recordatorio',danger:true,action:()=>changeReminder(reminder,null)},
        ]);
      });
      bindLocal(container,{daily:()=>ctx.dailyNote(selected),event:()=>ctx.promptName('Nuevo evento','', 'Nombre para el '+selected,async name=>{
        const path=choosePath(name,'.md');
        await postFile(path,writePage({fecha_calendario:selected},''));
        await ctx.refreshFiles();
        await draw();
      })});
    }
    const refresh=()=>{clearTimeout(timer);timer=setTimeout(()=>invoke(draw),200);};
    document.addEventListener('sc-document-changed',refresh);
    invoke(draw);
    return {refresh:draw,destroy(){alive=false;clearTimeout(timer);document.removeEventListener('sc-document-changed',refresh);}};
  }
  async function createDatabase(parentDoc = null, onCreated = null, view = 'table') {
    const calendar = view === 'calendar';
    ctx.promptName(calendar ? 'Nuevo calendario' : 'Nueva colección', calendar ? 'Mi calendario' : 'Mis proyectos', 'Nombre de la colección', async name => {
      const folder = parentDoc ? parentDoc.path.replace(/\.md$/, '') + '/' : '';
      const path = choosePath(folder + name, '.base');
      const database = newDatabase(title(path), path); if (parentDoc) database.parent = parentDoc.path;
      database.activeView = view;
      await postFile(path, JSON.stringify(database, null, 2));
      if (onCreated) await onCreated(path);
      else if (parentDoc) { const page = readPage(parentDoc.content); update(parentDoc, writePage(page.meta, page.body.trimEnd() + `\n\n[[${path}]]\n`)); await ctx.saveDocument(parentDoc); }
      await ctx.refreshFiles(); ctx.closeOverlay();
      if (onCreated) { ctx.renderWorkspace(); ctx.notify(calendar ? 'Calendario añadido a la página.' : 'Colección añadida a la página.'); } else await ctx.openNote(path);
    });
  }
  async function templates() {
    const stored = (await allPages()).filter(page => page.meta.template === true).map(page => ({ id:page.path, name:page.title, icon:page.meta.icon || 'FileText', description:'Tu plantilla personal', body:page.body, meta:{ ...page.meta, template:undefined } }));
    const all = [...pageTemplates, ...stored];
    ctx.dialog('La biblioteca de plantillas', `<div class="n-modal-eyebrow">EL COMIENZO DE ALGO VALIOSO</div><p class="n-modal-intro">Una estructura para tus ideas. Hazla tuya.</p><div class="n-template-grid">${all.map((template,index) => `<button class="n-template" data-template="${index}"><span class="n-template-icon">${I(template.icon,25)}</span><strong>${escape(template.name)}</strong><small>${escape(template.description)}</small><span class="n-template-use">Crear página ${I('ArrowUpRight',13)}</span></button>`).join('')}</div>`);
    $('.modal').classList.add('n-template-modal');
    $$('[data-template]').forEach(el => el.onclick = () => newPage(all[Number(el.dataset.template)]));
  }


  function mountHome(container) {
    let alive = true;
    let embedded;
    async function render() {
      embedded?.destroy();
      embedded = null;
      const collections = ctx.state.files.filter(file => !file.is_dir && file.path.endsWith('.base'));
      const byPath = new Map(ctx.state.files.filter(file => !file.is_dir).map(file => [file.path, file]));
      const recent = ctx.recentFiles().filter(path => byPath.has(path)).slice(0, 5);
      const favorites = ctx.state.bookmarks.filter(path => byPath.has(path));
      const today = localDate();
      const events=(await calendarEntries()).filter(event=>event.date>=today).slice(0,5);
      if (!alive || !container.isConnected) return;
      const pinned = ctx.homeCollection();
      const hours = new Date().getHours();
      const greeting = hours < 12 ? 'Buenos días' : hours < 20 ? 'Buenas tardes' : 'Buenas noches';
      const open = (path, label = title(path)) => `<button type="button" data-open="${escape(path)}">${I(path.endsWith('.base') ? 'Columns2' : 'FileText',16)} <span>${escape(label)}</span></button>`;
      container.innerHTML = `<div class="sc-home">
        <h1>${greeting}</h1>
        <section><h2>${I('List',15)} Visitadas recientemente</h2>
          <div class="sc-home-recent">${recent.map(path => open(path)).join('')}${actionButton('Plus','Nueva página','page')}</div>
        </section>
        ${favorites.length ? `<section><h2>${I('Bookmark',15)} Favoritas</h2><div class="sc-home-panel sc-home-links">${favorites.map(path => open(path)).join('')}</div></section>` : ''}
        <section><h2>${I('CalendarDays',15)} Próximos eventos</h2>
          <div class="sc-home-panel sc-home-events">${events.length ? events.map(event => `<div><time datetime="${event.date}">${escape(new Date(event.date + 'T12:00:00').toLocaleDateString('es',{day:'numeric',month:'short'}))}</time>${open(event.path,event.title)}</div>`).join('') : `<div class="sc-home-empty">${I('CalendarDays',24)}<p>Las fechas de notas, colecciones y recordatorios aparecerán aquí.</p>${actionButton('CalendarDays','Abrir calendario','calendar')}</div>`}</div>
        </section>
        <section><h2>${I('Columns2',15)} Vistas de inicio</h2><div class="sc-home-panel">
          <div class="sc-home-pin"><label for="homeCollection">Colección anclada</label><select id="homeCollection"><option value="">Seleccionar colección</option>${collections.map(file => `<option value="${escape(file.path)}">${escape(title(file.path))}</option>`).join('')}</select></div>
          ${collections.some(file => file.path === pinned) ? '<div id="homePinned"></div>' : `<div class="sc-home-empty">${I('Columns2',24)}<p>Ancla una colección para consultar y editar sus vistas desde Inicio.</p>${actionButton('Plus','Crear colección','database')}</div>`}
        </div></section>
      </div>`;
      const selector = $('#homeCollection',container);
      selector.value = collections.some(file => file.path === pinned) ? pinned : '';
      selector.onchange = () => { ctx.homeCollection(selector.value); ctx.run(render()); };
      bindLocal(container, { page:() => ctx.createNote(), database:() => createDatabase(), calendar:()=>ctx.openCalendar() });
      $$('[data-open]',container).forEach(el => el.onclick = () => ctx.run(ctx.openNote(el.dataset.open)));
      const target = $('#homePinned',container);
      if (target) embedded = mountDatabase(target, { id:'home-pinned', path:pinned }, true);
    }
    ctx.run(render());
    return { destroy() { alive = false; embedded?.destroy(); }, refresh:render };
  }

  function bindLocal(root, actions) {
    root.onclick = event => { const el = event.target.closest('[data-n-action]'); if (!el || !root.contains(el)) return; const action = actions[el.dataset.nAction]; if (action) { event.preventDefault(); event.stopPropagation(); invoke(() => action(event,el)); } };
  }
  async function mountPageTree(container) {
    const wrapper=document.createElement('div');wrapper.className='n-page-tree';container.innerHTML='';container.append(wrapper);
    wrapper.innerHTML='<p class="empty-hint">Abriendo tu archivo…</p>';
    const pages=(await allPages()).filter(page=>!page.meta.template);
    const bases=await Promise.all(ctx.state.files.filter(file=>file.path.endsWith('.base')).map(async file=>{const doc=await ctx.loadDocument(file.path);let config={};try{config=JSON.parse(doc.content);}catch{}return{path:file.path,title:config.name||title(file.path),meta:{parent:config.parent,icon:'Columns2'},database:true};}));
    if(!wrapper.isConnected)return;
    const entries=[...bases,...pages].sort((a,b)=>(ctx.state.sort==='asc'?1:-1)*a.title.localeCompare(b.title,'es',{numeric:true}));
    const paths=new Set(entries.map(entry=>entry.path)),visited=new Set();
    const parentOf=entry=>paths.has(entry.meta.parent)?entry.meta.parent:paths.has(entry.meta._database)?entry.meta._database:null;
    wrapper.innerHTML=`<div class="n-logical-tree" role="tree" aria-label="Jerarquía de páginas"></div>`;
    const tree=$('.n-logical-tree',wrapper);
    function branch(parent,depth){for(const entry of entries.filter(entry=>parentOf(entry)===parent)){if(visited.has(entry.path))continue;visited.add(entry.path);const children=entries.some(child=>parentOf(child)===entry.path),expanded=ctx.state.folders.has(entry.path),row=document.createElement('div');row.className='n-page-tree-row';row.dataset.path=entry.path;row.classList.toggle('active',ctx.activePath()===entry.path);row.style.paddingLeft=(8+depth*14)+'px';row.innerHTML=`<button class="n-page-tree-expand" aria-label="${expanded?'Contraer':'Expandir'} ${escape(entry.title)}" ${!children?'disabled':''}>${children?I(expanded?'ChevronDown':'ChevronRight',12):''}</button><button class="n-page-tree-open" role="treeitem" ${children?`aria-expanded="${expanded}"`:''} title="${escape(entry.path)}">${I(entry.meta.icon||'FileText',14)}<span>${escape(entry.title)}</span></button>`;$('button.n-page-tree-expand',row).onclick=()=>{expanded?ctx.state.folders.delete(entry.path):ctx.state.folders.add(entry.path);ctx.persist();ctx.run(mountPageTree(container));};$('.n-page-tree-open',row).onclick=()=>ctx.run(ctx.openNote(entry.path,entry.database?{}:{mode:'blocks'}));tree.append(row);if(children&&expanded)branch(entry.path,depth+1);}}
    branch(null,0);if(!entries.length)tree.innerHTML='<p class="empty-hint">Crea tu primera página.</p>';
  }
  async function movePage(doc){
    if(!doc||!doc.path.endsWith('.md'))return;
    const pages=await allPages(),metaByPath=new Map(pages.map(page=>[page.path,page.meta]));
    const isDescendant=path=>{const seen=new Set();let parent=path;while(parent&&!seen.has(parent)){if(parent===doc.path)return true;seen.add(parent);parent=metaByPath.get(parent)?.parent;}return false;};
    ctx.chooser('Mover página a…','Buscar una página de destino…',query=>[{label:'Nivel principal del archivo',icon:'Files',path:null},...pages.filter(page=>!isDescendant(page.path)&&!page.meta.template).map(page=>({label:page.title,icon:page.meta.icon||'FileText',path:page.path}))].filter(item=>!query||item.label.toLocaleLowerCase().includes(query.toLocaleLowerCase())).map(item=>({...item,action:async()=>{update(doc,setProperties(doc.content,{parent:item.path||undefined}));await ctx.saveDocument(doc);if(item.path){const parent=await ctx.loadDocument(item.path),page=readPage(parent.content),target=doc.path.replace(/\.md$/,'');if(!page.body.includes(`[[${target}`)){update(parent,writePage(page.meta,page.body.trimEnd()+`\n\n[[${target}]]\n`));await ctx.saveDocument(parent);}}ctx.renderWorkspace();await ctx.refreshFiles();ctx.notify('Página movida en la jerarquía.');}})));
  }
  function coverMarkup(value, label = '') { return value && value !== 'none' && covers[value] ? `<div class="n-page-cover cover-${value}"><span class="n-cover-corner">✦</span><span class="n-cover-inscription">${escape(label || 'ARS · MEMORIAE')}</span><span class="n-cover-corner">✦</span></div>` : ''; }
  function appearance(doc, tab, redraw) {
    const page = readPage(doc.content);
    ctx.dialog('El aspecto de tu página', `<div class="n-modal-eyebrow">CADA DETALLE TIENE SU LUGAR</div><label class="field-label" for="pageIcon">Emblema</label><div class="n-icon-picker" id="pageIcon">${['FileText','Gem','BookOpen','Bookmark','CalendarDays','Network','Files','StickyNote','Columns2','SquareTerminal'].map(name => `<button class="${page.meta.icon === name ? 'selected' : ''}" data-icon="${name}" title="${name}" aria-label="Emblema ${name}">${I(name,23)}</button>`).join('')}</div><label class="field-label">Portada</label><div class="n-cover-picker">${Object.entries(covers).map(([key,label]) => `<button data-cover="${key}" class="cover-${key} ${page.meta.cover === key ? 'selected' : ''}"><span>${escape(label)}</span></button>`).join('')}</div>`);
    $$('[data-icon]').forEach(el => el.onclick = () => { update(doc,setProperties(doc.content,{icon:el.dataset.icon}),tab.id); ctx.closeOverlay(); redraw(); });
    $$('[data-cover]').forEach(el => el.onclick = () => { update(doc,setProperties(doc.content,{cover:el.dataset.cover}),tab.id); ctx.closeOverlay(); redraw(); });
  }

  function mountBlocks(container, tab) {
    const doc = ctx.state.documents.get(tab.path); let alive = true, blocks = [], page, dragged = null, cleanupChildren = [], blockPicker = null;
    tab.blockHistory ||= { undo:[], redo:[] };
    function remember() { tab.blockHistory.undo.push(doc.content); if (tab.blockHistory.undo.length > 80) tab.blockHistory.undo.shift(); tab.blockHistory.redo = []; }
    function commit(record = true) { const value = mergeBlocks(doc.content,blocks); if (value === doc.content) return; if (record) remember(); update(doc,value,tab.id); }
    function undo(redo = false) { const from = redo ? tab.blockHistory.redo : tab.blockHistory.undo, to = redo ? tab.blockHistory.undo : tab.blockHistory.redo; if (!from.length) return; to.push(doc.content); update(doc,from.pop(),tab.id); draw(); }
    function draw() {
      if (!alive) return; blockPicker?.close(); cleanupChildren.forEach(fn=>fn()); cleanupChildren=[];
      page = splitBlocks(doc.content); blocks = page.blocks.map(block=>({...block,id:uuid()}));
      const meta = page.meta;
      container.innerHTML = `<div class="n-block-page">${coverMarkup(meta.cover)}<div class="n-block-inner"><div class="n-page-tools">${actionButton('Gem','Personalizar','appearance','n-small-link')}${actionButton('FilePlus2','Subpágina','subpage','n-small-link')}${actionButton('BookOpen','Plantilla','save-template','n-small-link')}<span></span><button class="n-mini-button" data-n-action="undo" aria-label="Deshacer cambio de bloques" title="Deshacer cambio de bloques">${I('ArrowLeft',15)}</button><button class="n-mini-button" data-n-action="redo" aria-label="Rehacer cambio de bloques" title="Rehacer cambio de bloques">${I('ArrowRight',15)}</button></div><div class="n-page-emblem" data-n-action="appearance" role="button" tabindex="0" aria-label="Cambiar emblema y portada">${I(meta.icon || 'FileText',38)}</div><input class="inline-title n-page-title" aria-label="Título de la página" value="${escape(title(doc.path))}"><div class="n-page-meta" id="pageMetadata"></div><div class="n-block-list" aria-label="Editor de bloques"></div><button class="n-add-block" data-n-action="add-block">${I('Plus',16)} <span>Añadir un bloque <kbd>/</kbd></span></button><div class="n-subpages"></div><div class="n-page-footnote">${I('Gem',12)} Markdown, por dentro. Tus ideas, por delante.</div></div></div>`;
      $('.n-page-tools [data-n-action=appearance]',container).innerHTML = I('Gem',14)+'<span>Personalizar</span>';
      const name = $('.n-page-title',container); name.onkeydown = event => { if (event.key === 'Enter') { event.preventDefault(); name.blur(); } if (event.key === 'Escape') { name.value=title(doc.path);name.blur(); } }; name.onblur = () => { if (name.value.trim() !== title(doc.path)) { const parent = doc.path.includes('/') ? doc.path.slice(0,doc.path.lastIndexOf('/')+1):''; ctx.run(ctx.renamePath(doc.path,parent+name.value.trim()+'.md').catch(error=>{name.value=title(doc.path);throw error;})); } };
      const metadata = $('#pageMetadata',container); renderPageProperties(metadata,doc,tab,draw);
      const list = $('.n-block-list',container);
      blocks.forEach(block => list.append(renderBlock(block)));
      bindLocal(container, { appearance:() => appearance(doc,tab,draw), subpage:() => newPage(pageTemplates[0],doc), 'save-template':saveAsTemplate, 'add-block':() => insertBlock(), undo:()=>undo(),redo:()=>undo(true) });
      ctx.run(drawSubpages());
    }
    function drawList() { blockPicker?.close(); const list=$('.n-block-list',container); if(!list)return;cleanupChildren.forEach(fn=>fn());cleanupChildren=[];list.innerHTML='';blocks.forEach(block=>list.append(renderBlock(block))); }
    function insertBlock(after = blocks.at(-1)) {
      $('.n-block-textarea',container)?.blur();
      const block = !after.raw ? after : {id:uuid(),type:'text',raw:''};
      if (block !== after) {
        blocks.splice(blocks.indexOf(after)+1,0,block);
        $(`[data-block="${after.id}"]`,container).after(renderBlock(block));
      }
      edit(block.id,true);
    }
    function renderBlock(block) {
      const el=document.createElement('div');el.className=`n-block n-block-${block.type}`;el.dataset.block=block.id;
      el.tabIndex=-1;
      el.onblur=()=>el.classList.remove('n-block-selected');
      el.onkeydown=event=>{
        if(event.target!==el || event.isComposing || !el.classList.contains('n-block-selected'))return;
        if((event.ctrlKey||event.metaKey)&&event.code==='KeyZ'&&!event.altKey) {
          event.preventDefault();event.stopPropagation();undo(event.shiftKey);return;
        }
        if(!['Escape','Delete','Backspace','Enter','ArrowUp','ArrowDown'].includes(event.key))return;
        event.preventDefault();event.stopPropagation();
        if(event.key==='Enter')edit(block.id);
        else if(event.key.startsWith('Arrow')) {
          const next=blocks[blocks.indexOf(block)+(event.key==='ArrowUp'?-1:1)];
          if(next)selectBlock(next);
        } else {
          const next=blocks[blocks.indexOf(block)+1]||blocks[blocks.indexOf(block)-1];
          removeBlock(block);
          selectBlock(next||blocks[0]);
        }
      };
      el.innerHTML=`<div class="n-block-rail"><button class="n-block-insert" title="Insertar bloque debajo" aria-label="Insertar bloque debajo">${I('Plus',14)}</button><button class="n-block-handle" draggable="true" title="Opciones del bloque; arrastra para mover" aria-label="Opciones del bloque">${I('Grip',15)}</button></div><div class="n-block-render" tabindex="0">${block.raw ? renderMarkdown(block.raw) : '<span class="n-block-placeholder">Escribe algo o pulsa / para ver los bloques…</span>'}</div>`;
      const preview=$('.n-block-render',el);
      if(block.type==='callout')preview.classList.add('n-callout');
      const canvas=/^!\[\[(.+\.canvas)\]\]$/i.exec(block.raw.trim());
      if (canvas) {
        const path=canvas[1];
        preview.className='n-canvas-embed';
        preview.innerHTML=`<header>${I('LayoutDashboard',16)}<span>${escape(title(path))}</span><button type="button" aria-label="Abrir lienzo completo" title="Abrir lienzo completo">${I('ArrowUpRight',16)}</button></header><div class="n-canvas-body"></div>`;
        $('header button',preview).onclick=()=>ctx.run(ctx.openNote(path));
        const body=$('.n-canvas-body',preview);
        if (!ctx.state.files.some(file=>!file.is_dir&&file.path===path)) body.textContent='No se encuentra el archivo del lienzo.';
        else ctx.run((async()=>{
          await ctx.loadDocument(path);
          if(!alive||!preview.isConnected)return;
          const instance=ctx.mountCanvas(body,{id:tab.id+'-'+block.id,path},true);
          if(instance) {
            preview.disposeCanvas=instance.destroy;
            cleanupChildren.push(instance.destroy);
          }
        })());
        preview.removeAttribute('tabindex');
      }
      if (block.type==='code' && /^(?:```+|~~~+)chart[ \t]*\r?\n/.test(block.raw)) {
        preview.innerHTML=chartMarkup(blockText(block));
      }
      const embedded=/^\[\[([^\]]+\.base)\]\]$/.exec(block.raw.trim());
      if(embedded && ctx.state.files.some(file=>file.path===embedded[1])){
        preview.className='n-embedded-database';preview.innerHTML='';
        ctx.run((async()=>{const baseDoc=await ctx.loadDocument(embedded[1]);if(!alive||!preview.isConnected)return;const instance=mountDatabase(preview,{id:tab.id+'-'+block.id,path:baseDoc.path},true);cleanupChildren.push(()=>instance.destroy());})());
      }
      preview.onclick=event=>{if(event.target.closest('a,input,summary,.n-database,.n-canvas-embed'))return;edit(block.id);};
      preview.onkeydown=event=>{
        if (event.target !== preview || event.isComposing || event.ctrlKey || event.metaKey) return;
        if (event.key === 'Enter' || (event.key === '/' && !block.raw)) {
          event.preventDefault(); edit(block.id,event.key === '/');
        }
      };
      $$('input[type=checkbox]',preview).forEach(input=>{input.disabled=false;input.onchange=()=>{block.raw=block.raw.replace(/\[([ xX])\]/,input.checked?'[x]':'[ ]');commit();};});
      const insert=$('.n-block-insert',el);
      insert.onmousedown=event=>event.preventDefault();
      insert.onclick=()=>insertBlock(block);
      const handle=$('.n-block-handle',el);handle.onclick=event=>blockMenu(event,block);
      handle.ondragstart=event=>{dragged=block.id;event.dataTransfer.setData('application/sc-block',block.id);};
      el.ondragover=event=>{if(dragged){event.preventDefault();el.classList.add('n-drop-target');}};el.ondragleave=()=>el.classList.remove('n-drop-target');
      el.ondrop=event=>{event.preventDefault();el.classList.remove('n-drop-target');const index=blocks.findIndex(b=>b.id===dragged),target=blocks.indexOf(block);if(index<0||index===target)return;const [moving]=blocks.splice(index,1);blocks.splice(target,0,moving);dragged=null;commit();drawList();};
      return el;
    }
    function mention(block, from = 0, to = from) {
      pickReference({ currentPath: tab.path, onSelect: path => {
        if (!alive || (block && !blocks.includes(block))) return;
        const reference = fileMention(path);
        if (block) {
          const text = blockText(block);
          block.raw = updateBlockText(block, text.slice(0, from) + reference + ' ' + text.slice(to));
        } else {
          blocks.push({ id: uuid(), type: 'text', raw: reference });
        }
        commit();
        drawList();
      } });
    }
    function selectBlock(block) {
      const el=$(`[data-block="${block.id}"]`,container);
      if(!el)return;
      el.classList.add('n-block-selected');
      el.focus({preventScroll:true});
      el.scrollIntoView({block:'nearest'});
    }
    function removeBlock(block) {
      blocks=blocks.filter(item=>item!==block);
      if(!blocks.length)blocks=[{id:uuid(),type:'text',raw:''}];
      commit();drawList();
    }
    function findInPage() {
      ctx.chooser('Buscar en esta nota','Buscar texto…',query=>blocks.map(block=>({block,text:blockText(block)}))
        .filter(({text})=>text.trim()&&normalized(text).includes(normalized(query.trim()))).slice(0,60)
        .map(({block,text})=>{
          const index=Math.max(0,normalized(text).indexOf(normalized(query.trim()))-30);
          return {label:text.slice(index,index+150).replace(/\s+/g,' '),icon:'FileText',action:()=>selectBlock(block)};
        }));
    }
    function setInlineValue(block,value) {
      if(!alive||!blocks.includes(block))return;
      block.raw=updateBlockText(block,value);
      commit();drawList();edit(block.id);
    }
    function insertInline(block,value,from,to) {
      const text=blockText(block);
      setInlineValue(block,text.slice(0,from)+value+text.slice(to));
      const input=$('.n-block-textarea',container);
      input?.setSelectionRange(from+value.length,from+value.length);
    }
    function dateMention(date) { return '['+date+'](/cerebro?fecha='+encodeURIComponent(date)+')'; }
    function mentionMenu(block,from,to) {
      const today=new Date();
      const day=offset=>localDate(new Date(today.getFullYear(),today.getMonth(),today.getDate()+offset));
      ctx.chooser('Mencionar','Busca una página, fecha o recordatorio…',query=>[
        ...[['Hoy',0],['Mañana',1],['Ayer',-1]].map(([label,offset])=>({
          label,detail:day(offset),icon:'CalendarDays',action:()=>insertInline(block,dateMention(day(offset)),from,to),
        })),
        {label:'Elegir fecha',icon:'CalendarDays',action:()=>pickDate('Mencionar fecha','date',day(0),date=>insertInline(block,dateMention(date),from,to))},
        ...(dateOnly(query.trim())?[{label:query.trim(),icon:'CalendarDays',action:()=>insertInline(block,dateMention(query.trim()),from,to)}]:[]),
        {label:'Recordatorio',icon:'CalendarDays',detail:'Recordar · Añade un aviso al calendario.',action:()=>pickDate('Nuevo recordatorio','datetime-local',day(0)+'T09:00',async (when,label)=>{
          if(!alive||!blocks.includes(block))return;
          if(!label||!Number.isFinite(Date.parse(when)))throw new Error('Introduce un título y una fecha válida.');
          const item={id:uuid(),label,when};
          insertInline(block,'🔔 '+dateMention(when.slice(0,10)),from,to);
          update(doc,setProperties(doc.content,{recordatorios:[...remindersFor(readPage(doc.content)),item]}),tab.id);
          await ctx.saveDocument(doc);
          ctx.notify('Recordatorio añadido al calendario.');
        },false,title(doc.path))},
        {label:'Mencionar archivo…',icon:'AtSign',action:()=>mention(block,from,to)},
        ...ctx.state.files.filter(file=>!file.is_dir&&file.path!==doc.path&&/\.md$/i.test(file.path))
          .map(file=>({label:title(file.path),detail:file.path,icon:'FileText',action:()=>insertInline(block,'[['+file.path.replace(/\.md$/i,'')+']]',from,to)})),
        ...(query.trim()?[{label:'Mencionar a '+query.trim(),icon:'AtSign',detail:'Mención de texto.',action:()=>insertInline(block,'@'+query.trim(),from,to)}]:[]),
      ].filter(item=>normalized(item.label+' '+(item.detail||'')).includes(normalized(query.trim()))).slice(0,70));
    }
    function linkPage(block, from, to) {
      const attach=path=>{
        const raw='[['+path.replace(/\.md$/i,'')+']]';
        if (!alive || (block && !blocks.includes(block))) return;
        if (block && from!==undefined) { insertInline(block,raw,from,to);return; }
        if (block) { block.type='text';block.raw=raw; }
        else blocks.push({id:uuid(),type:'text',raw});
        commit();drawList();
      };
      ctx.chooser('Enlazar una página','Buscar por nombre o carpeta…',query=>ctx.state.files
        .filter(file=>!file.is_dir && file.path!==doc.path && /\.(md|base|canvas)$/i.test(file.path) && file.path.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()))
        .slice(0,60).map(file=>({label:title(file.path),detail:file.path,icon:'Links',action:()=>attach(file.path)}))
        .concat(from===undefined?[]:[{label:'Crear subpágina'+(query.trim()?' «'+query.trim()+'»':''),icon:'FilePlus2',action:()=>newPage(pageTemplates[0],doc,async path=>{
          attach(path);await ctx.saveDocument(doc);
        },query.trim())}]));
    }
    function edit(id, openPicker = false) {
      const block=blocks.find(b=>b.id===id),el=$(`[data-block="${id}"]`,container);if(!block||!el||$('.n-block-textarea',el))return;
      const preview=$('.n-block-render,.n-canvas-embed',el);if(!preview)return;
      if (preview.classList.contains('n-canvas-embed')) {
        preview.disposeCanvas?.();
        preview.replaceWith(Object.assign(document.createElement('div'),{className:'n-block-render'}));
        return edit(id,openPicker);
      }
      preview.innerHTML='';
      const textarea=document.createElement('textarea');textarea.className='n-block-textarea';textarea.setAttribute('aria-label','Contenido del bloque');textarea.placeholder='Escribe / para elegir un tipo de bloque';textarea.value=blockText(block);textarea.spellcheck=ctx.prefs.spellcheck;preview.append(textarea);
      const resize=()=>{textarea.style.height='auto';textarea.style.height=Math.max(30,textarea.scrollHeight)+'px';};resize();textarea.focus();textarea.setSelectionRange(textarea.value.length,textarea.value.length);
      const finish=()=>{if(!alive||!textarea.isConnected)return;if(blockPicker?.anchor===textarea)blockPicker.close();const replacement=renderBlock(block);el.replaceWith(replacement);};
      textarea.onblur=finish;
      let remembered=false, emojiState;
      textarea.oninput=event=>{
        resize();
        if (event.isComposing) return;
        if (block.type==='text' && /^\/[^/\n]*$/.test(textarea.value) && (!block.raw || blockPicker?.anchor===textarea)) {
          if (blockPicker?.anchor!==textarea) slash(block,textarea,true);
          blockPicker.filter(textarea.value.slice(1));
          return;
        }
        const end=textarea.selectionStart, token=/(?:^|\s):([\p{L}\p{N}_-]*)$/u.exec(textarea.value.slice(0,end));
        if(token && !['code','raw'].includes(block.type) && event.inputType!=='insertFromPaste') {
          const start=end-token[1].length-1;
          emojiState={before:textarea.value.slice(0,start),after:textarea.value.slice(end)};
          if(blockPicker?.kind!=='emoji')slash(block,textarea,true,emojis.map(([symbol,label])=>({
            label:label.split(' ')[0],symbol,aliases:label,detail:'',action:()=>{
              setInlineValue(block,emojiState.before+symbol+emojiState.after);
              $('.n-block-textarea',container)?.setSelectionRange(emojiState.before.length+symbol.length,emojiState.before.length+symbol.length);
            },
          })));
          blockPicker.filter(token[1]);
          return;
        }
        if (blockPicker?.anchor===textarea) blockPicker.close(false);
        if(!remembered){remember();remembered=true;}
        const type=block.type==='text'&&event.inputType!=='insertFromPaste'&&prefixBlock(textarea.value);
        if(type) {
          block.type=type;block.raw='';textarea.value='';
          textarea.placeholder=type==='toggle'?'Título del desplegable. Añade el contenido en la siguiente línea.':'Escribe el contenido del bloque…';
        }
        block.raw=updateBlockText(block,textarea.value);commit(false);
      };
      textarea.oncompositionend=()=>textarea.oninput(new Event('input'));
      textarea.onkeydown=event=>{
        if (event.isComposing) return;
        if (blockPicker?.anchor===textarea && blockPicker.keydown(event)) return;
        if (event.key === '@' && !event.isComposing && !['code', 'raw'].includes(block.type)
          && atMentionStart(textarea.value.slice(0, textarea.selectionStart))) {
          event.preventDefault();
          mentionMenu(block,textarea.selectionStart,textarea.selectionEnd);
          return;
        }
        if(event.key==='['&&!['code','raw'].includes(block.type)&&textarea.value[textarea.selectionStart-1]==='[') {
          event.preventDefault();linkPage(block,textarea.selectionStart-1,textarea.selectionEnd);return;
        }
        if(event.key==='Escape'){event.preventDefault();event.stopPropagation();textarea.blur();selectBlock(block);return;}
        if((event.ctrlKey||event.metaKey)&&!event.getModifierState('AltGraph')&&((!event.altKey&&['b','i'].includes(event.key.toLowerCase()))||(event.altKey&&event.code==='KeyK'))){event.preventDefault();event.stopPropagation();const wrap=event.key.toLowerCase()==='b'?'**':event.key.toLowerCase()==='i'?'*':'[[',end=wrap==='[['?']]':wrap;const {selectionStart:start,selectionEnd:stop,value}=textarea;textarea.value=value.slice(0,start)+wrap+value.slice(start,stop)+end+value.slice(stop);textarea.setSelectionRange(start+wrap.length,stop+wrap.length);textarea.dispatchEvent(new Event('input',{bubbles:true}));return;}
        if(event.key==='Enter'&&!event.shiftKey&&!event.ctrlKey&&!event.metaKey&&!['code','toggle','raw'].includes(block.type)){
          event.preventDefault();block.raw=updateBlockText(block,textarea.value);textarea.onblur=null;commit();if(!textarea.value.trim()&&['task','bullet','number'].includes(block.type)){block.type='text';block.raw='';commit();drawList();edit(block.id);return;}const next=block.type==='task'?{id:uuid(),type:'task',raw:'- [ ] '}:block.type==='bullet'?{id:uuid(),type:'bullet',raw:'- '}:block.type==='number'?{id:uuid(),type:'number',raw:String(Number(block.raw.match(/^\d+/)?.[0]||1)+1)+'. '}:{id:uuid(),type:'text',raw:''};blocks.splice(blocks.indexOf(block)+1,0,next);commit();drawList();edit(next.id);
        }
        if(event.key==='Backspace'&&!textarea.value&&blocks.length>1){event.preventDefault();const index=blocks.indexOf(block);textarea.onblur=null;blocks.splice(index,1);commit();drawList();edit(blocks[Math.max(0,index-1)].id);}
      };
      if (openPicker) {
        textarea.value='/';
        textarea.setSelectionRange(1,1);
        textarea.oninput(new Event('input'));
      }
    }
    function changeType(block,definition){block.type=definition.type;block.raw=definition.raw;commit();drawList();if(!['divider','database','page'].includes(definition.type))edit(block.id);}
    function insertCanvas(block,path) {
      if (!alive||!blocks.includes(block)) return;
      block.type='text';block.raw='![['+path+']]';commit();drawList();
    }
    function chooseCanvas(block) {
      ctx.chooser('Insertar lienzo','Buscar un lienzo…',query=>[
        {label:'Crear lienzo',icon:'Plus',action:()=>ctx.promptName('Nuevo lienzo','Mi lienzo','Nombre del lienzo',async name=>{
          const path=choosePath(name,'.canvas');
          await postFile(path,JSON.stringify({nodes:[],edges:[]}));
          await ctx.refreshFiles();
          insertCanvas(block,path);
        })},
        ...ctx.state.files.filter(file=>!file.is_dir&&/\.canvas$/i.test(file.path)&&file.path.toLocaleLowerCase().includes(query.toLocaleLowerCase()))
          .map(file=>({label:title(file.path),detail:file.path,icon:'LayoutDashboard',action:()=>insertCanvas(block,file.path)})),
      ]);
    }
    function slash(block,anchor,typing=false,customChoices=null){
      blockPicker?.close();
      const plain=blockText(block);
      const insertCreated=async path=>{
        if (!alive || !blocks.includes(block)) return;
        block.type='text';block.raw='[['+path.replace(/\.md$/,'')+']]';commit();await ctx.saveDocument(doc);
      };
      const convert=(label,icon,type,detail,raw,aliases='')=>({label,icon,detail,aliases,action:()=>changeType(block,{type,raw})});
      const choices=customChoices||[
        ...(typing ? [{label:'Preguntar a la IA',icon:'MessageSquare',detail:'Abre el chat con esta nota como contexto.',aliases:'ai ask chat',action:()=>ctx.chatWithNote(doc.path)}] : []),
        {group:'Bloques básicos'},
        convert('Texto','Type','text','Escribe un párrafo.',plain,'paragraph text'),
        ...[1,2,3].map(n=>convert('Encabezado '+n,'H'+n,'h'+n,['Título principal.','Título de sección.','Título de subsección.'][n-1],'#'.repeat(n)+' '+plain,'titulo heading h'+n)),
        convert('Código','CodeXml','code','Escribe código con formato.',updateBlockText({type:'code',raw:''},plain),'code'),
        convert('Cita','Quote','quote','Destaca un fragmento de texto.',updateBlockText({type:'quote'},plain),'quote'),
        convert('Nota destacada','Gem','callout','Resalta una idea o un aviso.','> [!note] Recuerda\n> '+(plain||'Una idea que merece atención.'),'callout aviso'),
        convert('Desplegable','ChevronDown','toggle','Oculta contenido bajo un título.','<details>\n<summary>'+escape(plain||'Una idea por descubrir')+'</summary>\n\nEscribe el contenido aquí.\n\n</details>','toggle'),
        convert('Divisor','Minus','divider','Separa dos secciones.','---','separator separador'),
        {group:'Listas y tablas'},
        convert('Lista con viñetas','List','bullet','Agrupa ideas sin un orden fijo.','- '+plain,'list bullet'),
        convert('Lista numerada','ListFilter','number','Organiza pasos en orden.','1. '+plain,'list number'),
        convert('Lista de tareas','Check','task','Añade una casilla para marcar.','- [ ] '+plain,'todo checklist task'),
        convert('Tabla sencilla','Columns2','raw','Crea una tabla dentro de la nota.','| Columna 1 | Columna 2 |\n| --- | --- |\n| Texto | Texto |','table'),
        {group:'Enlaces y contenido'},
        ...(typing ? [
          {label:'Enlace a una página',icon:'Links',detail:'Conecta esta nota con otra página.',aliases:'wiki link referencia',action:()=>linkPage(block)},
          {label:'Mencionar archivo',icon:'AtSign',detail:'Busca una nota, documento o presentación.',aliases:'mention archivo reference',action:()=>mention(block)},
          {label:'Subpágina',icon:'FilePlus2',detail:'Crea una página dentro de esta nota.',aliases:'page pagina',action:()=>newPage(pageTemplates[0],doc,insertCreated)},
          {label:'Colección en la página',icon:'Columns2',detail:'Inserta una base de datos editable.',aliases:'tabla database base de datos',action:()=>createDatabase(doc,insertCreated)},
          {label:'Calendario',icon:'CalendarDays',detail:'Organiza eventos por fecha dentro de la nota.',aliases:'calendar agenda',action:()=>createDatabase(doc,insertCreated,'calendar')},
          {label:'Gráfico de barras',icon:'List',detail:'Edita una lista de etiquetas y valores.',aliases:'chart barras grafico',action:()=>changeType(block,{type:'code',raw:'```chart\nEnero: 12\nFebrero: 18\nMarzo: 9\n```'})},
          {label:'Lienzo',icon:'LayoutDashboard',detail:'Crea o inserta un canvas editable.',aliases:'canvas pizarra tablero',action:()=>chooseCanvas(block)},
          convert('Fecha de hoy','CalendarDays','text','Inserta la fecha en formato AAAA-MM-DD.',localDate(),'date today'),
        ] : []),
        {label:'Imagen por URL',icon:'Image',detail:'Inserta una imagen desde un enlace.',aliases:'image foto',action:()=>ctx.promptName('Insertar imagen','','URL de la imagen (https://…)',url=>{
          if(!/^https?:\/\//i.test(url))throw new Error('Introduce una URL http o https válida.');
          changeType(block,{type:'raw',raw:'![Imagen]('+url.replaceAll(')','%29')+')'});
        })},
        {group:'Más encabezados'},
        ...[4,5,6].map(n=>convert('Encabezado '+n,'Type','h'+n,'Crea un subtítulo de nivel '+n+'.','#'.repeat(n)+' '+plain,'titulo heading h'+n)),
      ];
      const popup=document.createElement('div'), id='block-picker-'+uuid();
      popup.className='n-block-picker';
      popup.innerHTML=(typing?'':'<input type="search" placeholder="Buscar un bloque…" aria-label="Buscar un bloque" autocomplete="off">')+`<div id="${id}" class="n-block-options" role="listbox" aria-label="Tipos de bloque"></div>`;
      const list=$('.n-block-options',popup), input=typing?anchor:$('input',popup);
      input.setAttribute('aria-controls',id);
      input.setAttribute('aria-expanded','true');
      let rows=[], selected=0;
      const normalize=value=>value.toLocaleLowerCase().normalize('NFD').replace(/\p{Diacritic}/gu,'');
      function position() {
        const rect=anchor.getBoundingClientRect(), below=innerHeight-rect.bottom-10, above=rect.top-10;
        const flip=below<Math.min(360,list.scrollHeight+16) && above>below;
        popup.style.maxHeight=Math.max(0,flip?above:below)+'px';
        popup.style.left=Math.max(8,Math.min(rect.left,innerWidth-popup.offsetWidth-8))+'px';
        popup.style.top=Math.max(8,flip?rect.top-popup.offsetHeight-4:rect.bottom+4)+'px';
      }
      function highlight(index,scroll=false) {
        selected=index;
        rows.forEach((row,i)=>row.setAttribute('aria-selected',String(i===index)));
        if (rows[index]) {
          input.setAttribute('aria-activedescendant',rows[index].id);
          if (scroll) rows[index].scrollIntoView({block:'nearest'});
        } else input.removeAttribute('aria-activedescendant');
      }
      function filter(query) {
        const words=normalize(query).trim().split(/\s+/).filter(Boolean);
        rows=[];
        list.replaceChildren();
        let group, heading;
        for (const item of choices) {
          if (item.group) { group=item.group;heading=null;continue; }
          const haystack=normalize(item.label+' '+item.detail+' '+item.aliases);
          if (!words.every(word=>haystack.includes(word))) continue;
          if (group&&!heading) {
            heading=document.createElement('div');heading.className='n-block-group';heading.textContent=group;list.append(heading);
          }
          const row=document.createElement('button'), index=rows.length;
          row.type='button';row.id=id+'-'+index;row.tabIndex=-1;row.setAttribute('role','option');
          row.innerHTML=`<span class="n-block-option-icon">${item.symbol?escape(item.symbol):I(item.icon,16)}</span><span><strong>${escape(item.label)}</strong><small>${escape(item.detail)}</small></span>`;
          row.onpointermove=()=>highlight(index);
          row.onmousedown=event=>event.preventDefault();
          row.onclick=()=>{close();invoke(item.action);};
          rows.push(row);list.append(row);
        }
        if (!rows.length) list.innerHTML='<p role="status">Sin bloques que coincidan.</p>';
        list.scrollTop=0;
        highlight(0);
        position();
      }
      function keydown(event) {
        if (event.isComposing || !['ArrowDown','ArrowUp','Enter','Escape','Tab'].includes(event.key) || event.ctrlKey || event.metaKey || event.altKey) return false;
        if (event.key==='Tab') { close();return false; }
        event.preventDefault();event.stopPropagation();
        if (event.key==='Escape') { close();if(!typing)anchor.focus(); }
        else if (event.key==='Enter') rows[selected]?.click();
        else if (rows.length) highlight((selected+(event.key==='ArrowDown'?1:-1)+rows.length)%rows.length,true);
        return true;
      }
      function dismiss(event) { if (!popup.contains(event.target)&&event.target!==anchor) close(); }
      function close(reset=true) {
        blockPicker=null;
        if (typing&&reset) { input.value=blockText(block);input.style.height='auto';input.style.height=Math.max(30,input.scrollHeight)+'px'; }
        for (const attr of ['aria-controls','aria-expanded','aria-activedescendant']) input.removeAttribute(attr);
        document.removeEventListener('pointerdown',dismiss,true);
        window.removeEventListener('resize',position);
        window.removeEventListener('scroll',position,true);
        popup.remove();
      }
      blockPicker={anchor,close,filter,keydown,kind:customChoices?'emoji':'slash'};
      document.body.append(popup);
      document.addEventListener('pointerdown',dismiss,true);
      window.addEventListener('resize',position);
      window.addEventListener('scroll',position,true);
      if (!typing) { input.oninput=()=>filter(input.value);input.onkeydown=keydown;input.focus({preventScroll:true}); }
      filter('');
    }
    function blockMenu(event,block){const anchor=event.currentTarget.closest('.n-block');ctx.menu(event,[{icon:'Pencil',label:'Editar bloque',action:()=>edit(block.id)},{icon:'ListFilter',label:'Convertir en…',action:()=>slash(block,anchor)},{icon:'Files',label:'Duplicar',action:()=>{blocks.splice(blocks.indexOf(block)+1,0,{...block,id:uuid()});commit();drawList();}},{icon:'ArrowLeft',label:'Mover arriba',action:()=>{const index=blocks.indexOf(block);if(index>0){[blocks[index-1],blocks[index]]=[blocks[index],blocks[index-1]];commit();drawList();}}},{icon:'ArrowRight',label:'Mover abajo',action:()=>{const index=blocks.indexOf(block);if(index<blocks.length-1){[blocks[index+1],blocks[index]]=[blocks[index],blocks[index+1]];commit();drawList();}}},null,{icon:'Trash2',label:'Eliminar bloque',danger:true,action:()=>{blocks=blocks.filter(b=>b!==block);if(!blocks.length)blocks=[{id:uuid(),type:'text',raw:''}];commit();drawList();}}]);}
    async function saveAsTemplate(){ctx.promptName('Guardar como plantilla',title(doc.path),'Nombre de tu plantilla',async name=>{const path=choosePath('Plantillas/'+name,'.md');const page=readPage(doc.content);await postFile(path,writePage({...page.meta,template:true,parent:undefined,_database:undefined},page.body));await ctx.refreshFiles();ctx.notify('Plantilla guardada en tu biblioteca.');});}
    async function drawSubpages(){const pages=await allPages();if(!alive)return;const children=pages.filter(page=>page.meta.parent===doc.path),root=$('.n-subpages',container);if(!root)return;root.innerHTML=children.length?`<div class="n-eyebrow">DENTRO DE ESTA PÁGINA</div>${children.map(child=>`<button data-child="${escape(child.path)}">${I(child.meta.icon||'FileText',17)}<span>${escape(child.title)}</span>${I('ArrowUpRight',13)}</button>`).join('')}`:'';$$('[data-child]',root).forEach(el=>el.onclick=()=>ctx.run(ctx.openNote(el.dataset.child,{mode:'blocks'})));}
    draw();return{destroy(){alive=false;blockPicker?.close();cleanupChildren.forEach(fn=>fn());},refresh:draw,mention:()=>mention(),link:()=>linkPage(),insert:()=>insertBlock(),find:findInPage};
  }

  function renderPageProperties(root,doc,tab,redraw){
    const page=readPage(doc.content);const reserved=new Set(['icon','cover','editor','parent','template','_database','fecha_calendario','recordatorios']);
    const entries=Object.entries(page.meta).filter(([key])=>!reserved.has(key));
    root.innerHTML=(page.meta.parent&&ctx.state.files.some(file=>file.path===page.meta.parent)?`<button class="n-parent-collection" data-parent-page>${I('FileText',13)}${escape(title(page.meta.parent))}${I('ArrowUpRight',12)}</button>`:'')+entries.map(([key,value])=>`<div class="n-page-property"><span>${I(typeof value==='boolean'?'Check':Array.isArray(value)?'Tags':'List',14)}${escape(key)}</span><button data-property="${escape(key)}">${escape(Array.isArray(value)?value.join(', '):String(value??''))||'<span class="muted">Vacío</span>'}</button></div>`).join('')+`<button class="n-add-property" data-add-property>${I('Plus',13)} Añadir propiedad</button>`;
    if($('[data-parent-page]',root))$('[data-parent-page]',root).onclick=()=>ctx.run(ctx.openNote(page.meta.parent,{mode:'blocks'}));
    $('[data-add-property]',root).innerHTML=I('Plus',13)+' Añadir propiedad';
    $$('[data-property]',root).forEach(el=>el.onclick=()=>editProperty(el.dataset.property));
    $('[data-add-property]',root).onclick=()=>ctx.promptName('Añadir propiedad','','Nombre (letras, números y guiones)',key=>{if(!isPropertyKey(key)||pageReservedKeys.has(key))throw new Error('Nombre no válido o reservado por la página.');editProperty(key);});
    if(page.meta._database)ctx.run((async()=>{const base=await ctx.loadDocument(page.meta._database);const config=JSON.parse(base.content);const pages=await allPages();if(!root.isConnected)return;const fields=config.fields||[],row={...page,path:doc.path,title:title(doc.path)};root.innerHTML=`<button class="n-parent-collection" data-return-to-base>${I('Columns2',13)}${escape(config.name||title(base.path))}${I('ArrowUpRight',12)}</button>`+fields.map(field=>{const value=fieldValue(row,field,pages),computed=['formula','rollup'].includes(field.type);let display=Array.isArray(value)?value.join(', '):String(value??'');if(field.type==='checkbox')display=value?'✓ Sí':'No';return `<div class="n-page-property"><span>${I(fieldIcons[field.type]||'List',14)}${escape(field.name)}</span><button data-db-page-prop="${escape(field.key)}" ${computed?'disabled':''}>${escape(display)||'<span class="muted">Vacío</span>'}</button></div>`;}).join('');$('[data-return-to-base]',root).onclick=()=>ctx.run(ctx.openNote(base.path));$$('[data-db-page-prop]',root).forEach(el=>el.onclick=()=>editTypedProperty(fields.find(f=>f.key===el.dataset.dbPageProp),config,pages));})());
    function editTypedProperty(field,config,pages){
      const value=readPage(doc.content).meta[field.key];
      if(field.type==='checkbox'){update(doc,setProperties(doc.content,{[field.key]:!value}),tab.id);redraw();return;}
      let control;
      if(field.type==='select')control=`<select id="typedPropValue" class="text-input"><option value="">Sin asignar</option>${(field.options||[]).map(option=>`<option value="${escape(option)}" ${value===option?'selected':''}>${escape(option)}</option>`).join('')}</select>`;
      else if(field.type==='relation'){const selected=new Set(relations(value));control=`<div class="n-relation-list">${pages.filter(p=>p.path!==doc.path).map((p,index)=>`<label><input type="checkbox" name="typedRelation" value="${escape(p.path)}" ${selected.has(p.path)?'checked':''}>${I('FileText',14)}<span>${escape(p.title)}</span></label>`).join('')}</div>`;}
      else control=`<input id="typedPropValue" class="text-input" type="${field.type==='date'?'date':field.type==='number'?'number':field.type==='url'?'url':'text'}" ${field.type==='number'?'step="any"':''} value="${escape(Array.isArray(value)?value.join(', '):String(value??''))}">`;
      ctx.dialog(field.name,`<form id="typedPropForm"><label class="field-label" for="typedPropValue">${escape(field.name)}${field.type==='multi'?' · valores separados por comas':''}</label>${control}<div class="form-actions"><button class="primary-button">Guardar</button></div></form>`);
      $('#typedPropForm').onsubmit=event=>{event.preventDefault();invoke(()=>{const raw=$('#typedPropValue')?.value;const next=field.type==='relation'?$$('input[name=typedRelation]:checked').map(el=>`[[${el.value.replace(/\.md$/,'')}]]`).join(', '):field.type==='multi'?raw.split(',').map(s=>s.trim()).filter(Boolean):field.type==='number'&&raw!==''?Number(raw):raw;if(field.type==='number'&&next!==''&&!Number.isFinite(next))throw new Error('Introduce un número válido.');update(doc,setProperties(doc.content,{[field.key]:next}),tab.id);ctx.closeOverlay();redraw();});};
    }
    function editProperty(key){const value=page.meta[key];ctx.dialog('Propiedad: '+key,`<form id="pagePropForm"><label class="field-label" for="pagePropType">Tipo</label><select id="pagePropType" class="text-input"><option value="text">Texto</option><option value="number" ${typeof value==='number'?'selected':''}>Número</option><option value="checkbox" ${typeof value==='boolean'?'selected':''}>Casilla</option><option value="date">Fecha</option><option value="multi" ${Array.isArray(value)?'selected':''}>Etiquetas separadas por comas</option></select><label class="field-label" for="pagePropValue">Valor</label><input class="text-input" id="pagePropValue" value="${escape(Array.isArray(value)?value.join(', '):String(value??''))}"><p class="empty-hint">Para una casilla, escribe true o false.</p><div class="form-actions"><button class="secondary-button" type="button" id="removePageProp">Quitar propiedad</button><button class="primary-button">Guardar</button></div></form>`);$('#pagePropForm').onsubmit=event=>{event.preventDefault();invoke(()=>{const type=$('#pagePropType').value,raw=$('#pagePropValue').value;const converted=type==='number'?Number(raw):type==='checkbox'?raw==='true':type==='multi'?raw.split(',').map(s=>s.trim()).filter(Boolean):raw;if(type==='number'&&!Number.isFinite(converted))throw new Error('Introduce un número válido.');update(doc,setProperties(doc.content,{[key]:converted}),tab.id);ctx.closeOverlay();redraw();});};$('#removePageProp').onclick=()=>{update(doc,setProperties(doc.content,{[key]:undefined}),tab.id);ctx.closeOverlay();redraw();};}
  }

  function mountDatabase(container,tab,inline=false){
    const doc=ctx.state.documents.get(tab.path);let config,alive=true,rows=[],all=[],generation=0,timer;
    try{config=JSON.parse(doc.content);if(config.version!==1||!Array.isArray(config.fields)||!Array.isArray(config.views)||!config.views.length)throw new Error();}catch{container.innerHTML='<p class="empty-hint">Esta colección no tiene un formato compatible. El archivo permanece intacto.</p>';return{destroy(){},refresh(){}};}
    function saveConfig(){update(doc,JSON.stringify(config,null,2),tab.id);}
    const view=()=>config.views.find(v=>v.id===config.activeView)||config.views[0];
    async function draw(){
      const request=++generation;all=await allPages();if(!alive||request!==generation||!container.isConnected)return;
      rows=all.filter(row=>row.meta._database===doc.path);const current=view(),visible=queryRows(rows,config,all);
      container.innerHTML=`<div class="n-database ${inline?'n-db-inline':''}">${inline?'':coverMarkup(config.cover||'tapestry','ORDO · EX COGITATIONE')}<div class="n-db-inner">${inline?'':`<div class="n-eyebrow">COLECCIÓN DE TU ARCHIVO</div><div class="n-db-emblem">${I('Columns2',32)}</div>`}<div class="n-db-title-row"><h1>${escape(config.name||title(doc.path))}</h1><button class="n-mini-button" data-n-action="db-menu" title="Opciones de la colección" aria-label="Opciones de la colección">${I('MoreHorizontal',20)}</button></div>${!inline?`<p class="n-db-description">${escape(config.description||'Cada registro es una página. Cada página, una posibilidad.')}</p>`:''}<div class="n-db-toolbar"><div class="n-db-views" role="tablist" aria-label="Vistas de la colección">${config.views.map(v=>`<button role="tab" aria-selected="${v.id===current.id}" data-view="${escape(v.id)}" class="${v.id===current.id?'selected':''}">${I(viewIcons[v.type]||'List',14)}${escape(v.name)}</button>`).join('')}</div><button class="n-mini-button" data-n-action="add-view" aria-label="Añadir vista" title="Añadir vista">${I('Plus',16)}</button><div class="n-db-toolbar-end">${actionButton('ListFilter','Filtrar','filter','n-db-tool '+(current.filters?.length?'applied':''))}${actionButton('ChevronsUpDown','Ordenar','sort','n-db-tool '+(current.sort?.key?'applied':''))}<button class="n-mini-button" data-n-action="db-options" aria-label="Ajustar vista" title="Ajustar vista">${I('SlidersHorizontal',16)}</button>${actionButton('Plus','Nuevo','new-record','n-gold-button n-db-new')}</div></div><div class="n-db-querybar"><label>${I('Search',14)}<input class="n-db-search" aria-label="Buscar en la colección" placeholder="Buscar en esta colección…" value="${escape(config.search||'')}"></label><span>${visible.length} de ${rows.length} páginas</span>${current.filters?.length?'<button data-n-action="clear-filters" class="n-small-link">Quitar filtros</button>':''}${config.showArchived?'<span class="n-status-chip">Archivo</span>':''}</div><div class="n-db-view-content"></div><div class="n-db-bottom">${I('FileText',13)} Cada registro vive como una nota Markdown en tu bóveda.<span></span>${actionButton('Plus','Propiedad','add-property','n-small-link')}</div></div></div>`;
      const root=$('.n-database',container),content=$('.n-db-view-content',root);
      if(current.type==='board')renderBoard(content,visible,current);else if(current.type==='gallery')renderGallery(content,visible);else if(current.type==='calendar')renderCalendar(content,visible,current);else if(current.type==='list')renderList(content,visible);else renderTable(content,visible);
      $$('[data-view]',root).forEach(el=>el.onclick=()=>{config.activeView=el.dataset.view;saveConfig();ctx.run(draw());});
      const search=$('.n-db-search',root);search.oninput=()=>{clearTimeout(timer);const position=search.selectionStart;timer=setTimeout(()=>{config.search=search.value;saveConfig();ctx.run(draw().then(()=>{const input=$('.n-db-search',container);input?.focus();input?.setSelectionRange(position,position);}));},220);};
      bindLocal(root,{'new-record':()=>newRecord(),filter:filterDialog,sort:sortDialog,'clear-filters':()=>{current.filters=[];saveConfig();return draw();},'add-property':()=>propertyDialog(),'add-view':addView,'db-options':optionsDialog,'db-menu':event=>databaseMenu(event)});
    }
    function chip(value,index=0){return `<span class="n-status-chip color-${index%5}">${escape(String(value))}</span>`;}
    function renderTable(root,visible){
      root.innerHTML=`<div class="n-table-scroll"><table class="n-table"><thead><tr><th class="n-title-column">${I('FileText',14)} Nombre</th>${config.fields.map(field=>`<th><button data-field-menu="${escape(field.key)}">${I(fieldIcons[field.type]||'List',13)}${escape(field.name)}</button></th>`).join('')}<th><button data-add-col aria-label="Añadir propiedad">${I('Plus',15)}</button></th></tr></thead><tbody>${visible.map(row=>`<tr data-record="${escape(row.path)}"><td><button class="n-record-title" data-record-open="${escape(row.path)}">${I(row.meta.icon||'FileText',15)}<span>${escape(row.title)}</span></button></td>${config.fields.map(field=>`<td data-cell="${escape(field.key)}" data-record-path="${escape(row.path)}">${cell(row,field)}</td>`).join('')}<td><button data-record-menu="${escape(row.path)}" class="n-mini-button" aria-label="Opciones de ${escape(row.title)}">${I('MoreHorizontal',15)}</button></td></tr>`).join('')}</tbody><tfoot><tr><td colspan="${config.fields.length+2}"><button data-new-row>${I('Plus',14)} Nueva página</button></td></tr></tfoot></table></div>${visible.length?'':'<div class="n-db-empty">Todavía no hay páginas en esta vista.<br><small>Añade una página o ajusta los filtros.</small></div>'}`;
      bindRecords(root);$$('[data-cell]',root).forEach(el=>{const field=config.fields.find(f=>f.key===el.dataset.cell),row=rows.find(r=>r.path===el.dataset.recordPath);if(['formula','rollup'].includes(field.type))return;el.tabIndex=0;el.onclick=event=>{if(!event.target.closest('a'))editCell(el,row,field);};el.onkeydown=event=>{if(event.key==='Enter'){event.preventDefault();editCell(el,row,field);}};});
      $$('[data-field-menu]',root).forEach(el=>el.onclick=()=>propertyDialog(config.fields.find(f=>f.key===el.dataset.fieldMenu)));$('[data-add-col]',root).onclick=()=>propertyDialog();$('[data-new-row]',root).onclick=()=>newRecord();
    }
    function cell(row,field){const value=fieldValue(row,field,all);if(field.type==='url'&&/^https?:\/\//i.test(String(value)))return `<a href="${escape(value)}" target="_blank" rel="noopener noreferrer">${escape(value)}</a>`;if(field.type==='checkbox')return `<span class="n-checkbox-display ${value?'checked':''}">${value?'✓':''}</span>`;if(field.type==='select')return value?chip(value,Math.max(0,(field.options||[]).indexOf(value))):'<span class="n-cell-empty">—</span>';if(field.type==='multi')return(Array.isArray(value)?value:String(value).split(',').filter(Boolean)).map((v,i)=>chip(v,i)).join(' ');if(field.type==='relation')return relations(value).map(path=>`<button data-related="${escape(path)}" class="n-relation-pill">${I('FileText',12)}${escape(title(path))}</button>`).join('')||'<span class="n-cell-empty">—</span>';return `<span class="${['formula','rollup'].includes(field.type)?'n-computed':''}">${escape(String(value))||'<span class="n-cell-empty">—</span>'}</span>`;}
    function editCell(el,row,field){if($('input,select',el))return;if(field.type==='checkbox'){return invoke(()=>setValue(row,field,!row.meta[field.key]));}if(field.type==='relation')return relationDialog(row,field);if(field.type==='multi')return ctx.promptName(field.name,Array.isArray(row.meta[field.key])?row.meta[field.key].join(', '):String(row.meta[field.key]||''),'Valores separados por comas',value=>setValue(row,field,value.split(',').map(s=>s.trim()).filter(Boolean)));
      let input;if(field.type==='select'){input=document.createElement('select');for(const option of ['',...(field.options||[])]){const el=document.createElement('option');el.value=option;el.textContent=option||'Sin asignar';input.append(el);}}else{input=document.createElement('input');input.type=field.type==='number'?'number':field.type==='date'?'date':field.type==='url'?'url':'text';if(field.type==='number')input.step='any';}
      input.className='n-cell-input';input.setAttribute('aria-label',field.name+' de '+row.title);input.value=String(row.meta[field.key]??'');el.innerHTML='';el.append(input);input.focus();let done=false;
      const commit=()=>{if(done)return;done=true;const value=field.type==='number'&&input.value!==''?Number(input.value):input.value;invoke(()=>setValue(row,field,value));};input.onblur=commit;input.onchange=()=>{if(field.type==='select'||field.type==='date')commit();};input.onkeydown=event=>{if(event.key==='Enter'){event.preventDefault();commit();}if(event.key==='Escape'){done=true;ctx.run(draw());}};
    }
    async function setValue(row,field,value){if(field.type==='number'&&value!==''&&!Number.isFinite(value))throw new Error('Número no válido.');const content=setProperties(row.doc.content,{[field.key]:value});update(row.doc,content,tab.id);await ctx.saveDocument(row.doc);await draw();}
    function bindRecords(root){$$('[data-record-open]',root).forEach(el=>el.onclick=event=>{event.stopPropagation();ctx.run(ctx.openNote(el.dataset.recordOpen,{mode:'blocks',newTab:event.ctrlKey||event.metaKey}));});$$('[data-related]',root).forEach(el=>el.onclick=event=>{event.stopPropagation();ctx.run(ctx.openNote(el.dataset.related,{mode:'blocks'}));});$$('[data-record-menu]',root).forEach(el=>el.onclick=event=>{event.stopPropagation();recordMenu(event,rows.find(row=>row.path===el.dataset.recordMenu));});}
    function renderBoard(root,visible,current){const field=config.fields.find(f=>f.key===(current.groupBy||'estado')&&f.type==='select')||config.fields.find(f=>f.type==='select');if(!field){root.innerHTML='<p class="empty-hint">Añade una propiedad de selección para agrupar el tablero.</p>';return;}const groups=[...new Set([...(field.options||[]),...visible.map(row=>String(row.meta[field.key]||''))])];root.innerHTML=`<div class="n-board">${groups.map((name,index)=>{const group=visible.filter(row=>String(row.meta[field.key]||'')===name);return `<section class="n-board-column" data-board-group="${escape(name)}"><header>${chip(name||'Sin asignar',index)}<small>${group.length}</small><button data-add-group="${escape(name)}" aria-label="Añadir página a ${escape(name)}">${I('Plus',15)}</button></header><div class="n-board-cards">${group.map(row=>card(row)).join('')}</div><button class="n-board-add" data-add-group="${escape(name)}">${I('Plus',13)} Nueva página</button></section>`;}).join('')}</div>`;bindRecords(root);$$('[data-add-group]',root).forEach(el=>el.onclick=()=>newRecord({[field.key]:el.dataset.addGroup}));$$('[data-drag-record]',root).forEach(el=>el.ondragstart=event=>event.dataTransfer.setData('application/sc-record',el.dataset.dragRecord));$$('[data-board-group]',root).forEach(el=>{el.ondragover=event=>{event.preventDefault();el.classList.add('n-board-drop');};el.ondragleave=()=>el.classList.remove('n-board-drop');el.ondrop=event=>{event.preventDefault();el.classList.remove('n-board-drop');const path=event.dataTransfer.getData('application/sc-record'),row=rows.find(r=>r.path===path);if(row)invoke(()=>setValue(row,field,el.dataset.boardGroup));};});}
    function card(row){return `<article class="n-record-card" draggable="true" data-drag-record="${escape(row.path)}"><div class="n-record-card-top">${I(row.meta.icon||'FileText',16)}<button class="n-mini-button" data-record-menu="${escape(row.path)}" aria-label="Opciones de ${escape(row.title)}">${I('MoreHorizontal',14)}</button></div><button class="n-record-title" data-record-open="${escape(row.path)}">${escape(row.title)}</button><p>${escape(row.body.replace(/[#>*`\[\]]/g,'').trim().slice(0,95))||'Una página por escribir.'}</p><div class="n-record-card-props">${config.fields.filter(f=>!['text','relation','checkbox'].includes(f.type)&&row.meta[f.key]).slice(0,3).map(field=>cell(row,field)).join(' ')}</div></article>`;}
    function renderGallery(root,visible){root.innerHTML=`<div class="n-gallery">${visible.map(row=>card(row)).join('')}<button class="n-gallery-new" data-gallery-new>${I('Plus',22)} Nueva página</button></div>`;bindRecords(root);$('[data-gallery-new]',root).onclick=()=>newRecord();}
    function renderList(root,visible){root.innerHTML=`<div class="n-record-list">${visible.map(row=>`<div><button class="n-record-title" data-record-open="${escape(row.path)}">${I(row.meta.icon||'FileText',16)}${escape(row.title)}</button><span>${config.fields.filter(f=>['select','date'].includes(f.type)).slice(0,2).map(field=>cell(row,field)).join(' ')}</span><button class="n-mini-button" data-record-menu="${escape(row.path)}" aria-label="Opciones de ${escape(row.title)}">${I('MoreHorizontal',15)}</button></div>`).join('')}<button class="n-board-add" data-list-new>${I('Plus',13)} Nueva página</button></div>`;bindRecords(root);$('[data-list-new]',root).onclick=()=>newRecord();}
    function renderCalendar(root,visible,current){const field=config.fields.find(f=>f.key===current.dateBy&&f.type==='date')||config.fields.find(f=>f.type==='date');if(!field){root.innerHTML='<p class="empty-hint">Añade una propiedad de fecha para usar el calendario.</p>';return;}const month=current.month||localDate().slice(0,7),[year,m]=month.split('-').map(Number),first=new Date(year,m-1,1),offset=(first.getDay()+6)%7,days=new Date(year,m,0).getDate();root.innerHTML=`<div class="n-calendar-controls"><strong>${escape(first.toLocaleDateString('es-ES',{month:'long',year:'numeric'}))}</strong><span></span><button data-month-step="-1" aria-label="Mes anterior">${I('ChevronLeft',17)}</button><button data-calendar-today>Hoy</button><button data-month-step="1" aria-label="Mes siguiente">${I('ChevronRight',17)}</button></div><div class="n-calendar-week">${['Lun','Mar','Mié','Jue','Vie','Sáb','Dom'].map(day=>`<span>${day}</span>`).join('')}</div><div class="n-calendar">${Array.from({length:Math.ceil((offset+days)/7)*7},(_,i)=>{const day=i-offset+1;if(day<1||day>days)return'<div class="n-calendar-day outside"></div>';const date=`${year}-${String(m).padStart(2,'0')}-${String(day).padStart(2,'0')}`;return `<div class="n-calendar-day ${date===localDate()?'today':''}" data-calendar-date="${date}"><header><span>${day}</span><button data-day-new="${date}" aria-label="Crear página para ${date}">${I('Plus',12)}</button></header>${visible.filter(row=>String(row.meta[field.key])===date).map(row=>`<button class="n-calendar-record" data-record-open="${escape(row.path)}">${escape(row.title)}</button>`).join('')}</div>`;}).join('')}</div><div class="n-unscheduled">${visible.filter(row=>!row.meta[field.key]).length} páginas sin fecha</div>`;bindRecords(root);$$('[data-day-new]',root).forEach(el=>el.onclick=()=>newRecord({[field.key]:el.dataset.dayNew}));$$('[data-month-step]',root).forEach(el=>el.onclick=()=>{current.month=localDate(new Date(year,m-1+Number(el.dataset.monthStep),1)).slice(0,7);saveConfig();ctx.run(draw());});$('[data-calendar-today]',root).onclick=()=>{current.month=localDate().slice(0,7);saveConfig();ctx.run(draw());};}
    function newRecord(values={},template=null){ctx.promptName('Nueva página en '+config.name,'Sin título','Nombre de la página',async name=>{const folder=config.folder||doc.path.replace(/\.base$/,'');let path=`${folder}/${name.trim()}.md`,number=2;while(ctx.state.files.some(file=>file.path===path))path=`${folder}/${name.trim()} ${number++}.md`;if(!ctx.validPath(path))throw new Error('Nombre no válido.');const defaults=Object.fromEntries(config.fields.filter(f=>f.type==='select').map(f=>[f.key,f.options?.[0]||'']));const content=writePage({icon:'FileText',editor:'blocks',_database:doc.path,...defaults,...(template?.properties||{}),...values},template?.body||'');await postFile(path,content);await ctx.refreshFiles();await ctx.loadDocument(path);ctx.closeOverlay();await draw();ctx.notify('Página creada en la colección.');});}
    function recordMenu(event,row){ctx.menu(event,[{icon:'ExternalLink',label:'Abrir como página',action:()=>ctx.openNote(row.path,{mode:'blocks'})},{icon:'Files',label:'Duplicar página',action:()=>duplicateRecord(row)},{icon:'BookOpen',label:'Guardar como plantilla de la colección',action:()=>ctx.promptName('Nueva plantilla',row.title,'Nombre de la plantilla',name=>{config.templates||=[];config.templates.push({id:uuid(),name,body:row.body,properties:Object.fromEntries(config.fields.filter(f=>!['formula','rollup'].includes(f.type)).map(f=>[f.key,row.meta[f.key]??'']))});saveConfig();ctx.notify('Plantilla guardada. Disponible en opciones de la colección.');})},null,{icon:'Bookmark',label:row.meta.archived?'Restaurar página':'Archivar página',action:async()=>{update(row.doc,setProperties(row.doc.content,{archived:!row.meta.archived}),tab.id);await ctx.saveDocument(row.doc);await draw();}}]);}
    async function duplicateRecord(row){let path=row.path.replace(/\.md$/,' copia.md'),i=2;while(ctx.state.files.some(file=>file.path===path))path=row.path.replace(/\.md$/,` copia ${i++}.md`);await postFile(path,row.doc.content);await ctx.refreshFiles();await draw();}
    function relationDialog(row,field){const selected=new Set(relations(row.meta[field.key]));const candidates=all.filter(r=>r.path!==row.path);ctx.dialog('Relacionar páginas',`<form id="relationForm"><input class="text-input" id="relationSearch" placeholder="Buscar página…" aria-label="Buscar página para relacionar"><div class="n-relation-list">${candidates.map((page,index)=>`<label data-relation-label="${escape(page.title.toLocaleLowerCase())}"><input type="checkbox" name="relation" value="${index}" ${selected.has(page.path)?'checked':''}>${I(page.meta.icon||'FileText',15)}<span>${escape(page.title)}</span><small>${escape(page.path)}</small></label>`).join('')}</div><div class="form-actions"><button class="primary-button">Guardar relaciones</button></div></form>`);$('#relationSearch').oninput=event=>$$('[data-relation-label]').forEach(el=>el.hidden=!el.dataset.relationLabel.includes(event.target.value.toLocaleLowerCase()));$('#relationForm').onsubmit=event=>{event.preventDefault();const paths=$$('input[name=relation]:checked').map(el=>candidates[Number(el.value)].path.replace(/\.md$/,''));ctx.closeOverlay();invoke(()=>setValue(row,field,paths.map(path=>`[[${path}]]`).join(', ')));};}
    function propertyDialog(existing){ctx.dialog(existing?'Editar propiedad':'Nueva propiedad',`<form id="dbPropertyForm"><label class="field-label" for="dbPropName">Nombre</label><input class="text-input" id="dbPropName" value="${escape(existing?.name||'')}" required><label class="field-label" for="dbPropType">Tipo</label><select class="text-input" id="dbPropType">${Object.entries(fieldTypes).map(([key,name])=>`<option value="${key}" ${existing?.type===key?'selected':''}>${name}</option>`).join('')}</select><div id="dbPropExtra"></div><p class="field-error" id="dbPropError"></p><div class="form-actions">${existing?'<button class="secondary-button" type="button" id="hideDbProp">Ocultar propiedad</button>':''}<button class="primary-button">Guardar propiedad</button></div></form>`);
      function extra(){const type=$('#dbPropType').value;$('#dbPropExtra').innerHTML=['select','multi'].includes(type)?`<label class="field-label" for="dbPropOptions">Opciones, separadas por comas</label><input id="dbPropOptions" class="text-input" value="${escape((existing?.options||['Por empezar','En curso','Completado']).join(', '))}">`:['formula','rollup'].includes(type)?`<label class="field-label" for="dbPropSource">Propiedad de origen</label><select id="dbPropSource" class="text-input">${config.fields.filter(f=>f.key!==existing?.key).map(f=>`<option value="${escape(f.key)}" ${existing?.source===f.key?'selected':''}>${escape(f.name)}</option>`).join('')}</select><label class="field-label" for="dbPropOperation">Operación</label><select id="dbPropOperation" class="text-input">${(type==='rollup'?[['sum','Suma de la propiedad relacionada'],['average','Promedio'],['count','Número de páginas relacionadas']]:[['sum','Origen + segunda propiedad'],['difference','Origen − segunda propiedad'],['multiply','Origen × segunda propiedad'],['relation_count','Número de relaciones'],['days_until','Días hasta la fecha']]).map(([key,label])=>`<option value="${key}" ${existing?.operation===key?'selected':''}>${label}</option>`).join('')}</select><label class="field-label" for="dbPropSecond">${type==='rollup'?'Clave numérica en las páginas relacionadas':'Segunda propiedad'}</label>${type==='rollup'?`<input class="text-input" id="dbPropSecond" value="${escape(existing?.target||'')}" placeholder="Ej.: horas">`:`<select class="text-input" id="dbPropSecond"><option value="">Ninguna</option>${config.fields.filter(f=>f.key!==existing?.key).map(f=>`<option value="${escape(f.key)}" ${existing?.second===f.key?'selected':''}>${escape(f.name)}</option>`).join('')}</select>`}`:'';}
      function computedSources(){const operation=$('#dbPropOperation'),source=$('#dbPropSource');if(!operation||!source)return;const type=$('#dbPropType').value,op=operation.value,required=type==='rollup'||op==='relation_count'?'relation':op==='days_until'?'date':'number',selected=source.value||existing?.source;const choices=config.fields.filter(field=>field.key!==existing?.key&&field.type===required);source.innerHTML=choices.length?choices.map(field=>`<option value="${escape(field.key)}" ${selected===field.key?'selected':''}>${escape(field.name)}</option>`).join(''):'<option value="">Añade primero una propiedad de tipo '+fieldTypes[required]+'</option>';operation.onchange=computedSources;if(type==='formula'){const second=$('#dbPropSecond'),chosen=second.value;second.innerHTML='<option value="">Ninguna</option>'+config.fields.filter(field=>field.key!==existing?.key&&field.type==='number').map(field=>`<option value="${escape(field.key)}" ${chosen===field.key?'selected':''}>${escape(field.name)}</option>`).join('');}}
      $('#dbPropType').onchange=()=>{extra();computedSources();};extra();computedSources();if(existing)$('#hideDbProp').onclick=()=>{config.fields=config.fields.filter(f=>f!==existing);saveConfig();ctx.closeOverlay();ctx.run(draw());};
      $('#dbPropertyForm').onsubmit=event=>{event.preventDefault();try{const name=$('#dbPropName').value.trim(),key=existing?.key||databaseKey(name);if(!name||!isPropertyKey(key))throw new Error('Elige un nombre válido.');if(!existing&&config.fields.some(f=>f.key===key))throw new Error('Ya existe una propiedad con ese nombre.');const field={key,name,type:$('#dbPropType').value};if(['select','multi'].includes(field.type))field.options=$('#dbPropOptions').value.split(',').map(s=>s.trim()).filter(Boolean);if(['formula','rollup'].includes(field.type)){field.source=$('#dbPropSource').value;if(!field.source)throw new Error('Añade primero una propiedad de origen del tipo adecuado.');field.operation=$('#dbPropOperation').value;field[field.type==='rollup'?'target':'second']=$('#dbPropSecond').value;}if(existing)config.fields.splice(config.fields.indexOf(existing),1,field);else config.fields.push(field);saveConfig();ctx.closeOverlay();ctx.run(draw());}catch(error){$('#dbPropError').textContent=error.message;}};
    }
    function filterDialog(){const current=view(),filters=current.filters||[];ctx.dialog('Filtros de «'+current.name+'»',`<form id="filterForm"><div class="n-filter-list">${filters.map((filter,index)=>`<div>${escape(filter.key)} ${escape(filter.op)} ${escape(filter.value||'')}<button type="button" data-remove-filter="${index}" aria-label="Quitar filtro">${I('X',13)}</button></div>`).join('')||'<p class="empty-hint">Esta vista muestra todas las páginas.</p>'}</div><label class="field-label" for="filterField">Propiedad</label><select class="text-input" id="filterField"><option value="title">Nombre</option>${config.fields.map(f=>`<option value="${escape(f.key)}">${escape(f.name)}</option>`).join('')}</select><label class="field-label" for="filterOp">Condición</label><select class="text-input" id="filterOp"><option value="contains">Contiene</option><option value="eq">Es igual a</option><option value="neq">No es igual a</option><option value="empty">Está vacío</option><option value="gte">Mayor o igual que</option><option value="lte">Menor o igual que</option></select><label class="field-label" for="filterValue">Valor</label><input class="text-input" id="filterValue"><p class="empty-hint">Todos los filtros deben cumplirse (Y). Cada vista guarda sus propios filtros.</p><div class="form-actions"><button class="primary-button">Añadir filtro</button></div></form>`);$$('[data-remove-filter]').forEach(el=>el.onclick=()=>{current.filters.splice(Number(el.dataset.removeFilter),1);saveConfig();ctx.closeOverlay();ctx.run(draw());});$('#filterForm').onsubmit=event=>{event.preventDefault();current.filters||=[];current.filters.push({key:$('#filterField').value,op:$('#filterOp').value,value:$('#filterValue').value});saveConfig();ctx.closeOverlay();ctx.run(draw());};}
    function sortDialog(){const current=view();ctx.dialog('Ordenar «'+current.name+'»',`<form id="sortForm"><label class="field-label" for="sortField">Propiedad</label><select class="text-input" id="sortField"><option value="">Sin ordenación</option><option value="title" ${current.sort?.key==='title'?'selected':''}>Nombre</option>${config.fields.map(f=>`<option value="${escape(f.key)}" ${current.sort?.key===f.key?'selected':''}>${escape(f.name)}</option>`).join('')}</select><label class="field-label" for="sortDirection">Dirección</label><select class="text-input" id="sortDirection"><option value="asc">Ascendente</option><option value="desc" ${current.sort?.direction==='desc'?'selected':''}>Descendente</option></select><div class="form-actions"><button class="primary-button">Aplicar</button></div></form>`);$('#sortForm').onsubmit=event=>{event.preventDefault();current.sort={key:$('#sortField').value,direction:$('#sortDirection').value};saveConfig();ctx.closeOverlay();ctx.run(draw());};}
    function addView(){ctx.dialog('Añadir vista',`<form id="addViewForm"><label class="field-label" for="viewName">Nombre de la vista</label><input class="text-input" id="viewName" required placeholder="Mi perspectiva"><label class="field-label" for="viewType">Diseño</label><select class="text-input" id="viewType"><option value="table">Tabla</option><option value="board">Tablero</option><option value="gallery">Galería</option><option value="calendar">Calendario</option><option value="list">Lista</option></select><div class="form-actions"><button class="primary-button">Crear vista</button></div></form>`);$('#addViewForm').onsubmit=event=>{event.preventDefault();const next={id:uuid(),name:$('#viewName').value,type:$('#viewType').value,filters:[],sort:null};config.views.push(next);config.activeView=next.id;saveConfig();ctx.closeOverlay();ctx.run(draw());};}
    function optionsDialog(){const current=view();ctx.dialog('Opciones de la vista',`<form id="viewOptions"><label class="field-label" for="viewRename">Nombre</label><input class="text-input" id="viewRename" value="${escape(current.name)}" required><label class="field-label" for="viewGroup">Agrupar tablero por</label><select class="text-input" id="viewGroup">${config.fields.filter(f=>f.type==='select').map(f=>`<option value="${escape(f.key)}" ${current.groupBy===f.key?'selected':''}>${escape(f.name)}</option>`).join('')}</select><label class="field-label" for="viewDate">Fecha del calendario</label><select class="text-input" id="viewDate">${config.fields.filter(f=>f.type==='date').map(f=>`<option value="${escape(f.key)}" ${current.dateBy===f.key?'selected':''}>${escape(f.name)}</option>`).join('')}</select><label class="n-check-label"><input type="checkbox" id="showArchived" ${config.showArchived?'checked':''}> Mostrar páginas archivadas</label><div class="form-actions">${config.views.length>1?'<button type="button" class="secondary-button" id="deleteView">Quitar vista</button>':''}<button class="primary-button">Guardar</button></div></form>`);$('#viewOptions').onsubmit=event=>{event.preventDefault();current.name=$('#viewRename').value;current.groupBy=$('#viewGroup').value;current.dateBy=$('#viewDate').value;config.showArchived=$('#showArchived').checked;saveConfig();ctx.closeOverlay();ctx.run(draw());};if($('#deleteView'))$('#deleteView').onclick=()=>{config.views=config.views.filter(v=>v!==current);config.activeView=config.views[0].id;saveConfig();ctx.closeOverlay();ctx.run(draw());};}
    function databaseMenu(event){ctx.menu(event,[{icon:'Pencil',label:'Nombre y descripción',action:()=>{ctx.dialog('Tu colección',`<form id="dbDetails"><label class="field-label" for="dbName">Nombre</label><input class="text-input" id="dbName" value="${escape(config.name)}" required><label class="field-label" for="dbDescription">Descripción</label><input class="text-input" id="dbDescription" value="${escape(config.description||'')}"><div class="form-actions"><button class="primary-button">Guardar</button></div></form>`);$('#dbDetails').onsubmit=event=>{event.preventDefault();config.name=$('#dbName').value;config.description=$('#dbDescription').value;saveConfig();ctx.closeOverlay();ctx.run(draw());};}},{icon:'Upload',label:'Importar CSV',action:importCsv},{icon:'Download',label:'Exportar vista como CSV',action:exportCsv},...(config.templates||[]).map(template=>({icon:'BookOpen',label:'Crear desde «'+template.name+'»',action:()=>newRecord({},template)}))]);}

    function importCsv(){
      const picker=document.createElement('input');picker.type='file';picker.accept='.csv,text/csv';
      picker.onchange=()=>invoke(async()=>{const file=picker.files?.[0];if(!file)return;const parsed=parseCsv(await file.text());if(parsed.length<2)throw new Error('El CSV necesita encabezados y al menos una fila.');const [headers,...data]=parsed;const mappings=headers.map((name,index)=>{if(index===0)return{title:true};const found=config.fields.find(field=>field.key===name||field.name.toLocaleLowerCase()===name.trim().toLocaleLowerCase());if(found)return found;const key=databaseKey(name);if(!isPropertyKey(key))return null;return{key,name:name.trim(),type:'text',fresh:true};});
      const assigned=mappings.filter(field=>field&&!field.title).map(field=>field.key);if(mappings.some(field=>!field)||data.some(row=>row.length>headers.length))throw new Error('Revisa los encabezados y el número de columnas del CSV.');if(new Set(assigned).size!==assigned.length)throw new Error('El CSV contiene encabezados repetidos o que se asignan a la misma propiedad.');
      ctx.dialog('Importar en '+config.name,`<p class="n-modal-intro">${data.length} páginas para tu colección.</p><p class="empty-hint">La primera columna será el título. Las columnas nuevas se añadirán como propiedades de texto. Los archivos existentes se conservarán.</p><div class="n-import-preview">${data.slice(0,5).map(row=>'<div>'+escape(row[0]||'Sin título')+'</div>').join('')}</div><div class="form-actions"><button class="secondary-button" data-action="close-overlay">Cancelar</button><button class="primary-button" id="confirmCsvImport">Importar páginas</button></div>`);
      $('#confirmCsvImport').onclick=()=>invoke(async()=>{const submit=$('#confirmCsvImport');submit.disabled=true;submit.textContent='Importando…';try{const newKeys=new Set(config.fields.map(field=>field.key));for(const field of mappings.filter(Boolean)){if(field.fresh&&!newKeys.has(field.key)){config.fields.push({key:field.key,name:field.name,type:'text'});newKeys.add(field.key);}}saveConfig();await ctx.saveDocument(doc);let count=0;for(const row of data){const rawName=String(row.find((value,index)=>mappings[index]?.title)||'Sin título').trim();const name=rawName.replace(/[<>:"\\|?*\x00-\x1f/]/g,' ').trim().replace(/^\.+/,'')||'Sin título';const folder=config.folder||doc.path.replace(/\.base$/,'');let path=folder+'/'+name+'.md',index=2;while(ctx.state.files.some(file=>file.path===path))path=folder+'/'+name+' '+index+++'.md';if(!ctx.validPath(path))continue;const meta={icon:'FileText',editor:'blocks',_database:doc.path};row.forEach((value,column)=>{const field=mappings[column];if(!field||field.title||['formula','rollup'].includes(field.type))return;meta[field.key]=field.type==='number'&&value!==''&&Number.isFinite(Number(value))?Number(value):field.type==='checkbox'?['true','yes','sí','si','1','✓'].includes(value.toLocaleLowerCase()):field.type==='multi'?value.split(',').map(s=>s.trim()).filter(Boolean):value;});await postFile(path,writePage(meta,''));ctx.state.files.push({path,name:path.split('/').pop(),is_dir:false});count++;}await ctx.refreshFiles();ctx.closeOverlay();await draw();ctx.notify(count+' páginas importadas.');}catch(error){ctx.closeOverlay();await ctx.refreshFiles();await draw();throw error;}});
      });picker.click();
    }
    function exportCsv(){const fields=[{key:'title',name:'Nombre',type:'title'},...config.fields],visible=queryRows(rows,config,all),quote=value=>'"'+String(value??'').replaceAll('"','""')+'"';const csv='\uFEFF'+[fields.map(f=>quote(f.name)).join(','),...visible.map(row=>fields.map(f=>quote(fieldValue(row,f,all))).join(','))].join('\r\n');const url=URL.createObjectURL(new Blob([csv],{type:'text/csv;charset=utf-8'})),link=document.createElement('a');link.href=url;link.download=config.name+'.csv';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);}
    const onChange=event=>{if(!alive||event.detail.origin===tab.id)return;if(event.detail.path===doc.path){try{config=JSON.parse(doc.content);}catch{return;}}else if(!event.detail.path.endsWith('.md'))return;clearTimeout(timer);timer=setTimeout(()=>ctx.run(draw()),200);};document.addEventListener('sc-document-changed',onChange);ctx.run(draw());
    return{destroy(){alive=false;clearTimeout(timer);document.removeEventListener('sc-document-changed',onChange);},refresh(){try{config=JSON.parse(doc.content);ctx.run(draw());}catch(error){ctx.report(error);}}};
  }
  return { mountHome, mountCalendar, scheduleNote, dueReminders,mountBlocks,mountDatabase,mountPageTree,newPage,createDatabase,templates,appearance,allPages,movePage };
}
