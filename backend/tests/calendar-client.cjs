// Exercise the production overlap algorithm with touching, nested and chained events.
const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict'),path=require('node:path');
const source=fs.readFileSync(path.join(__dirname,'../../frontend/calendar/index.html'),'utf8');
const begin=source.indexOf('  function layoutIntervals('),end=source.indexOf('  function renderWeek(',begin);
const context=vm.createContext({});vm.runInContext(source.slice(begin,end)+';this.layout=layoutIntervals;',context);
const input=[{id:1,start:540,end:600},{id:2,start:570,end:630},{id:3,start:630,end:660}];
const out=context.layout(input);assert.equal(out[0].columns,2);assert.equal(out[1].columns,2);assert.notEqual(out[0].lane,out[1].lane);assert.equal(out[2].columns,1);assert(!('lane'in input[0]));
const nested=context.layout([{start:540,end:720},{start:550,end:580},{start:560,end:570},{start:600,end:640}]);assert(nested.every(x=>x.columns===3));assert.equal(nested[3].lane,1);
console.log('PASS eventos solapados, límites adyacentes, grupos y carriles sin modificar los datos.');
