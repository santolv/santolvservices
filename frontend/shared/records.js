// Shared transport and session only. Each application owns its controller and form.
(() => {
  const origin=location.protocol==='https:'?location.origin:'http://localhost:8443';
  const errors={invalid_title:'Introduce un título de 1 a 200 caracteres.',invalid_content:'El contenido admite hasta 50 000 caracteres.',invalid_search:'La búsqueda admite hasta 200 caracteres.',invalid_due_at:'Introduce una fecha válida.',not_found:'El registro ya no está disponible.',service_disabled:'El servicio está desactivado.',service_forbidden:'No tienes permiso para acceder a este servicio.'};
  window.SantolvRecords={create(){
    let me;
    const home=()=>{document.body.hidden=true;location.replace('/')};
    async function api(path,{method='GET',body,signal}={}){
      const headers={};if(method!=='GET')headers['X-CSRF-Token']=me?.csrf_token;
      if(body!==undefined)headers['Content-Type']='application/json';
      const response=await fetch(origin+'/api/v1'+path,{method,headers,credentials:'include',signal,body:body===undefined?undefined:JSON.stringify(body)});
      if(response.ok)return response.status===204?null:response.json();
      if(response.status===401)home();
      const data=await response.json().catch(()=>null);throw new Error(errors[data?.error?.code]||'No se pudo completar la operación.');
    }
    async function logout(){await api('/auth/logout',{method:'POST'});window.SantolvShell?.sessionEnded();home()}
    async function start(){me=await api('/me');window.SantolvShell?.account(me,{logout,switch:logout});window.SantolvShell?.session(me.expires_at);return me}
    return {api,start};
  }};
})();
