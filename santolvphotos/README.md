# SantolvPhotos

Interfaz de Fotos de [Santolv Services](../README.md), conectada al backend Rust
compartido de ../backend. Mantiene su distribución y animaciones propias.

Abre /photos/ desde el servidor de Santolv. Acceso y sesión comunes con Drive;
Inicio vuelve al selector de servicios. Ya no es una demo sin conexión ni se
configura una URL independiente para Fotos.

Incluye subidas verificadas, biblioteca paginada, álbumes como carpetas,
favoritos persistentes, papelera y restauración. Admite JPEG, PNG, WebP, GIF y
AVIF de hasta 30 MiB. La fecha corresponde a la subida; las miniaturas derivadas,
EXIF, vídeos y aplicaciones móviles quedan pendientes.

Repositorio activo: santolv/santolvservices. El historial anterior de Fotos se
conserva importado. Consulta [el plan](frontend/PLAN.md) y la guía central.
