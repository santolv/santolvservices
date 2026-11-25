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

La primera implementación debe definir antes de codificar:

- notas con título, contenido, archivado y fechas;
- tareas con título, estado, prioridad, vencimiento y fechas;
- relación opcional entre una tarea y una nota;
- filtros y búsqueda limitados a este servicio;
- permisos por usuario antes de leer o modificar cualquier registro;
- si los adjuntos son necesarios, usar `file_store` mediante coordinación con
  Compartido, sin crear almacenamiento paralelo.

## Integración

Cuando el contrato esté aprobado, crear el descriptor `Service`, añadirlo a
`backend/src/modules/mod.rs`, crear una migración nueva y añadir la página al
frontend. Si no depende de Drive, debe declarar `requires: None`; si se decide
que necesita Drive, documentar esa dependencia antes de registrarlo.

No modificar migraciones ya aplicadas. No cambiar API, shell, gestor o motor de
archivos desde esta área sin coordinar su PR con el área responsable.

## Verificación

```sh
cargo fmt --manifest-path backend/Cargo.toml --check
cargo clippy --manifest-path backend/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path backend/Cargo.toml --locked
git diff --check
```

El módulo debe quedar desactivado por defecto hasta que Core registre su
instalación y el administrador lo active.
