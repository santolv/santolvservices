# Revisión

Separación a partir del servicio unificado. Sin dependencias nuevas.

Revisadas con Edge las páginas y editores con la barra común real a 1366×900 y
390×900, en claro y oscuro. Sin desbordamiento horizontal; botones de la barra
32×32 y botón de creación con radio de 8px. Contornos visibles en oscuro.
Las capturas usan datos sintéticos y no ejecutan operaciones sobre cuentas reales.

Pruebas Rust: aislamiento entre dos usuarios, CRUD, permisos, activación independiente,
alias protegidos, vínculos conservados con Notas desactivado y migración desde
la versión anterior preservando IDs, propietarios, registros y denegaciones.
Validación local contra un clúster PostgreSQL exclusivo de esta tarea.
