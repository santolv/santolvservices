# Revisión visual y funcional

## Composición prevista

Aplicación real de escritorio, no un vídeo de marketing: marco completo del producto, sin un navegador dibujado ni una tarjeta envolvente. El foco es el archivo seleccionado y su acción. Como referencia se leyó el gold `stageshow/examples/upload/index.tsx` y `upload.css`, además de craft, framing, products y rehearse. Se trasladaron a HTML sus filas alineadas, jerarquía tranquila, icono de archivo y progreso que termina en un elemento real. No se copiaron componentes React ni se aplicó la regla de recortar controles de una ilustración a una app funcional.

Paleta blanca/cálida, acento verde, tipografía del sistema, bordes ligeros. En 390 px la barra lateral se abre como navegación, los formularios mantienen campos legibles y el detalle pasa debajo de la lista. Los controles conservan foco visible; diálogos nativos, navegación de filas por teclado y reducción de movimiento.

## Recorridos ejecutados

- Ventana de acceso y estado sin sesión, escritorio/móvil.
- Administrador real: crear una cuenta sin acceder a su contenido.
- Cuenta con contraseña temporal: cambio obligatorio, revocación e inicio de sesión nuevo.
- Carpetas reales, navegación y migas de pan.
- Archivo de 5,4 MiB: respuesta de creación perdida, reintento con la misma clave, interrupción tras el primer bloque, reanudación desde 4 MiB confirmados y finalización real.
- Descarga cuyo SHA-256 coincide con el contenido original.
- Compartir con un lector: descarga permitida y escritura rechazada.
- Historial real, renombrado, papelera y restauración; lista y cuadrícula.
- Ausencia de excepciones JavaScript y errores CSP. La respuesta 401 de `/me` antes de iniciar sesión es esperada.
- Capturas de acceso, administración, archivos, permisos, historial y cuadrícula a 1440 × 960; acceso, archivos y cuadrícula a 390 × 844. Revisadas visualmente y comprobadas sin desbordamiento horizontal.

Las capturas y el ejecutor de pruebas están en `backend/target/frontend-build/` y no forman parte del producto. Las cuentas, la base y el proceso del servidor de pruebas se eliminan al terminar. No se insertan datos de muestra en instalaciones del usuario.

## Qué no se verificó

No se ejecutó el film `--gate` del kit Stage: no hay una escena React, un cursor dibujado ni un bucle; se comprobaron las interacciones reales con Chrome y capturas. No se certifican Safari/Firefox, dispositivos físicos, lector de pantalla, más de 64 MiB, ni una auditoría formal WCAG. La restauración a una versión anterior, paginaciones masivas y revocaciones de permisos dependen de endpoints existentes; la suite del backend los cubre, pero no todos los diálogos se han recorrido de extremo a extremo en esta prueba de navegador.

Se sustituyeron los glifos iniciales por un único conjunto de SVG oficiales de Lucide, con cajas y trazo uniformes. El ajuste final de iconos no cambia los recorridos funcionales comprobados. Se capturó de nuevo el acceso con este conjunto.

## Actualización: HTML independiente
El HTML se sirve desde localhost:5500 y solicita la API en localhost:8443. Se retiró include_str; los cambios del HTML se leen sin recompilar. En esta actualización solo se ejecutaron pruebas de código y HTTP (CORS, cookies, CSRF y conexión). No se hicieron nuevas capturas ni pruebas visuales, conforme a la instrucción del usuario. Las capturas descritas antes son antecedentes de la entrega anterior.

## Revisión de diseño · 21 de noviembre de 2026

Referencias consultadas: stageshow/examples/finder/finder.css, examples/upload/upload.css y references/craft.md; skill apple-design para tipografía del sistema, respuesta de controles y accesibilidad. Se aplican a una interfaz real en HTML, conservando sus controles, sin añadir React, dependencias ni una escena simulada.

Cambios: paleta monocroma inspirada en iOS 18, superficies agrupadas, navegación lateral, modo claro/oscuro del sistema, controles táctiles y diálogos; se eliminan ilustraciones, degradados y textos promocionales. La conexión, autenticación, subidas y descargas conservan su JavaScript; solo se sustituyen textos visibles. La API no se modifica.

Verificación: tests/frontend-client.cjs; comparación del script antes/después descontando los textos sustituidos; todos los identificadores HTML conservados; comprobación estática de paleta y reglas para modo oscuro, movimiento reducido y contraste alto. No se ha abierto navegador, filmado, usado capturas ni ejecutado el gate visual de stageshow, por petición del usuario. El ajuste visual real en escritorio/móvil y ambos temas queda sin verificar.

## Drive solo para usuarios · 21 de noviembre de 2026

Aplicadas stageshow y apple-design: composición de explorador de archivos, búsqueda superior, botón Nuevo lateral, navegación de Mi unidad/Compartido/Papelera y panel principal redondeado. Monocromo claro/oscuro con controles de iOS 18. Se retiran todas las pantallas y acciones web de administración. El archivo continúa funcionando desde file://; la API solo devuelve datos y contenidos autorizados.

Comprobaciones sin navegador ni capturas: contrato JavaScript de login/token/descargas; rechazo de una identidad administrativa; filtro de tipo; pruebas HTTP del backend y comandos de consola; pruebas Rust de permisos, revocación, sesiones antiguas y migración con datos existentes. No se ejecutó el gate visual de stageshow por instrucción del usuario. No se afirma equivalencia visual píxel a píxel con Google Drive.

## Diseño proporcionado por el usuario · 22 de noviembre de 2026

Referencia: C:\Users\Santi\Downloads\Drive.html. Su bloque CSS se incorpora íntegro, seguido únicamente de reglas para integrar el acceso, diálogos y funciones existentes. Se copia su estructura de cabecera, navegación, carpetas, tarjetas, vista de lista, menú contextual, barra de selección y panel inferior. Se mantienen los colores y las proporciones de la referencia, incluidos los temas y la navegación móvil.

Los archivos, carpetas, cuotas y acciones simuladas de la referencia se sustituyen por los datos y operaciones reales. La selección es de un elemento. Recientes, Destacados y Escanear se muestran desactivados: no hay soporte en la API. Las miniaturas de líneas son la representación decorativa de la referencia, no vistas previas del contenido real. Versiones, renombrado, permisos, movimiento, sustitución, papelera y restauración permanecen disponibles en los paneles.

Verificación sin navegador ni capturas: coincidencia íntegra del CSS original; comparación del código de login, petición API, subida y descarga con la versión anterior; contrato de token y cookies; cuadrícula/lista, selección, nombres escapados y apertura/cierre del panel inferior. Identificadores HTML únicos y CSS equilibrado. No se modifican el backend ni sus rutas. La equivalencia visual exacta y los gestos en dispositivos reales no se han comprobado.

Corrección funcional: aceptado 202 vacío en el borrado definitivo, carpetas de papelera accesibles y nombres de descarga originales. Prueba JavaScript de la confirmación y prueba HTTP con verificación del archivo real, restauración, retirada física y cuota. No se ha abierto navegador ni usado capturas.
