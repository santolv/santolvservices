const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict'),path=require('node:path');
const html=fs.readFileSync(path.join(__dirname,'../../frontend/home/index.html'),'utf8');
assert(!/<aside|id="navigation"|id="sidebar"/.test(html));
const elements=new Map();
function element(){return {hidden:false,_text:'',children:[],value:'',get textContent(){return this._text+this.children.map(c=>c.textContent).join(' ')},set textContent(value){this._text=value;this.children=[]},addEventListener(){},setAttribute(){},replaceChildren(...children){this.children=children},append(...children){this.children.push(...children)}}}
const context=vm.createContext({console,Date,encodeURIComponent,location:{protocol:'http:'},window:{addEventListener(){}},document:{getElementById(id){if(!elements.has(id))elements.set(id,element());return elements.get(id)},createElement:element,querySelectorAll(){return elements.get('recent-list')?.children||[]}},fetch:async()=>({ok:true,status:200,json:async()=>({items:[{id:'a',name:'<Foto privada>',service:'photos',kind:'file',last_used_at:1700000000000000},{id:'b',name:'Trabajo',service:'drive',kind:'folder',last_used_at:1690000000000000}]})})});
vm.runInContext(html.match(/<script>([\s\S]*?)<\/script>/)[1],context);
(async()=>{
 await vm.runInContext('loadRecent(enterRevision)',context);
 const rows=elements.get('recent-list').children;
 assert.equal(rows[0].children[0].children[0].href,'/photos/?photo=a');assert.equal(rows[1].children[0].children[0].href,'/drive/?node=b');
 assert.equal(rows[0].children[0].children[0].children[1].textContent,'<Foto privada>');
 assert.equal(rows[0].children[1].textContent,'Fotos');assert.equal(rows[1].children[1].textContent,'Drive');
 elements.get('home-search').value='drive';vm.runInContext('filterHome()',context);assert.equal(rows[0].hidden,true);assert.equal(rows[1].hidden,false);
 vm.runInContext('signedOut()',context);assert.equal(elements.get('recent').hidden,true);assert.equal(elements.get('recent-list').children.length,0);
 console.log('PASS Inicio independiente, actividad por servicio, enlaces y limpieza al salir.');
})().catch(error=>{console.error(error);process.exitCode=1});
