# SantolvDrive · HTML independiente

Abrir index.html con doble clic. El archivo permanece en file://, no redirige a localhost ni necesita un servidor web para el frontend. Todo el CSS, JavaScript e iconos está incluido.

Solo debe estar encendida la API. Desde backend:

```powershell
.\dist\santolvdrive-windows-x64.exe serve --config .\servidor\server.json
```

El HTML comprueba http://localhost:8443/health/live. Al pulsar Entrar envía POST /api/v1/auth/login con username, password y local_file: true. La API responde con datos JSON y un token de 30 minutos. El HTML lo guarda exclusivamente en memoria y lo presenta mediante Authorization: Bearer en consultas, cambios, subidas y descargas. credentials: omit impide enviar cookies. Recargar o cerrar la página requiere iniciar sesión otra vez. Cerrar sesión revoca el token en el servidor.

La API nunca entrega esta interfaz. Su raíz devuelve JSON y /index.html devuelve 404. Se ha retirado el comando frontend y no hay redirección automática.

Seguridad: file:// tiene origen opaco null, que también puede proceder de otros documentos aislados. No identifica un archivo de confianza. La API no acepta cookies desde ese origen, no devuelve Access-Control-Allow-Credentials y requiere credenciales válidas y un token para operaciones privadas. Abre únicamente el HTML de confianza del proyecto. No se guardan contraseñas ni tokens en localStorage/sessionStorage ni se incluyen en URLs. Los clientes web alojados conservan sus sesiones con cookies y CSRF. El servidor continúa limitado al mismo equipo.

El navegador puede pedir permiso para acceder a la red local: permite el acceso a localhost para este archivo. Una política del navegador que prohíba acceder a localhost desde archivos no se puede anular desde JavaScript.

Funciones: un único acceso de usuarios; espacios personales y organizaciones; carpetas; subidas reanudables; permisos; papelera; restauración y descargas autenticadas. El servidor verifica los permisos en cada operación.

Límites de esta interfaz: subidas y descargas de hasta 64 MiB, un archivo simultáneo. Las descargas autenticadas se reúnen en memoria; los tokens no aparecen en enlaces. Las subidas usan WebCrypto para el hash completo. La búsqueda filtra lo cargado; permisos y miembros la primera página de 100. Los listados principales tienen paginación. Los metadatos de reanudación se guardan en sessionStorage cuando el navegador lo permite; algunos navegadores restringen ese almacenamiento en file://.

Verificación actual: node tests/frontend-client.cjs y node tests/frontend-http.cjs desde backend. Sin navegador ni capturas; la prueba HTTP requiere PostgreSQL aislado y el binario debug compilado. REHEARSAL.md contiene verificaciones históricas de la interfaz alojada, no una prueba visual del modo file://.

Iconos Lucide/Feather incluidos en SVG. Ver LUCIDE-LICENSE.txt. La apariencia sigue las referencias de stageshow con un acabado monocromo inspirado en iOS 18: modo claro/oscuro automático, tipografía del sistema y listas agrupadas. Sin dependencias de frontend.

Administración: el superusuario es único y solo se gestiona desde el servidor. No inicia sesión en el HTML ni tiene una pantalla web. Para crear usuarios: .\dist\santolvdrive-windows-x64.exe users create --config .\servidor\server.json desde backend; pide el usuario y una contraseña temporal oculta. Los comandos list, disable, enable, reset-password y audit están documentados en el README del backend. La interfaz sigue la organización de Google Drive con el acabado monocromo de iOS 18: Nuevo, Mi unidad, compartidos, papelera, búsqueda sobre lo cargado, filtros, orden por nombre, lista/cuadrícula y detalles. No se simulan funciones ausentes.


Diseño actual: aplicado directamente desde Drive.html proporcionado por el usuario, conservando su CSS original, sus colores y su distribución. Los datos y acciones de demostración se sustituyen por la API existente. La selección actúa sobre un elemento. Recientes, Destacados y Escanear quedan desactivados hasta que exista soporte real; las miniaturas abstractas son decorativas. Ver REHEARSAL.md para las comprobaciones sin capturas.

Subir archivos: botón Subir archivo o Nuevo → Subir archivo. Se admiten tipos personalizados y binarios, hasta 64 MiB por archivo en esta interfaz. La carpeta de cada archivo sigue el esquema `usuarios/nombre_usuario/archivos/Carpeta/nombre.ext/nombre.ext` y contiene un único contenido, sin copias ni historial. Las carpetas, los renombrados y los movimientos se reflejan en disco. El servidor conserva nombres, permisos y ubicaciones en PostgreSQL. Los temporales de operaciones en curso quedan fuera de los usuarios, en `data/.temporal`.

Nuevo abre un menú rectangular de ancho fijo. El clic derecho en archivos o carpetas muestra abrir, renombrar, mover, compartir y enviar a la papelera. También se abre con la tecla de menú contextual o Mayús+F10; se navega con flechas y se cierra con Escape. Mover permite elegir una carpeta por su nombre. En la papelera se puede restaurar o confirmar la eliminación definitiva; esta última retira los bytes en segundo plano y no tiene recuperación. Reemplazar un archivo no conserva la copia anterior.

Los nombres ocupados se rechazan sin sobrescritura. La interfaz avisa si hay una operación de disco pendiente. La aplicación continúa siendo un HTML independiente que llama a la API Rust.

La pantalla inicial es Recientes. El servidor guarda los últimos 200 archivos o carpetas utilizados por cada usuario; el historial persiste al cerrar sesión y vuelve a comprobar los permisos al mostrarlo. Mi unidad sigue disponible para explorar todos los archivos. Destacados se ha retirado.

El propietario puede eliminar un espacio compartido desde su cabecera o su menú contextual. Se solicita confirmación porque borra también archivos, carpetas y papelera para todos los miembros. El contenido se retira en segundo plano y no admite restauración. El espacio personal está protegido.

El clic derecho en un elemento, un espacio o el fondo abre el menú del Drive. También funciona con la tecla de menú contextual o Mayús+F10. Los campos de texto mantienen las opciones habituales de copiar y pegar.
