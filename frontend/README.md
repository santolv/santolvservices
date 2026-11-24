# SantolvPhotos · HTML base

Abre `index.html` en un navegador de escritorio. Funciona como archivo local,
sin instalación, compilación, framework, conexión a una API ni recursos de CDN.
Conserva `assets/` junto al HTML para que aparezcan las fotos de muestra.

La estructura, los colores base, la tipografía del sistema, la barra lateral,
el buscador, los botones, los temas y los iconos siguen el frontend de
[SantolvDrive](https://github.com/santolv/santolvdrive/tree/b9c19e100e15c85534ede1a38e331411d4d2edad/frontend/pruebahtml).
La galería, los álbumes y el visor se han adaptado para SantolvPhotos.

## Qué puedes probar

- Fotos por fecha, búsqueda por nombre/lugar, filtros y dos densidades de cuadrícula.
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

La comprobación de esta entrega cubre sintaxis JavaScript, referencias locales,
identificadores HTML y diff de Git. La revisión interactiva en el navegador
integrado quedó bloqueada porque no admite abrir direcciones `file://`.
La comprobación visual e interactiva queda pendiente en un navegador de escritorio.
