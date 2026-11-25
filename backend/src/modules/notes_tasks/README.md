# Servicio Notas y Tareas

Este directorio reserva la lógica Rust del futuro servicio unificado de notas y
tareas. El servicio compartirá autenticación, permisos, gestor de módulos y
PostgreSQL con el monolito central.

Todavía no se registra en `modules::REGISTERED` ni se añade a producción: el
chat del área debe diseñar primero el contrato de API y la migración.

Al implementarlo, cada usuario tendrá sus notas y tareas aisladas por permisos.
Las tareas podrán tener estado, prioridad, vencimiento y relación opcional con
una nota. No se debe copiar la sesión ni crear otro backend.
