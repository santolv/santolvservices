# Área Calendario

Servicio independiente para organizar calendarios y eventos personales o
compartidos.

- Backend Rust: `backend/src/modules/calendar/`
- Frontend: `frontend/calendar/`
- Migración PostgreSQL: siguiente número disponible en `backend/migrations/`
- Chat: `SantolvDrive · Calendario`
- Rama inicial: `codex/scaffold-calendario`

## Alcance inicial

Antes de registrarlo en el backend, definir:

- calendarios personales y compartidos;
- eventos con título, descripción, inicio, fin y zona horaria;
- eventos de día completo y repetición;
- recordatorios y estados de disponibilidad;
- permisos de lectura y edición;
- filtros por calendario y rango temporal;
- relación opcional con Notas y Tareas, sin acoplar tablas entre servicios.

Core controla autenticación, permisos globales, configuración y activación. Los
usuarios comunes no pueden instalar ni activar el módulo. Si requiere adjuntos,
debe usar `file_store` mediante coordinación con Compartido.

## Integración

Cuando el contrato esté aprobado, crear el descriptor `Service`, añadirlo a
`backend/src/modules/mod.rs`, crear una migración nueva y añadir la página al
frontend. Elegir explícitamente `requires: None` o documentar una dependencia;
no asumir dependencia de Drive o Notas y Tareas.

No modificar migraciones aplicadas ni contratos comunes desde este módulo.

## Verificación

```sh
cargo fmt --manifest-path backend/Cargo.toml --check
cargo clippy --manifest-path backend/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path backend/Cargo.toml --locked
git diff --check
```
