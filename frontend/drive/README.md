# Drive

Abrir [Drive](http://localhost:5500/drive/) para usar la sesión compartida de Santolv Services. El CSS, JavaScript e iconos de Drive están en `index.html`. La barra superior vive en `../shared/`. El ejecutable del backend incorpora ambos al compilar. Las rutas y la lógica propias están en [el módulo Rust de Drive](../../backend/src/modules/drive/).

Desde backend:

```powershell
.\servidor\santolvdrive.exe serve
```

La sesión se guarda en una cookie HttpOnly de dos horas, con SameSite=Strict y ámbito de API. El servidor verifica la caducidad, restaura CSRF al recargar y revoca la sesión al cerrarla. Una contraseña temporal solo permite quince minutos para cambiarla. No se guardan contraseñas ni tokens de acceso en localStorage/sessionStorage ni en URLs. Si abres index.html con doble clic, muestra el enlace a la interfaz con cookies en lugar de iniciar una sesión opaca.

El listener del HTML está restringido a localhost y aplica CSP con nonce. Para configurar un origen HTTPS y su modelo de proxy, consulta [la documentación de seguridad](../../backend/INFORME_SEGURIDAD.md).

Funciones: un único acceso de usuarios; espacios personales y organizaciones; carpetas; subidas reanudables; permisos; papelera; restauración y descargas autenticadas. El servidor verifica los permisos en cada operación.

Límites de esta interfaz: subidas y descargas de hasta 64 MiB, un archivo simultáneo. Las descargas autenticadas se reúnen en memoria; los tokens no aparecen en enlaces. Las subidas usan WebCrypto para el hash completo. La búsqueda filtra lo cargado; permisos y miembros la primera página de 100. Los listados principales tienen paginación. Los metadatos de reanudación se guardan en sessionStorage cuando el navegador lo permite; algunos navegadores restringen ese almacenamiento en file://.

Verificación actual: node tests/frontend-client.cjs y node tests/frontend-http.cjs desde backend. Sin navegador ni capturas; la prueba HTTP requiere PostgreSQL aislado y el binario debug compilado. REHEARSAL.md contiene verificaciones históricas de la interfaz alojada, no una prueba visual del modo file://.

Iconos Lucide/Feather incluidos en SVG. Ver LUCIDE-LICENSE.txt. La apariencia sigue las referencias de stageshow con un acabado monocromo inspirado en iOS 18: modo claro/oscuro automático, tipografía del sistema y listas agrupadas. Sin dependencias de frontend.

El superusuario se gestiona desde el servidor. Para crear usuarios desde `backend/`, usa `.\servidor\santolvdrive.exe users create`. Consulta [los comandos de administración](../../backend/README.md#administración-y-acceso).


Diseño actual: aplicado directamente desde Drive.html proporcionado por el usuario, conservando su CSS original, sus colores y su distribución. Los datos y acciones de demostración se sustituyen por la API existente. La selección actúa sobre un elemento. Recientes, Destacados y Escanear quedan desactivados hasta que exista soporte real; las miniaturas abstractas son decorativas. Ver REHEARSAL.md para las comprobaciones sin capturas.

Subir archivos: botón Subir archivo o Nuevo → Subir archivo. Se admiten tipos personalizados y binarios, hasta 64 MiB por archivo en esta interfaz. La carpeta de cada archivo sigue el esquema `usuarios/nombre_usuario/archivos/Carpeta/nombre.ext/nombre.ext` y contiene un único contenido, sin copias ni historial. Las carpetas, los renombrados y los movimientos se reflejan en disco. El servidor conserva nombres, permisos y ubicaciones en PostgreSQL. Los temporales de operaciones en curso quedan fuera de los usuarios, en `data/.temporal`.

Nuevo abre un menú rectangular de ancho fijo. El clic derecho en archivos o carpetas muestra abrir, renombrar, mover, compartir y enviar a la papelera. También se abre con la tecla de menú contextual o Mayús+F10; se navega con flechas y se cierra con Escape. Mover permite elegir una carpeta por su nombre. En la papelera se puede restaurar o confirmar la eliminación definitiva; esta última retira los bytes en segundo plano y no tiene recuperación. Reemplazar un archivo no conserva la copia anterior.

Los nombres ocupados se rechazan sin sobrescritura. La interfaz avisa si hay una operación de disco pendiente. La aplicación continúa siendo un HTML independiente que llama a la API Rust.

La pantalla inicial es Recientes. El servidor guarda los últimos 200 archivos o carpetas utilizados por cada usuario; el historial persiste al cerrar sesión y vuelve a comprobar los permisos al mostrarlo. Mi unidad sigue disponible para explorar todos los archivos. Destacados se ha retirado.

El propietario puede eliminar un espacio compartido desde su cabecera o su menú contextual. Se solicita confirmación porque borra también archivos, carpetas y papelera para todos los miembros. El contenido se retira en segundo plano y no admite restauración. El espacio personal está protegido.

El clic derecho en un elemento, un espacio o el fondo abre el menú del Drive. También funciona con la tecla de menú contextual o Mayús+F10. Los campos de texto mantienen las opciones habituales de copiar y pegar.
