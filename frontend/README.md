# SantolvPhotos · HTML base

Abre `index.html` en un navegador de escritorio. Funciona como archivo local,
sin instalación, compilación, framework, conexión a una API ni recursos de CDN.
Conserva `assets/` junto al HTML para que aparezcan las fotos de muestra.

El diseño se inspira en las capturas oficiales de Fotos de Apple para Mac:
barra lateral compacta, cuadrícula continua de borde a borde, herramientas
flotantes, vistas temporales y visor sobre fondo oscuro. No se incluyen logotipos
ni capturas de Apple dentro del producto. La identidad sigue siendo SantolvPhotos.
La base técnica se conserva de SantolvDrive: HTML/CSS/JavaScript sin framework.

## Qué puedes probar

- Veinte fotos locales en una cuadrícula continua, sin tarjetas ni títulos bajo las imágenes.
- Vistas Años, Meses y Todas las fotos; entrar en un año abre sus meses y en un mes sus fotos.
- Búsqueda por nombre/lugar, filtros y orden desde el botón de filtros.
- Ajuste del tamaño de las miniaturas con el deslizador y los botones −/+ en escritorio.
- Tres álbumes de muestra, favoritos, papelera y restauración.
- Visor con flechas, Escape, zoom y favoritos.
- Temas claro/oscuro y preferencia del sistema desde Ajustes.
- Selección o arrastre de imágenes para previsualizarlas en la pestaña, hasta
  30 MB por archivo, en JPEG, PNG, WebP, GIF o AVIF.

Las fechas, nombres y colecciones son ficticios. Solo el tema se guarda en
localStorage; favoritos, papelera e imágenes añadidas se reinician al recargar.
No existe autenticación, subida al servidor, almacenamiento remoto ni backup.
La información de cuenta es de demostración y no solicita credenciales.

El alcance actual es la interfaz web/de PC en navegador. La distribución móvil
y las aplicaciones descargables quedan pendientes de decisión del propietario.
El HTML se adapta a ventanas estrechas, sin implementar PWA ni instaladores.

## Recursos y verificación

Ver [ASSETS.md](ASSETS.md) para las fotografías, la referencia de diseño y sus
fuentes. La licencia de los iconos se conserva en [LUCIDE-LICENSE.txt](LUCIDE-LICENSE.txt).

Esta versión se ha revisado visual e interactivamente mediante una vista previa
HTTP local en el navegador integrado: años → meses → fotos, búsqueda, filtro de
ciudad, visor, flecha de teclado, Escape y favoritos; revisión de temas claro y
oscuro. No se observaron errores de consola ni desbordamiento horizontal en las
ventanas observadas de 1280 y 347 píxeles. El archivo local sigue funcionando sin
servidor; HTTP se utilizó solo para la verificación. Sintaxis JavaScript, imágenes,
referencias locales, identificadores HTML y diff de Git también comprobados.
Quedan pendientes las pruebas en otros navegadores y dispositivos reales.
