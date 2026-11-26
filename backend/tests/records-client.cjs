// Run the actual shared client with each service page's identity.
const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict'),path=require('node:path');
const script=fs.readFileSync(path.join(__dirname,'../../frontend/shared/records.js'),'utf8');
async function check(service){
 const html=fs.readFileSync(path.join(__dirname,`../../frontend/${service}/index.html`),'utf8');
 assert(html.includes(`data-service="${service}"`));
 const elements=new Map(),requests=[];
 function element(id){if(['notes','tasks'].includes(id))throw Error('Removed combined-service tab used');if(!elements.has(id))elements.set(id,{hidden:false,disabled:false,value:'',checked:false,textContent:'',children:[],dataset:{},elements:{},addEventListener(){},setAttribute(){},replaceChildren(){},append(){},reset(){},focus(){}});return elements.get(id)}
 for(const key of ['q','archived','status','priority'])element('filters').elements[key]={value:key==='archived'?'false':''};
 element('edit-form').elements.note_id={value:'',options:[]};
 const context=vm.createContext({console,URLSearchParams,AbortController,Intl,Date,encodeURIComponent,location:{protocol:'http:',search:'',replace(){throw Error('Unexpected redirect')}},window:{SantolvShell:{account(){},session(){}}},document:{body:{dataset:{service}},getElementById:element,querySelectorAll(){return[]}},fetch:async(url)=>{requests.push(url);return {ok:true,status:200,json:async()=>url.endsWith('/me')?{csrf_token:'test',expires_at:'2099-01-01T00:00:00Z'}:{items:[],next_cursor:null}}}});
 vm.runInContext(script,context);await new Promise(resolve=>setImmediate(resolve));
 assert(requests.some(url=>url.includes('/api/v1/'+service+'?')));
 assert(!requests.some(url=>url.includes('notes-tasks')));
 assert.equal(element('new').textContent,service==='notes'?'Nueva nota':'Nueva tarea');
 assert.equal(element('archived-filter').hidden,service==='tasks');
 assert.equal(element('message').textContent,'No hay resultados.');
}
(async()=>{await check('notes');await check('tasks');console.log('PASS Notas y Tareas cargan su API independiente y sus filtros propios.');})().catch(error=>{console.error(error);process.exitCode=1});
