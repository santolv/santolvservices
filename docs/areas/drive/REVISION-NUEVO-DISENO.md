# Nuevo diseño de Drive y separación final de interfaces

Drive adapta drive (1).html aportado por el usuario: barra compacta, rejilla de
iconos, selección sobre icono y nombre, lista de columnas y filas alternas, orden
por nombre/fecha, menús y pie de estado. Conserva la barra global compartida,
carpetas Finder claro/oscuro, colores por tipo y las operaciones existentes del
servidor: subir, conflictos, carpetas, permisos, espacios, mover, papelera,
restaurar, descarga y arrastre. No se importan datos ni acciones simuladas.
La selección del cliente existente sigue siendo de un elemento.

Notas y Tareas tienen HTML, campos y controladores separados sin pestañas entre
ambas. Comparten únicamente estilo, transporte API y sesión. Se conserva el vínculo
opcional de una tarea con una nota y los enlaces privados desde Calendario.
Calendario usa cheurones SVG y selección gris en el segmento activo en oscuro.

Verificado con JavaScript real y API sintética en Edge: cuatro pantallas, dos temas,
escritorio 1366 y móvil 390; Drive en iconos/lista y selección, creación de notas y
tareas y ausencia de pestañas cruzadas. Sin desbordamiento ni errores JS.
Pasan los seis clientes Node, incluida la prueba de que un controlador no accede a
campos de la otra pantalla, ordenación y los contratos de archivos existentes.
Sin cambios Rust, dependencias, migraciones ni escrituras sobre cuentas privadas.
