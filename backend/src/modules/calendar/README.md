# Servicio Calendario

Calendario usa la sesión del Core y sus permisos de módulo. La migración crea el
esquema `calendar` y deja el servicio deshabilitado. Un administrador del servidor
puede activarlo mediante `services enable calendar` en la consola local.

## API

La API se publica bajo `/api/v1/calendar` y figura en OpenAPI. Las escrituras
autenticadas con cookie requieren el token CSRF de Core.

- `/calendars` lista los calendarios accesibles y su rol. El propietario puede
  modificar su calendario y dar acceso de lectura o edición a otra cuenta por
  nombre de usuario mediante `/calendars/{id}/members`.
- `GET /events` requiere `from` y `to` en RFC 3339, con un máximo de 62 días.
  Acepta `calendar_id`, `limit` y `offset`. Devuelve `items` y `next_offset`.
  Cada ocurrencia contiene `event_id`, `original_start`, calendario, rol,
  revisión, campos locales e instantes `starts_at` y `ends_at`.
- `POST /events` crea el evento en un calendario editable. Si se omite
  `calendar_id`, usa el calendario personal predeterminado y lo crea si hace falta.
  Enviar `local_start`, `local_end` y `zone` junto al título. Un evento de día
  completo usa medianoche local y fecha de fin excluida.
- `GET /events/{id}` muestra la definición de la serie. Para sustituirla o
  eliminarla, el cliente envía su `revision`. Las modificaciones de horario o
  repetición que borrarían excepciones devuelven `confirm_clear_exceptions` y
  requieren `clear_exceptions: true` para repetir el cambio.
- `/events/{id}/occurrences/{original}` acepta el inicio local original, incluso
  si la ocurrencia se mueve. Un `PUT` con `revision` y `event` la sustituye.
  Enviar `cancelled: true` en lugar de `event` para cancelarla.
- `PUT /events/{id}/reminder` guarda el aviso personal de un evento o serie en
  minutos de antelación. Para el día completo, la hora de referencia son las
  09:00 en la zona del evento. `GET /reminders` muestra avisos pendientes de los
  últimos siete días. `POST /reminders` los descarta por `event_id`,
  `original_start` y `due_at`.

Las recurrencias conservan la hora de inicio local y los eventos con hora
conservan la duración transcurrida. Si en un mes falta el día original, se usa
el último día del mes. PostgreSQL resuelve las horas ambiguas. Si una ocurrencia
cae en un salto de hora, se desplaza hacia delante por la duración del salto.
Los horarios de inicio y fin introducidos manualmente deben existir en la zona
seleccionada.
