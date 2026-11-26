# Área Calendario

Servicio independiente para organizar calendarios y eventos personales o
compartidos.

- Backend Rust: `backend/src/modules/calendar/`
- Frontend: `frontend/calendar/`
- Migración PostgreSQL: siguiente número disponible en `backend/migrations/`
- Chat: `SantolvDrive · Calendario`
- Rama inicial: `codex/scaffold-calendario`

## Primera versión

- calendarios personales y compartidos;
- eventos con título, descripción, inicio, fin y zona horaria;
- eventos de día completo y repetición;
- recordatorios y estados de disponibilidad;
- permisos de lectura y edición;
- filtros por calendario y rango temporal;
- agenda, creación rápida y recordatorios desde la barra global, con agenda de
  siete días en Inicio.

Core controla autenticación, permisos globales, configuración y activación. Los
usuarios comunes no pueden instalar ni activar el módulo. Si requiere adjuntos,
debe usar `file_store` mediante coordinación con Compartido.

## Integración

El descriptor usa `requires: None`. La migración instala el esquema y el
registro deshabilitado. La activación se hace con `services enable calendar`
desde la consola local del servidor. El contrato está documentado en
[la ficha de la API](../../../backend/src/modules/calendar/README.md).

La acción para crear un evento enlazado desde una tarea requiere la API estable
de Notas y Tareas. Se integrará mediante un identificador opaco y mantendrá los
permisos de origen de la tarea.

## Verificación

```sh
cargo fmt --manifest-path backend/Cargo.toml --check
cargo clippy --manifest-path backend/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path backend/Cargo.toml --locked
git diff --check
```

## Vínculos personales con Notas y Tareas

`GET/PUT /api/v1/calendar/events/{id}/links/{notes|tasks}` lee o sustituye
`{ "id": UUID|null }` para el usuario autenticado. Exige acceso al evento y al
servicio de origen, y el registro debe pertenecer al usuario. Los vínculos son
privados incluso en calendarios compartidos. No copian contenido de notas ni
cambian vencimientos o estados de tareas. Se gestionan al abrir un evento guardado.
La migración 0022 añade las referencias y las limpia si se elimina el origen.

La semana muestra horas, bloques por duración, eventos solapados en carriles y
una fila de día completo. El panel usa el botón compartido para mostrar/ocultar.
El editor mantiene repetición, zonas horarias y recordatorios en opciones plegables.
