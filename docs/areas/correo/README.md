# Correo

El módulo `mail` sirve la página `/mail/` y monta su API en `/api/v1/mail`. Core gestiona identidad, sesión, permisos y activación. Los mensajes viven en PostgreSQL con propietario, revisión y fechas. La migración 0024 instala el módulo desactivado, igual que Notas y Calendario.

La primera versión permite redactar y guardar borradores con Para, CC y CCO, buscar, destacar, archivar, mover a la papelera, restaurar y borrar definitivamente. Entrada y Enviados tienen sus vistas y estados vacíos. La aplicación identifica si el transporte está conectado y guarda borradores en el servidor. Contactos aporta sugerencias cuando está disponible.

Para la integración futura, el modelo diferencia mensaje recibido, enviado y borrador. Un adaptador de transporte será responsable de insertar mensajes recibidos y de registrar envíos confirmados. El contrato local únicamente crea borradores y no contiene credenciales de correo.

Cada lectura y escritura filtra por el usuario de Core. La revisión evita sobrescribir cambios de otra sesión. El borrado definitivo requiere pasar primero por la papelera. El contenido de los mensajes se representa como texto plano.
