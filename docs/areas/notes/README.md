# Área Notas

Servicio independiente: backend/src/modules/notes/, frontend/notes/ y esquema PostgreSQL notes.
Core controla sesiones, permisos y activación. Identificador: notes; página /notes/;
API /api/v1/notes y /api/v1/notes/{id}. Los alias históricos
/api/v1/notes-tasks/notes conservan el mismo control del servicio.

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

## Editor y eliminación

El editor tiene título y contenido sin bordes, acciones Archivar/Restaurar,
Eliminar y Hecho, y fondo desenfocado con degradado. `DELETE /api/v1/notes/{id}`
elimina únicamente una nota propia. Las tareas vinculadas se conservan y su
`note_id` pasa a null dentro de la misma transacción; Calendario limpia sus
referencias automáticamente. La interfaz confirma antes de eliminar.
