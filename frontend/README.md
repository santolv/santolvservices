# SantolvPhotos · HTML base

Abre `index.html` en un navegador de escritorio. Funciona como archivo local,
sin instalación, compilación, framework, conexión a una API ni recursos de CDN.
Conserva `assets/` junto al HTML para que aparezcan las fotos de muestra.

El diseño actual sigue las capturas de iCloud Photos aportadas por el propietario:
cabecera de marca, barra lateral clara, herramientas separadas de las imágenes,
fechas visibles, cuadrícula con pequeños márgenes y selección azul. Se mantiene
Liquid Glass en controles, buscador, filtros y ventanas: transparencia, desenfoque,
saturación, bordes iluminados y sombras suaves, con alternativa opaca si el
navegador no admite desenfoque. No se incluyen logotipos
ni capturas de Apple dentro del producto. La identidad sigue siendo SantolvPhotos.
La base técnica se conserva de SantolvDrive: HTML/CSS/JavaScript sin framework.

## Qué puedes probar

- Veinte fotos locales en una cuadrícula ordenada, sin títulos bajo las imágenes.
- Vistas Años, Meses y Todas las fotos; entrar en un año abre sus meses y en un mes sus fotos.
- Búsqueda por nombre/lugar, filtros y orden desde el botón de filtros.
- Ajuste del tamaño de las miniaturas con el deslizador y los botones −/+ en escritorio.
- Tres álbumes de muestra, favoritos, papelera y restauración.
- Selección múltiple con contornos azules, contador y seleccionar/deseleccionar
  todas las fotos de la vista. También puedes usar Ctrl, Cmd o Mayús al pulsar una foto
  para activar la selección. Favoritos y papelera admiten acciones sobre el grupo.
  No se seleccionan fotos ocultas por los filtros y la selección se limpia al navegar.
- Visor con flechas, Escape, zoom y favoritos.
- Temas claro/oscuro y preferencia del sistema desde Ajustes.
- Selección o arrastre de imágenes para previsualizarlas en la pestaña, hasta
  30 MB por archivo, en JPEG, PNG, WebP, GIF o AVIF.
- Entrada escalonada de fotos y colecciones, transiciones entre periodos y
  reorganización animada al cambiar el tamaño de las miniaturas.
- Apertura/cierre animados del visor, transición al avanzar de foto y respuesta
  visual de botones y favoritos. Marcar un favorito conserva el zoom del visor.
- Indicadores de filtros activos con botón para quitarlos y «Deshacer» durante
  siete segundos al enviar una foto a la papelera de demostración.
- Atajo `F` para favoritos en el visor, `/` para buscar y Escape para cerrar
  ventanas o limpiar la búsqueda. La ventana de información incluye una guía.

Las animaciones usan CSS y las APIs nativas del navegador, sin dependencias.
Se desactivan cuando el sistema indica `prefers-reduced-motion: reduce`; la
navegación, los filtros y los diálogos siguen funcionando sin movimiento.
La entrada escalonada se limita a un máximo de 24 elementos visibles.

Los detalles de Liquid Glass incluyen una lente que se desliza entre Años,
Meses y Todas las fotos, y una segunda lente que sigue la opción señalada o con
foco. El selector también admite flechas izquierda/derecha, Inicio y Fin.
Las lentes recalculan su posición al cambiar de vista, de tamaño o de tipografía.

Al pasar el cursor sobre una foto, o enfocarla con el teclado, aparece una tarjeta
de cristal con nombre, fecha y lugar de muestra. Se oculta durante la selección
múltiple y no se exige hover en dispositivos táctiles. Los controles incluyen
reflejos suaves y ayudas flotantes; las colecciones y la navegación responden
con pequeños movimientos. No se añaden animaciones continuas en reposo.

Las fechas, nombres y colecciones son ficticios. Solo el tema se guarda en
localStorage; favoritos, papelera e imágenes añadidas se reinician al recargar.
No existe autenticación, subida al servidor, almacenamiento remoto ni backup.
La interfaz no solicita credenciales.

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

En la entrega de animaciones también se comprobaron: zoom seguido de favorito
con `F` sin perder la ampliación, papelera → Deshacer (19 → 20 fotos), navegación
temporal, quitar el filtro de mes, búsqueda y su limpieza con Escape, y cambio
de tamaño de miniaturas. No hubo errores en la consola de la aplicación.

En la adaptación a las capturas se revisaron visualmente los temas claro y oscuro,
el contador y los contornos de selección, y papelera de grupo → Deshacer
(20 → 18 → 20 fotos). En escritorio a 1280 píxeles, la barra de herramientas,
el título/fecha y la cuadrícula quedan separados y no hay desbordamiento horizontal.
El desenfoque efectivo de los controles es de 26 píxeles con saturación del 180 %.

En la entrega de microinteracciones se comprobaron la tarjeta de detalles, las
ayudas de controles, la navegación Años → Meses → Todas las fotos con ratón y
teclado y la alineación de la lente con el botón activo (4 px en Años y 123 px
en Todas las fotos en la ventana de prueba). Sin errores de consola observados.
