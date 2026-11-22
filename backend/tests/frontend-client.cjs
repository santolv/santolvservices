// Non-visual contract checks: node tests/frontend-client.cjs (no browser or screenshots).
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const path = require('node:path');
const html = fs.readFileSync(path.join(__dirname, '../../frontend/pruebahtml/index.html'), 'utf8');
const script = html.match(/<script>([\s\S]*?)<\/script>/)[1].replace(/\nconnectAPI\(\);\s*$/, '');
const elements = new Map();
function element(id) {
  const classes=new Set();
  if (!elements.has(id)) elements.set(id, { hidden:false, disabled:false, value:'', textContent:'', innerHTML:'', style:{}, dataset:{}, open:false,
    offsetWidth:250,offsetHeight:240,focus(){},getBoundingClientRect(){return {left:20,bottom:100}},
    listeners:{}, attributes:{}, classList:{toggle(c,value){if(value??!classes.has(c))classes.add(c);else classes.delete(c)},add(c){classes.add(c)},remove(c){classes.delete(c)},contains(c){return classes.has(c)}},
    addEventListener(type,handler){this.listeners[type]=handler}, querySelectorAll(){return []},querySelector(selector){return selector==='[type=submit]'?element('modal-submit'):null},
    setAttribute(name,value){this.attributes[name]=String(value)},removeAttribute(name){delete this.attributes[name]},
    showModal(){this.open=true},close(){this.open=false} });
  return elements.get(id);
}
const requests = [];
const context = vm.createContext({ console, Headers, Blob, ArrayBuffer, Uint8Array, AbortSignal, URL, DOMException, FormData:class extends Map{constructor(){super()}},crypto:require('node:crypto').webcrypto,
  setTimeout:()=>0, clearTimeout(){}, location:{protocol:'http:'},innerWidth:800,innerHeight:600,
  document:{body:element('body'),getElementById:element, addEventListener(){}, querySelectorAll(){return []}, querySelector(){return null}},
  window:{addEventListener(){}}, sessionStorage:{getItem(){return null},setItem(){},removeItem(){}},
  fetch:async (url, options={}) => { requests.push({url, options}); return {ok:true,status:200,json:async()=>({items:[],next_cursor:null})}; }
});
vm.runInContext(script, context);
(async()=>{
  await vm.runInContext("api('/me')", context);
  assert.equal(requests[0].url,'http://localhost:8443/api/v1/me');
  assert.equal(requests[0].options.credentials,'include');
  vm.runInContext("state.csrf='test-csrf'", context);
  await vm.runInContext("api('/spaces',{method:'POST',body:{name:'Example'}})", context);
  assert.equal(requests[1].options.headers.get('x-csrf-token'),'test-csrf');
  assert.equal(JSON.parse(requests[1].options.body).name,'Example');
  assert.equal(requests[1].url,'http://localhost:8443/api/v1/spaces');
  context.fetch = async()=>{throw new Error('API unavailable')};
  await vm.runInContext('connectAPI()', context);
  assert.equal(element('login-submit').disabled,false);
  assert.equal(element('api-status').textContent,'Sin conexión');
  assert(element('connection-help').innerHTML.includes('Reintentar'));
  context.fetch=async url=>url.endsWith('/health/live')?{ok:true}:{ok:false,status:401,json:async()=>({error:{code:'authentication_required'}})};
  await vm.runInContext('connectAPI()', context);
  assert.equal(element('login-submit').disabled,false);
  assert.equal(element('api-status').textContent,'API conectada');
  element('username').value='sample-user';
  element('password').value='sample-password';
  let loginRequest;
  context.fetch=async(url,options)=>{loginRequest={url,options};return {ok:false,status:401,json:async()=>({error:{code:'invalid_credentials'}})}};
  await element('login-form').listeners.submit({preventDefault(){}});
  assert.equal(loginRequest.url,'http://localhost:8443/api/v1/auth/login');
  assert.equal(loginRequest.options.method,'POST');
  assert.equal(loginRequest.options.credentials,'include');
  assert.deepEqual(JSON.parse(loginRequest.options.body),{username:'sample-user',password:'sample-password'});
  assert.equal(element('login-submit').disabled,false);
  const fileRequests=[];
  const fileContext=vm.createContext({...context,location:{protocol:'file:'},fetch:async(url,options)=>{fileRequests.push({url,options});return {ok:false,status:401,json:async()=>({error:{code:'invalid_credentials'}})}}});
  vm.runInContext(script,fileContext);
  await vm.runInContext('connectAPI()',fileContext);
  assert.equal(fileRequests[0].url,'http://localhost:8443/health/live');
  assert.equal(fileRequests[0].options.credentials,'omit');
  await element('login-form').listeners.submit({preventDefault(){}});
  const fileLogin=fileRequests.at(-1);
  assert.equal(fileLogin.url,'http://localhost:8443/api/v1/auth/login');
  assert.equal(fileLogin.options.credentials,'omit');
  assert.equal(JSON.parse(fileLogin.options.body).local_file,true);
  vm.runInContext("state.token='temporary-token'",fileContext);
  await vm.runInContext("api('/spaces',{method:'POST',body:{name:'Local'}}).catch(()=>{})",fileContext);
  assert.equal(fileRequests.at(-1).options.headers.get('authorization'),'Bearer temporary-token');
  assert.equal(fileRequests.at(-1).options.headers.get('x-csrf-token'),null);
  await vm.runInContext("downloadFile('http://localhost:8443/api/v1/nodes/example/content').catch(()=>{})",fileContext);
  assert.equal(fileRequests.at(-1).options.headers.Authorization,'Bearer temporary-token');
  assert.equal(fileRequests.at(-1).options.credentials,'omit');
  vm.runInContext('enter=async()=>{}',fileContext);
  fileContext.fetch=async()=>({ok:true,json:async()=>({token:'new-session-token'})});
  await element('login-form').listeners.submit({preventDefault(){}});
  assert.equal(vm.runInContext('state.token',fileContext),'new-session-token');
  vm.runInContext('signedOut()',fileContext);
  assert.equal(vm.runInContext('state.token',fileContext),'');
  assert(!html.includes('location.replace'));
  assert(!html.includes('/admin/'));
  assert(!html.includes('data-nav="users"'));
  assert(!html.includes('data-user='));
  context.fetch=async()=>({ok:true,status:200,json:async()=>({account_type:'admin',username:'server-admin'})});
  await assert.rejects(vm.runInContext('enter()',context),/únicamente desde el servidor/);
  vm.runInContext("state.rows=[{id:'2',name:'Zeta',kind:'file',revision:1},{id:'1',name:'Alpha',kind:'folder',revision:1}];state.cursor=null",context);
  element('type-filter').value='folder';element('sort-order').value='asc';element('search').value='';
  vm.runInContext('render()',context);
  assert(element('items').innerHTML.includes('Alpha'));
  assert(!element('items').innerHTML.includes('Zeta'));
  element('type-filter').value='all';
  vm.runInContext("state.rows=[{id:'file-1',name:'Budget <2027>.pdf',kind:'file',revision:2}];state.grid=true;state.selection='file-1';render()",context);
  assert(element('items').innerHTML.includes('files grid'));
  assert(element('items').innerHTML.includes('data-t="PDF"'));
  assert(element('items').innerHTML.includes('Budget &lt;2027&gt;.pdf'));
  assert.equal(element('selbar').hidden,false);
  vm.runInContext('state.grid=false;render()',context);
  assert(element('items').innerHTML.includes('files list'));
  vm.runInContext('state.selected=state.rows[0];detail()',context);
  assert.equal(element('detail').open,true);
  assert(element('detail').innerHTML.includes('http://localhost:8443/api/v1/nodes/file-1/content'));
  assert(element('detail').innerHTML.includes('Budget &lt;2027&gt;.pdf'));
  assert(!element('detail').innerHTML.includes('Historial de versiones'));
  vm.runInContext("actions['new-menu']($('newBtn'))",context);
  assert.equal(element('new-menu').classList.contains('show'),true);
  assert(html.includes('right:auto;bottom:auto;width:250px'));
  vm.runInContext("state.rows=[{id:'folder-1',name:'Trabajo',kind:'folder',revision:1}];nodeMenu({preventDefault(){},clientX:790,clientY:590},{dataset:{node:'folder-1'},getBoundingClientRect(){return {left:20,bottom:100}}})",context);
  assert.equal(element('new-menu').classList.contains('show'),false);
  assert.equal(element('node-menu').classList.contains('show'),true);
  for(const action of ['inspect','rename','move','share','trash'])assert(element('node-menu').innerHTML.includes('data-action="'+action+'"'));
  assert.equal(element('node-menu').style.left,'538px');assert.equal(element('node-menu').style.top,'348px');
  vm.runInContext("state.rows[0].trashed_at='2026-11-22';nodeMenu({preventDefault(){}},{dataset:{node:'folder-1'},getBoundingClientRect(){return {left:20,bottom:100}}})",context);
  assert(element('node-menu').innerHTML.includes('data-action="restore"'));
  assert(element('node-menu').innerHTML.includes('data-action="purge"'));
  assert(!element('node-menu').innerHTML.includes('data-action="rename"'));
  vm.runInContext('closeMenus()',context);assert.equal(element('node-menu').classList.contains('show'),false);
  const moveRequests=[];
  context.fetch=async(url,options={})=>{moveRequests.push({url,options});return {ok:true,status:200,json:async()=>({items:[{id:'destination',name:'Destino',kind:'folder'}],next_cursor:null})}};
  vm.runInContext("state.space='space';state.selected={id:'folder-1',space_id:'space',kind:'folder',name:'Trabajo',revision:1}",context);
  await vm.runInContext('moveDialog()',context);
  assert(moveRequests[0].url.startsWith('http://localhost:8443/api/v1/spaces/space/nodes?'));
  assert(element('dialog-body').innerHTML.includes('data-destination="destination"'));
  assert(!element('dialog-body').innerHTML.includes('Identificador'));
  vm.runInContext('signedOut()',context);
  assert.equal(element('detail').open,false);
  const deletionRequests=[];
  fileContext.fetch=async(url,options={})=>{deletionRequests.push({url,options});return url.endsWith('/purge')?{ok:true,status:202,headers:new Headers(),json:async()=>{throw Error('Empty 202 must not be decoded')}}:{ok:true,status:200,json:async()=>({items:[],next_cursor:null})}};
  vm.runInContext("state.me={id:'user',account_type:'user'};state.token='delete-session';state.space='space';state.selected={id:'trashed-file',name:'file.txt',revision:3,trashed_at:'2026-11-22'};actions.purge()",fileContext);
  await element('modal-form').listeners.submit({preventDefault(){},currentTarget:element('modal-form')});
  const purge=deletionRequests.find(r=>r.url.endsWith('/purge'));
  assert(purge);assert.equal(purge.options.method,'POST');
  assert.equal(purge.options.headers.get('if-match'),'"3"');
  assert.equal(element('modal-error').textContent,'');
  assert.equal(element('login-submit').disabled,false);
  assert(!html.includes("credentials:'same-origin'"));
  assert(!html.includes('href="/api/'));
  assert(html.includes("fetch(apiUrl('/api/v1/auth/login')"));
  console.log('PASS independent API URL, credentials, CSRF, downloads, offline state and reconnection (no browser).');
})().catch(error=>{console.error(error);process.exitCode=1});
