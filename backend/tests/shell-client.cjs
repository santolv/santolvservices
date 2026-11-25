const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict');
const script=fs.readFileSync(path.join(__dirname,'../../frontend/shell.js'),'utf8');
const values=new Map(),pages=[];
function page(){
 const events={},buttonEvents={},attributes={};
 const button={setAttribute(k,v){attributes[k]=v},addEventListener(k,f){buttonEvents[k]=f}};
 const select={value:''},media={matches:false,addEventListener(k,f){this.change=f}};
 const document={documentElement:{dataset:{}},addEventListener(k,f){events[k]=f},querySelectorAll(selector){return selector==='[data-shell-theme]'?[button]:selector==='[data-shell-theme-select]'?[select]:[]}};
 const window={matchMedia(){return media},addEventListener(k,f){events[k]=f}};
 const localStorage={getItem(k){return values.get(k)||null},setItem(k,v){values.set(k,v);for(const other of pages)if(other.window!==window)other.events.storage?.({key:k})}};
 const context=vm.createContext({document,window,localStorage});vm.runInContext(script,context);
 const result={window,document,events,buttonEvents,attributes,select,media};pages.push(result);events.DOMContentLoaded();return result;
}
const home=page(),drive=page(),photos=page();
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
for(const file of ['frontend/index.html','santolvdrive/frontend/pruebahtml/index.html','santolvphotos/frontend/index.html']){
 const html=fs.readFileSync(path.join(__dirname,'../..',file),'utf8');
 assert(html.includes('data-shell-theme'),file);assert(html.includes('aria-label="Aplicaciones, volver a Inicio"'),file);
}
console.log('PASS tema global, pestañas, sistema, historial y controles en los tres servicios.');
