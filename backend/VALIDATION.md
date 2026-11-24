# Integración central — 24 de noviembre de 2026

Backend trasladado a la raíz del repositorio sin duplicar su motor. Comprobaciones
de esta entrega, realizadas sobre PostgreSQL temporal independiente del servidor
instalado:

- `cargo fmt --check` y `cargo clippy --locked --all-targets -- -D warnings`: pasan.
- `cargo test --locked -- --test-threads=2`: 52 pruebas pasan; tres pruebas de
  instalación y una de carga permanecen ignoradas por su configuración original.
- Clientes de Drive y Fotos en Node: pasan credenciales, CSRF, revisiones,
  transferencias binarias y actualización del estado tras confirmación.
- Ensayo HTTP con Drive individual y con Inicio/Drive/Fotos: ambos pasan.
- Navegador: acceso común, selector sin herramientas de los servicios, creación
  de álbum, subida de muestra, visor, favorito tras recargar, papelera,
  restauración y paso a Drive con la misma sesión; sin errores de consola.
- Migración ensayada con nombres reales: conservación de bytes, reanudación tras
  cambio de directorio y rechazo de colisiones sin sobrescritura.

No se ha migrado ni reiniciado la instalación del usuario. Las pruebas no incluyen
dispositivos móviles reales, miniaturas derivadas, EXIF ni vídeo. Los informes
inferiores corresponden a verificaciones anteriores de Drive.

---

# Validación del backend local — 21 de noviembre de 2026

La aplicación funciona por defecto en http://127.0.0.1:8443, únicamente desde el propio equipo. La instalación solo prepara PostgreSQL y la cuenta administrativa local. Se retiraron los módulos, comandos, campos de configuración, credenciales requeridas, supervisión y descargas de conectores externos. El descargador solo admite el host oficial EDB para el PostgreSQL portable. No cambió el esquema SQL ni se eliminaron datos.

## Comprobaciones

- Compilación, Clippy y formato del código.
- Prueba HTTP real: raíz accesible, login con credenciales Drive, cookie HttpOnly local y token CSRF; origen ajeno rechazado y datos protegidos sin sesión devuelven 401.
- Las pruebas existentes conservan separación del administrador, permisos, cuotas, idempotencia, subidas reanudables, conflictos, papelera, versiones, reconciliación y migración SQL con datos existentes.
- Configuración: se rechazan HTTP fuera de loopback y bind IPv4/IPv6 abierto. HTTPS Rustls local opcional mantiene sus pruebas de certificados y handshake.
- La conversión configure-local conserva URL SQL, rutas, cuentas y contenido; guarda el JSON original en un backup privado y elimina campos obsoletos del archivo activo. Repetirla conserva el backup.
- Instalación automática en clúster temporal: configuración HTTP local, migraciones, parada/reinicio y configuración persistente estable.
- La extracción portable PostgreSQL conserva runtime/licencias, rechaza traversal y no incluye pgAdmin. Descargas verificadas por SHA-256, límites de tamaño y HTTPS; las copias alteradas no se sustituyen ni ejecutan.

El ejecutable Windows se entrega en dist con CRT estático y checksum SHA-256. install --check y --help no requieren cuenta ni dominio. El único programa externo del producto es PostgreSQL. Se conserva ZIP 9.8.1, que pasó la revisión RustSec en la entrega anterior; esta retirada no cambia Cargo.lock.

## Límites

El frontend es ahora un archivo independiente servido desde localhost:5500; consulta la API en localhost:8443. La raíz de la API solo informa de su estado. Se verificaron en Chrome el acceso, cuentas, cambio de contraseña, subida con interrupción/reanudación, descarga con integridad, permisos, historial y papelera. Capturas a 1440 y 390 píxeles sin desbordamiento horizontal. El servidor estático lee el HTML al solicitarlo, con nonce CSP por respuesta y permiso de conexión únicamente a la API local. CORS admite el origen exacto del frontend y conserva autenticación y CSRF. La interfaz limita a 64 MiB por archivo. No se certifican otros navegadores, dispositivos físicos ni todos los diálogos. No se ha registrado arranque automático al reiniciar el equipo. El HTTP sin cifrar está limitado a loopback: no se permite acceder desde LAN/Internet. No se simularon cortes eléctricos físicos ni disco físicamente lleno; la durabilidad requiere comprobar fsync/rename en el sistema de archivos de producción. Falta validar instaladores Linux/macOS y el flujo interactivo completo en el servidor definitivo. No se eliminan programas ni configuraciones ajenas situados fuera del workspace.

Actualización de frontend independiente: verificaciones no visuales con pruebas Rust, JavaScript sin DOM real y peticiones HTTP a ambos procesos locales. Se comprueban preflight CORS, cookie de sesión, CSRF, rechazo de orígenes externos/null/otros puertos y cierre de sesión. No se abrió ningún navegador ni se hicieron capturas en esta actualización. El comportamiento de cookies entre puertos en dispositivos/navegadores reales no se volvió a verificar.

Actualización file://: API sin servidor HTML; login local_file devuelve bearer en memoria durante 30 minutos, sin cookies; pruebas JavaScript y HTTP sin navegador ni capturas. Las comprobaciones visuales históricas no validan este modo.

Administración desde servidor: rutas HTTP administrativas eliminadas; login y sesiones solo para usuarios; migración 0003 conserva archivos y sesiones de usuario y revoca las del administrador, con índice único para un solo superusuario. Comandos users list/create/disable/enable/reset-password/audit comprobados con una base aislada. Rediseño HTML de Drive sin navegador ni capturas.

Almacenamiento por cuenta: 32 pruebas Rust, aislamiento físico usuarios/alice y usuarios/bob, binarios personalizados, editor compartido en el espacio propietario, migración de objetos y temporales, reanudación y recuperación, protección contra rutas y colisiones. Prueba HTTP de subida real y descarga con igualdad de bytes en una base aislada. Sin pruebas visuales ni capturas. No se han instalado programas nuevos.

Corrección de nombres y eliminación: archivos publicados con nombre/extensión originales y cabecera UTF-8; migración de los UUID anteriores; HTML acepta 202 vacío; papelera/restauración/borrado definitivo sin espera mínima; borrado físico y cuota verificados por HTTP. Se conserva contenido referenciado por otra versión. Sin navegador ni capturas.

Resultado final de esta corrección: 34 pruebas Rust superadas (22 unitarias y 12 de integración), Clippy y formato correctos. Pruebas HTTP de nombre original Unicode, igualdad de bytes, papelera, restauración y eliminación definitiva con desaparición física y cuota cero. Prueba de referencias compartidas: una versión que aún usa el contenido impide retirarlo hasta eliminar la última referencia. Contrato JavaScript de confirmación con respuesta 202 vacía superado. No se introduce borrado automático de la papelera.

Verificación de la instalación: se encontraron tres registros cuyos contenidos no estaban presentes en la carpeta configurada. Se localizaron sus originales en Descargas y se restauraron únicamente tras coincidencia exacta de tamaño y SHA-256, mediante copia temporal sincronizada y publicación sin sobrescritura. Se mantuvieron IDs, versiones, permisos y cuotas. La reparación no modifica los originales de Descargas.

Árbol con nombres: 36 pruebas Rust y pruebas HTTP superadas. Carpetas anidadas, archivo/nombre.ext, renombrado, movimiento, copia independiente, recuperación de interrupciones y destinos ocupados sin sobrescritura. Archivos auxiliares preservados al mover o renombrar. Caso de mayúsculas y rutas largas verificados en Windows. Las cuotas lógicas no incluyen el espacio físico extra de la copia de trabajo. Sin navegador ni capturas.

Instalación actual: se recuperó un contenido ausente desde su original local únicamente tras coincidencia exacta de tamaño y SHA-256, manteniendo su estado de papelera. La migración crea la carpeta de usuario y sus nodos existentes con nombres reales. El diseño HTML no se modifica. Las nuevas comprobaciones de movimientos incluyen actualizaciones y restauración de versiones conservando archivos auxiliares.
