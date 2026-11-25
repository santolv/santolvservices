# Área Notas y Tareas

Servicio unificado para notas personales y tareas. Su código pertenecerá a:

- Backend Rust: `backend/src/modules/notes_tasks/`
- Frontend: `frontend/notes-tasks/`
- Migración PostgreSQL: siguiente número disponible en `backend/migrations/`
- Chat: `SantolvDrive · Notas y Tareas`
- Rama del chat: `codex/chat-notas-tareas`

## Alcance inicial

Notas y tareas serán un único servicio y módulo, con rutas y tablas propias.
Core seguirá siendo responsable de autenticación, permisos, configuración y
activación. El servicio no podrá ser instalado ni activado por usuarios comunes.

## Contrato v1 aprobado

El backend reutilizará `axum`, `tokio`, `sqlx`, `serde`, `uuid` y `time` del
proyecto. La lógica irá en archivos `.rs`, el esquema en una migración `.sql`
y la página usará HTML, CSS y JavaScript. La API intercambiará JSON. Las notas
y las tareas serán registros de PostgreSQL. La v1 usa texto plano y prescinde
de adjuntos y de la dependencia de Drive.

### Datos

- `notes_tasks.notes` contiene `id`, `owner_id`, `title`, `content`, `archived_at`,
  `created_at` y `updated_at`. `archived_at` es nulo mientras la nota está activa.
- `notes_tasks.tasks` contiene `id`, `owner_id`, `title`, `status`, `priority`,
  `due_at`, `note_id`, `created_at` y `updated_at`. `due_at` y `note_id` admiten
  nulos. El estado admite `pending`, `in_progress` y `done`. La prioridad admite
  `low`, `normal` y `high`. PostgreSQL restringe ambos conjuntos de valores.
- Los identificadores son UUID. `owner_id` referencia `public.users`. La clave
  foránea `(owner_id, note_id)` referencia `(owner_id, id)` de notas. Archivar
  una nota conserva sus tareas y sus vínculos. Las notas se archivan y restauran.
  Las tareas se pueden eliminar de forma individual.
- Las fechas son `timestamptz`. La API usa RFC 3339 y devuelve fechas en UTC.
  El servidor establece los identificadores, las fechas de auditoría y el
  propietario. En cada modificación, actualiza `updated_at`.

### API

Las rutas cuelgan de `/api/v1/notes-tasks`. Las escrituras requieren JSON.

| Método y ruta relativa | Operación y respuesta |
| --- | --- |
| `GET /notes` | Listar notas. `archived=false` por defecto, `archived=true` para listar archivadas. |
| `POST /notes` | Crear nota activa con `title` y `content`. Devuelve `201` y la nota. |
| `GET /notes/{id}` | Leer una nota propia, incluida una nota archivada. Devuelve `200`. |
| `PUT /notes/{id}` | Sustituir `title` y `content` y aplicar `archived` como booleano. Devuelve `200` y la nota. |
| `GET /tasks` | Listar tareas con filtros opcionales `status`, `priority` y `note_id`. |
| `POST /tasks` | Crear tarea con `title`. Por defecto, `status=pending`, `priority=normal`, `due_at=null` y `note_id=null`. Devuelve `201` y la tarea. |
| `GET /tasks/{id}` | Leer una tarea propia. Devuelve `200`. |
| `PUT /tasks/{id}` | Sustituir `title`, `status`, `priority`, `due_at` y `note_id`. `null` limpia el vencimiento o el vínculo. Devuelve `200` y la tarea. |
| `DELETE /tasks/{id}` | Eliminar una tarea propia. Devuelve `204`. |

- Los objetos de respuesta contienen los campos del registro salvo `owner_id`.
  Las operaciones `PUT` requieren todos los campos editables de su fila. Al
  archivar, el servidor fija `archived_at` en la primera transición a archivada.
  Al restaurar, lo vuelve nulo.
- Los listados devuelven `200` con `{ "items": [...], "next_cursor": null }`.
  Aceptan `limit` entre 1 y 100, con 50 por defecto, y `cursor` opcional. El
  cursor es opaco para el cliente y codifica `created_at` e `id`, ambos en orden
  descendente. `next_cursor` contiene el cursor si existen más resultados.
- Los listados aceptan `q`, una subcadena literal con búsqueda sin distinción
  de mayúsculas, en el título y contenido de notas o en el título de tareas.
  PostgreSQL ejecuta la búsqueda y combina los filtros. Las consultas usan
  parámetros y escapan los comodines para que `%` y `_` sean literales.
- El título recortado admite de 1 a 200 caracteres. El contenido admite de 0
  a 50 000 caracteres y la búsqueda hasta 200. Un `q` vacío tras recortarlo
  equivale a omitirlo. Los valores inválidos devuelven `400`. Los cuerpos que
  superan el límite de lectura devuelven `413`, y las escrituras que esperan
  un cuerpo JSON devuelven `415` si el tipo de contenido es incompatible.

### Permisos

Core autentica la sesión y comprueba la disponibilidad y el acceso al módulo
antes de ejecutar las rutas. Cada consulta del módulo limita los registros al
usuario autenticado, también cuando busca por UUID o comprueba el vínculo con
una nota. Un UUID ajeno o inexistente devuelve `404 not_found`. El módulo usa
el formato de error de Core y conserva sus respuestas `401` para una sesión
ausente, `403` para acceso denegado, `404` para un servicio sin instalar y
`503` para un servicio desactivado. La instalación y la activación quedan bajo
el control administrativo de Core.

## Integración

La migración `0019_notes_tasks.sql` prepara el esquema. El descriptor `Service`
declara el identificador `notes-tasks`, el esquema `notes_tasks`, la página
`/notes-tasks`, `requires: None` y `uses_file_store: false`. La integración debe
confirmar la numeración de la migración y coordinar con Core el registro en
`backend/src/modules/mod.rs`, los enlaces de navegación y la instalación del
módulo en estado desactivado.

No modificar migraciones ya aplicadas. No cambiar API, shell, gestor o motor de
archivos desde esta área sin coordinar su PR con el área responsable.

## Verificación

```sh
cargo fmt --manifest-path backend/Cargo.toml --check
rustfmt --edition 2024 --check backend/src/modules/notes_tasks/mod.rs
cargo clippy --manifest-path backend/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path backend/Cargo.toml --locked
git diff --check
```

Las pruebas del módulo se compilan cuando `modules/mod.rs` declara `notes_tasks`.
Hasta la integración, probarlas en una copia aislada con el descriptor añadido
a `REGISTERED` y una base PostgreSQL de pruebas. La migración no instala el
servicio. Las pruebas insertan el estado de instalación en su propia base.

Probar en un PostgreSQL aislado la migración desde una base vacía y desde la
versión anterior. Las pruebas de API deben cubrir creación, edición, archivado,
restauración, filtros y paginación. Con dos usuarios, comprobar el aislamiento
de lecturas, escrituras y búsquedas, así como el rechazo de vínculos cruzados.
Verificar también que Core bloquea el acceso sin sesión, sin permiso o con el
módulo desactivado y que un usuario común no puede instalarlo ni activarlo.
