# Área Tareas

Servicio independiente: backend/src/modules/tasks/, frontend/tasks/ y esquema PostgreSQL tasks.
Core controla sesiones, permisos y activación. Identificador: tasks; página /tasks/;
API /api/v1/tasks y /api/v1/tasks/{id}. Los alias históricos
/api/v1/notes-tasks/tasks conservan el mismo control del servicio.

La migración 0021 mueve las tablas sin recrear registros y conserva UUID, fechas,
propietarios y vínculos. Copia activación y denegaciones anteriores a ambos servicios.
Cada uno se puede activar o desactivar sin afectar al otro ni borrar datos.

Tareas conserva note_id como vínculo opcional. Añadir o cambiar un vínculo exige
acceso a Notas y una nota propia; conservar un vínculo existente no exige que
Notas esté activo. Las lecturas y validación de notas pertenecen a Notas.

El controlador y estilo visual reutilizables están en frontend/shared/records.*;
los formularios y páginas están separados. Los estilos solo afectan al contenido
y al editor propios, nunca a la barra global ni a sus diálogos.

Comprobar cargo fmt --check, cargo clippy --locked --all-targets -- -D warnings,
cargo test --locked contra PostgreSQL de pruebas, los cuatro clientes y diff --check.
