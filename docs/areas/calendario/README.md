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
