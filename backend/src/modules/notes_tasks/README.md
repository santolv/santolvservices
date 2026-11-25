# Servicio Notas y Tareas

Este directorio contiene la lógica Rust del servicio unificado de notas y
tareas. Las rutas usan la autenticación de Core y el pool de PostgreSQL del
gestor de módulos.

El descriptor `SERVICE` se registra en `modules::REGISTERED` y la migración
instala el servicio desactivado. El contrato y las comprobaciones se describen en la
[ficha del área](../../../../docs/areas/notas-tareas/README.md).
