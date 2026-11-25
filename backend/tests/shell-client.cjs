const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict');
const script=fs.readFileSync(path.join(__dirname,'../../frontend/shared/shell.js'),'utf8');
const values=new Map(),pages=[];
function page(withSidebar=false){
 const events={},buttonEvents={},attributes={};
 const button={setAttribute(k,v){attributes[k]=v},addEventListener(k,f){buttonEvents[k]=f}};
 const select={value:''},media={matches:false,addEventListener(k,f){this.change=f}};
 const layout={matches:false},classes=new Set(),sidebar={};
 let focused;
 const app={classList:{remove(name){classes.delete(name)},toggle(name,force){if(force)classes.add(name);else classes.delete(name)}},querySelector(selector){return Object.values(sidebar).find(control=>selector.includes(`"${control.dataset.shellSidebar}"`))}};
 for(const action of ['hide','show'])sidebar[action]={dataset:{shellSidebar:action},closest(){return app},addEventListener(_,listener){this.click=listener},focus(){focused=action}};
 const document={documentElement:{dataset:{}},getElementById(){return null},addEventListener(k,f){events[k]=f},querySelectorAll(selector){return selector==='[data-shell-theme]'?[button]:selector==='[data-shell-theme-select]'?[select]:selector==='[data-shell-sidebar]'&&withSidebar?Object.values(sidebar):[]}};
 const window={matchMedia(query){return query.includes('max-width')?layout:media},addEventListener(k,f){events[k]=f}};
 const localStorage={getItem(k){return values.get(k)||null},setItem(k,v){values.set(k,v);for(const other of pages)if(other.window!==window)other.events.storage?.({key:k})}};
 const context=vm.createContext({document,window,localStorage,location:{protocol:'http:'},requestAnimationFrame:callback=>callback()});vm.runInContext(script,context);
 const result={window,document,events,buttonEvents,attributes,select,media,layout,classes,sidebar,get focused(){return focused}};pages.push(result);events.DOMContentLoaded();return result;
}
const home=page(),drive=page(true),photos=page(true);
const themes=()=>pages.map(p=>p.document.documentElement.dataset.theme);
assert.deepEqual(themes(),['light','light','light']);
home.buttonEvents.click();assert.deepEqual(themes(),['dark','dark','dark']);assert.equal(values.get('santolv.theme'),'dark');
assert.equal(drive.attributes['aria-pressed'],'true');assert.equal(photos.select.value,'dark');
drive.buttonEvents.click();assert.deepEqual(themes(),['light','light','light']);
photos.window.SantolvShell.setTheme('system');assert.equal(home.select.value,'system');
for(const p of pages){p.media.matches=true;p.media.change()}
assert.deepEqual(themes(),['dark','dark','dark']);
values.set('santolv.theme','light');home.events.pageshow();assert.equal(home.document.documentElement.dataset.theme,'light');
photos.window.SantolvShell.setTheme('invalid');assert.deepEqual(themes(),['light','light','light']);
for(const app of [drive,photos]){
 app.sidebar.hide.click();assert(app.classes.has('sidebar-hidden'));assert.equal(app.focused,'show');
 app.sidebar.show.click();assert(!app.classes.has('sidebar-hidden'));assert.equal(app.focused,'hide');
 app.layout.matches=true;
 app.sidebar.show.click();assert(app.classes.has('mobile-sidebar'));assert.equal(app.focused,'hide');
 app.sidebar.hide.click();assert(!app.classes.has('mobile-sidebar'));assert.equal(app.focused,'show');
}
for(const file of ['frontend/home/index.html','frontend/drive/index.html','frontend/photos/index.html','frontend/account/index.html']){
 const html=fs.readFileSync(path.join(__dirname,'../..',file),'utf8');
 assert(html.includes('<!-- SANTOLV_SHELL -->'),file);
 if(file.includes('/drive/')||file.includes('/photos/'))for(const action of ['HIDE','SHOW'])assert(html.includes(`<!-- SANTOLV_SIDEBAR_${action} -->`),file);
}
const avatar={textContent:'',dataset:{}};
home.window.SantolvShell.avatar(avatar,{username:'prueba',display_name:'Ana Gómez',avatar_color:'plum'});
assert.equal(avatar.textContent,'AG');
assert.equal(avatar.dataset.avatarColor,'plum');
console.log('PASS tema global, pestañas, sistema y panel compartido en escritorio y móvil.');
