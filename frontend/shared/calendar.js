(() => {
  'use strict';
  const API_ORIGIN=location.protocol==='https:'?location.origin:'http://localhost:8443';
  const $ = id => document.getElementById(id);
  const zone = Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
  const localTime = date => {
    const shifted = new Date(date.getTime() - date.getTimezoneOffset() * 60000);
    return shifted.toISOString().slice(0, 16);
  };
  let profile, generation = 0, refreshNumber = 0, active = false, timer;

  async function request(path, method = 'GET', body) {
    const headers = {};
    if (method !== 'GET' && profile?.csrf_token) headers['X-CSRF-Token'] = profile.csrf_token;
    if (body !== undefined) headers['Content-Type'] = 'application/json';
    const response = await fetch(API_ORIGIN + '/api/v1' + path, {
      method, headers, credentials: 'include', body: JSON.stringify(body)
    });
    if (response.status === 401) {
      session(null);
      document.body.hidden = true;
      location.replace('/');
    }
    if (!response.ok) {
      const problem = await response.json().catch(() => ({}));
      throw Object.assign(new Error(problem.error?.code || 'request_failed'), {status: response.status});
    }
    return response.status === 204 ? null : response.json();
  }

  const api = (path, method, body) => request('/calendar' + path, method, body);
  function occurrenceUrl(item) {
    const params = new URLSearchParams({event: item.event_id, original: item.original_start});
    return '/calendar/?' + params;
  }
  function eventLabel(item) {
    return item.all_day
      ? new Date(item.local_start.slice(0, 10) + 'T12:00:00').toLocaleDateString('es-ES', {dateStyle: 'medium'}) + ' · Todo el día'
      : new Date(item.starts_at).toLocaleString('es-ES', {dateStyle: 'medium', timeStyle: 'short'});
  }
  function describe(error) {
    return ({
      invalid_event_text: 'Revisa el título y la descripción.',
      invalid_time_zone: 'Introduce una zona horaria válida, como Europe/Madrid.',
      invalid_zoned_time: 'Esa hora no existe en la zona seleccionada. Elige otra hora.',
      invalid_event_duration: 'El fin debe ser posterior al inicio. El evento puede durar hasta 31 días.',
      invalid_repeat_rule: 'Revisa la fecha o el número de repeticiones.',
      invalid_calendar_name: 'Introduce un nombre de calendario válido.',
      event_changed: 'El evento ha cambiado. Vuelve a abrirlo antes de editar.',
      not_found: 'Este recurso no está disponible o no tienes acceso.',
      service_disabled: 'Calendario está desactivado.',
      service_forbidden: 'Tu cuenta no tiene acceso a Calendario.'
    })[error.message] || 'No se pudo completar la operación.';
  }

  function renderList(list, items, reminders = false) {
    if (!list) return;
    const rows = items.map(item => {
      const row = document.createElement('li');
      const a = document.createElement('a');
      a.href = occurrenceUrl(item);
      a.textContent = item.title;
      row.append(a);
      if (reminders) {
        const button = document.createElement('button');
        button.type = 'button';
        button.textContent = 'Descartar';
        button.setAttribute('aria-label', 'Descartar recordatorio de ' + item.title);
        button.addEventListener('click', async () => {
          button.disabled = true;
          try {
            await api('/reminders', 'POST', {
              event_id: item.event_id, original_start: item.original_start, due_at: item.due_at
            });
            await refresh();
          } catch (error) {
            button.disabled = false;
            $('shell-calendar-status').textContent = describe(error);
          }
        });
        row.append(button);
      } else {
        const details = document.createElement('small');
        details.textContent = eventLabel(item) + ' · ' + item.calendar_name;
        row.append(details);
      }
      return row;
    });
    if (!rows.length) {
      const empty = document.createElement('li');
      empty.textContent = reminders ? 'Sin recordatorios pendientes.' : 'Sin eventos en los próximos siete días.';
      rows.push(empty);
    }
    list.replaceChildren(...rows);
  }

  function clear() {
    active = false;
    const trigger = $('shell-calendar-trigger');
    if (!trigger) return;
    trigger.hidden = true;
    $('shell-calendar').hidePopover?.();
    $('shell-calendar-dialog').close();
    $('shell-calendar-form').reset();
    $('shell-calendar-form').elements.calendar_id.replaceChildren();
    $('shell-calendar-count').hidden = true;
    $('shell-calendar-count').textContent = '';
    $('shell-calendar-status').textContent = '';
    $('shell-calendar-form-status').textContent = '';
    for (const id of ['shell-calendar-agenda', 'shell-calendar-reminders', 'home-calendar-agenda']) $(id)?.replaceChildren();
    if ($('home-calendar')) $('home-calendar').hidden = true;
    const card = $('home-calendar-card');
    if (card) {
      card.hidden = true;
      card.inert = true;
    }
  }

  async function refresh() {
    if (!profile || !$('shell-calendar-trigger') || document.hidden) return;
    const revision = generation;
    const refreshId = ++refreshNumber;
    const current = () => generation === revision && refreshNumber === refreshId;
    try {
      const {items} = await request('/services');
      if (!current()) return;
      if (!items.some(item => item.id === 'calendar' && item.enabled)) {
        clear();
        return;
      }
      active = true;
      $('shell-calendar-trigger').hidden = false;
      const home = $('home-calendar');
      if (home) {
        home.hidden = false;
        $('home-calendar-card').inert = false;
        const search = $('home-search').value.trim().toLocaleLowerCase('es');
        $('home-calendar-card').hidden = !$('home-calendar-card').textContent.toLocaleLowerCase('es').includes(search);
      }
      const now = new Date();
      const until = new Date(now);
      until.setDate(now.getDate() + 7);
      const query = new URLSearchParams({from: now.toISOString(), to: until.toISOString(), limit: '8'});
      const [agenda, reminders] = await Promise.all([api('/events?' + query), api('/reminders')]);
      if (!current()) return;
      renderList($('shell-calendar-agenda'), agenda.items);
      renderList($('home-calendar-agenda'), agenda.items);
      renderList($('shell-calendar-reminders'), reminders.items, true);
      $('shell-calendar-count').hidden = !reminders.items.length;
      $('shell-calendar-count').textContent = reminders.items.length || '';
      $('shell-calendar-trigger').setAttribute('aria-label', reminders.items.length
        ? 'Agenda y recordatorios pendientes: ' + reminders.items.length : 'Agenda y recordatorios');
      $('shell-calendar-status').textContent = '';
    } catch (error) {
      if (!current()) return;
      if ([401, 403, 404, 503].includes(error.status)) clear();
      $('shell-calendar-status').textContent = describe(error);
    }
  }

  function session(next) {
    if (profile?.id === next?.id && profile?.csrf_token === next?.csrf_token && next) return;
    profile = next;
    generation++;
    clearInterval(timer);
    clear();
    if (!next) return;
    void refresh();
    timer = setInterval(refresh, 60000);
  }

  document.addEventListener('DOMContentLoaded', () => {
    const form = $('shell-calendar-form');
    if (!form) return;
    const dialog = $('shell-calendar-dialog');
    const fields = form.elements;
    const status = $('shell-calendar-form-status');
    $('shell-calendar').addEventListener('toggle', event => {
      if (event.newState === 'open') void refresh();
    });
    $('shell-calendar-create').addEventListener('click', async () => {
      const revision = generation;
      const button = $('shell-calendar-create');
      button.disabled = true;
      try {
        const {items} = await api('/calendars');
        if (revision !== generation || !active) return;
        form.reset();
        const options = items.filter(item => item.role !== 'reader');
        fields.calendar_id.replaceChildren(...options.map(item => new Option(item.name, item.id)));
        if (!options.some(item => item.is_default)) fields.calendar_id.prepend(new Option('Personal', ''));
        fields.calendar_id.selectedIndex = 0;
        const start = new Date();
        start.setMinutes(start.getMinutes() + 30, 0, 0);
        fields.local_start.value = localTime(start);
        fields.local_end.value = localTime(new Date(start.getTime() + 3600000));
        $('shell-calendar-zone').textContent = 'Zona horaria: ' + zone;
        status.textContent = '';
        $('shell-calendar').hidePopover();
        dialog.showModal();
      } catch (error) {
        if (revision === generation) $('shell-calendar-status').textContent = describe(error);
      } finally { button.disabled = false; }
    });
    $('shell-calendar-cancel').addEventListener('click', () => dialog.close());
    form.addEventListener('submit', async event => {
      event.preventDefault();
      const submit = form.querySelector('[type=submit]');
      const revision = generation;
      submit.disabled = true;
      try {
        await api('/events', 'POST', {
          title: fields.title.value, calendar_id: fields.calendar_id.value || null,
          local_start: fields.local_start.value, local_end: fields.local_end.value, zone
        });
        if (revision !== generation) return;
        dialog.close();
        document.dispatchEvent(new Event('calendar-updated'));
        await refresh();
        if (active) $('shell-calendar').showPopover();
      } catch (error) {
        if (revision === generation) status.textContent = describe(error);
      } finally { submit.disabled = false; }
    });
    document.addEventListener('visibilitychange', () => { if (!document.hidden) void refresh(); });
    if (profile) void refresh();
  });

  window.SantolvCalendar = {api, request, zone, localTime, occurrenceUrl, eventLabel, describe, session, refresh};
})();
