// Focused HTTP/state contracts, without a browser or dependencies.
const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict'),path=require('node:path');
const html=fs.readFileSync(path.join(__dirname,'../../santolvphotos/frontend/index.html'),'utf8');
for(const match of html.matchAll(/<script>([\s\S]*?)<\/script>/g))new vm.Script(match[1]);
const client=html.slice(html.indexOf('const API_ORIGIN=')).split('</script>')[0].replace(/route\(\);\s*$/,'');
const requests=[],photos=[{id:'photo',revision:4,favorite:false}],elements=new Map();
const $=id=>{if(!elements.has(id))elements.set(id,{hidden:true,value:'',addEventListener(){}});return elements.get(id)};
let reply={ok:true,status:200,data:{revision:5,favorite:true}};
const context=vm.createContext({Headers,Blob,ArrayBuffer,Uint8Array,URL,crypto:require('node:crypto').webcrypto,console,setTimeout,photos,albums:[],
 state:{route:'photos',selected:new Set()},$,render(){},updateViewer(){},notify(){},route(){},
 location:{protocol:'http:',hash:'',assign(){}},window:{addEventListener(){}},
 IntersectionObserver:class{observe(){}unobserve(){}disconnect(){}},
 fetch:async(url,options)=>{requests.push({url,options});return {...reply,json:async()=>reply.data}}
});
vm.runInContext(client,context);
(async()=>{
 vm.runInContext("csrf='session-csrf'",context);
 await vm.runInContext("persistPhoto(photos[0],'favorite',true)",context);
 assert.equal(requests[0].url,'http://localhost:8443/api/v1/photos/photo/favorite');
 assert.equal(requests[0].options.credentials,'include');
 assert.equal(requests[0].options.headers.get('x-csrf-token'),'session-csrf');
 assert.equal(requests[0].options.headers.get('if-match'),'"4"');
 assert.equal(photos[0].favorite,true);assert.equal(photos[0].revision,5);
 // A rejected write cannot turn a confirmed favorite off.
 reply={ok:false,status:412,data:{error:{code:'revision_changed'}}};
 vm.runInContext('loadLibrary=async()=>{}',context);
 await vm.runInContext("persistPhoto(photos[0],'favorite',false)",context);
 assert.equal(photos[0].favorite,true);assert.equal(photos[0].revision,5);
 reply={ok:true,status:200,data:{}};
 await vm.runInContext("photoApi('/uploads/test/content',{method:'PATCH',body:new ArrayBuffer(3)})",context);
 assert(requests.at(-1).options.body instanceof ArrayBuffer);
 assert.equal(requests.at(-1).options.headers.has('authorization'),false);
 assert(!html.includes("const photos = [\n      {"));
 assert(html.includes("'/spaces/'+photoSpace+'/nodes'"));
 console.log('PASS Photos credentials, CSRF, revisions, binary body and confirmed state.');
})().catch(error=>{console.error(error);process.exitCode=1});
