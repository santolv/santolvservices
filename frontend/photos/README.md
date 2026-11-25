# Fotos

Servida por el backend central en /photos/. Conserva HTML, CSS, JavaScript,
Liquid Glass, selección, visor, temas y navegación temporal propios.

Inicia sesión en / y entra en Fotos. Importar sube originales; dentro de una
colección, los guarda en la carpeta del álbum. Colecciones permite crear álbumes.
Favoritos, papelera y restauración esperan confirmación del servidor y se
conservan tras recargar. Las muestras de la maqueta no se usan como datos de cuenta.

Cargar más continúa la consulta paginada. La búsqueda abarca lo cargado. El visor
usa originales autenticados: no hay miniaturas derivadas todavía. La fecha es la
de subida. Formatos: JPEG/PNG/WebP/GIF/AVIF, hasta 30 MiB.

No abrir con file://: necesita servidor y sesión. Consulta [la guía central](../../README.md).

La lógica del servicio vive en [el módulo Rust de Fotos](../../backend/src/modules/photos/).
El [plan](PLAN.md) recoge las funciones pendientes y el historial de diseño.
