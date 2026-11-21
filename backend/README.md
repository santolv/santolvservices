# SantolvDrive: backend nativo en Rust

Axum + Tokio + Tower + Rustls. La aplicación y su instalador están escritos en Rust y se ejecutan como un único proceso nativo. **PostgreSQL es la única excepción externa autorizada.** No se necesita Docker, Caddy, Nginx, winget, scripts de instalación ni intérpretes para ejecutar el producto. No se añaden ni descargan otros programas.

## Instalar y arrancar

Distribuir el ejecutable correspondiente al sistema y arquitectura. Los paquetes Linux/macOS se generan con `.github/workflows/installers.yml`; el artefacto local Windows está en `dist/`. Los instaladores antiguos basados en contenedores quedan sustituidos.

```text
santolvdrive install --directory /ruta/santolvdrive --origin https://drive.tudominio.com:8443
# Windows
.\santolvdrive.exe install --directory C:\SantolvDrive --origin https://drive.tudominio.com:8443
# Inspección sin escribir configuración ni instalar programas
santolvdrive install --check
```

Antes de ejecutar el instalador, tener PostgreSQL disponible y un certificado/cadena y clave privada en formato PEM para el dominio. El instalador pide la URL de PostgreSQL de forma oculta, las rutas PEM y las credenciales del primer administrador. Crea un directorio privado, copia el certificado y la clave, escribe `server.json`, aplica las migraciones y copia el propio ejecutable. No compila código en el servidor, lanza gestores de paquetes ni instala servicios del sistema.

Arrancar el ejecutable instalado con `serve --config /ruta/server.json`. HTTPS termina directamente en Rustls; no se necesita proxy. El puerto lo controla `bind`: los puertos bajos como 443 pueden exigir permisos del sistema. El 8443 permite un despliegue sin privilegios de administrador del sistema si las reglas de red lo permiten. Ajustar el DNS y el firewall al puerto publicado.

La configuración no almacena la contraseña del administrador; PostgreSQL y las sesiones solo conservan sus verificadores. La URL SQL sí puede incluir una contraseña y debe mantenerse privada. El directorio se restringe al propietario en Unix y al propietario/SYSTEM mediante las APIs nativas de seguridad de Windows, sin invocar programas auxiliares. Reanudar una instalación conserva archivos existentes y rechaza reemplazos con distinto contenido. Actualizar un ejecutable requiere parar el proceso y un procedimiento explícito.

## Configuración y TLS

`server.example.json` es una plantilla sin credenciales reales. Las rutas relativas se resuelven desde el directorio de configuración. `--config` funciona con `serve`, `bootstrap-admin`, `reset-admin` y `admin-initialized`. Alternativamente se pueden exportar las variables de `.env.example`; el binario no interpreta `.env` automáticamente.

En producción se requiere un origen HTTPS y una cadena y clave PEM válidas y concordantes. Rustls admite TLS 1.2/1.3 y HTTP/1.1 con keep-alive. El certificado debe cubrir el dominio y ser confiable para los clientes. La emisión y renovación no son automáticas: sustituir los PEM de forma coordinada y reiniciar para cargarlos. No se instala software de certificados ni se omite la validación TLS. Los certificados autofirmados generados en las pruebas solo se confían en el cliente de pruebas.

El listener limita a 256 conexiones y 64 handshakes simultáneos; cada handshake tiene 10 segundos, las cabeceras HTTP tienen un plazo de 30 segundos y un buffer máximo de 32 KiB, y las conexiones sin progreso de E/S caducan en 60 segundos. La aplicación conserva límites de solicitudes, transferencias, contraseñas y consultas. Las cabeceras `X-Forwarded-For` no se usan como identidad: los límites por IP emplean el peer de la conexión aceptada.

HTTP sin TLS solo se permite explícitamente para desarrollo con `allow_insecure_localhost=true`, origen HTTP de loopback y bind de loopback. No hay fallback automático de HTTPS a HTTP si falla el certificado.

PostgreSQL debe usar un rol de aplicación sin permisos de superusuario, creación de roles ni bases. El rol necesita permisos sobre su propio esquema para las migraciones. Las conexiones remotas exigen `sslmode=verify-full`; las conexiones de loopback pueden usar el servidor local autorizado. El instalador no descarga PostgreSQL ni cambia sus roles: se reutiliza la instancia que indique el administrador. No se ha implementado todavía registro automático como servicio ni emisión ACME.

## Administración y acceso

El **administrador del servidor** es una cuenta del producto con tipo `admin`. Solo puede administrar usuarios y consultar el registro de administración. No obtiene espacio personal, no aparece como usuario de almacenamiento y todos los endpoints de archivos, organizaciones y cargas rechazan su sesión, incluso con permisos compartidos accidentalmente.

- `POST /api/v1/auth/login` con `{username,password}`. Web: cookie HttpOnly, Secure, SameSite=Strict y `csrf_token`; enviar `X-CSRF-Token` en escrituras. Móvil: añadir `mobile:true` y usar el token bearer en el almacén seguro del sistema. Sesiones normales de 7 días; administrador, 30 minutos.
- `GET /api/v1/me`: identidad, tipo de cuenta y si debe cambiar la contraseña.
- `POST /api/v1/admin/users` con `{username,password}`: crea un usuario y su espacio personal. La contraseña inicial es temporal.
- `GET /api/v1/admin/users?after=UUID`: lista cuentas sin contraseñas ni contenido.
- `PATCH /api/v1/admin/users/{id}` con `{disabled:true|false}`: desactiva/reactiva. Desactivar revoca sesiones y bloquea nuevas publicaciones; conserva archivos y membresías. Reactivar no resucita sesiones anteriores.
- `POST /api/v1/admin/users/{id}/password` con `{password}`: establece una contraseña temporal y revoca todas las sesiones. No reactiva una cuenta desactivada.
- `GET /api/v1/admin/audit?after=ID`: acciones administrativas, sin secretos ni contraseñas.
- El usuario entra con su contraseña temporal y solo puede consultar identidad, salir y usar `POST /api/v1/auth/password` con `{current_password,new_password}`. Tras cambiarla debe iniciar sesión de nuevo para usar archivos. Este endpoint también permite a un administrador cambiar su propia contraseña.

Nombres nuevos: 3–64 caracteres ASCII alfanuméricos, punto, guion y guion bajo, normalizados a minúsculas. Contraseñas de 12–1024 bytes, protegidas con Argon2id. No hay ninguna cuenta ni contraseña predeterminada. Un usuario normal no puede crear usuarios, ascender a administrador ni ejecutar acciones administrativas.

Para despliegues manuales, con las mismas variables del servidor configuradas:

```text
santolvdrive bootstrap-admin --config server.json
santolvdrive reset-admin --config server.json
```

Bootstrap solo funciona una vez y nunca promueve una cuenta existente. `reset-admin` exige acceso a la consola del servidor y sus credenciales de PostgreSQL: sirve para recuperar el administrador, revoca sus sesiones y queda auditado. Protege el acceso al sistema y a PostgreSQL: el rol administrativo separa operaciones dentro de la API, pero no es cifrado de extremo a extremo frente a quien controla el servidor o puede restablecer credenciales.

## Datos existentes y migración

Se conservan las migraciones SQL y el modelo de datos, cuentas, permisos, versiones y contenido. El cambio de despliegue no convierte ni elimina archivos. Para pasar desde una instalación anterior: detener el backend, hacer una copia coordinada de PostgreSQL y del almacenamiento, conservar sus credenciales/rol y configurar `storage_root` con la ubicación correcta de los datos recuperados. No usar una instalación nueva sobre directorios ajenos ni eliminar volúmenes antiguos antes de verificar la migración.

La migración `0002` de cuentas sigue conservando IDs y contraseñas y sustituyendo el identificador anterior por `username`; los clientes usan `{username,password}`. No hay emails, verificaciones ni recuperación pública. El administrador del servidor administra cuentas y no es un gestor de espacios.

Los roles de espacio son `1` lector, `2` editor y `3` gestor. Los miembros tienen acceso al espacio completo y los permisos por carpeta se suman y heredan; no hay denegaciones que resten acceso. Quitar un miembro conserva el contenido organizativo y elimina sus permisos directos, aunque otros permisos independientes pueden seguir otorgando acceso. No se expulsa ni degrada al propietario del espacio.

## Archivos, cargas y conflictos

Crear carpetas con `POST /spaces/{space}/nodes`. Listar con `GET .../nodes?parent=UUID&limit=100&after=UUID`; sin parent lista la raíz. `trash=true` lista los elementos marcados directamente como papelera. Las carpetas en papelera ocultan descendientes sin reescribirlos. Restaurar una carpeta conserva su estructura; para restaurar un hijo individual, su padre debe estar activo.

Los nombres se normalizan a Unicode NFC, aceptan hasta 255 bytes UTF-8 y rechazan separadores, controles, `.` y `..`. Se permiten duplicados en **todas** las operaciones: crear, mover, renombrar y restaurar. La identidad es el UUID, no el nombre. Una nueva versión debe especificar `node_id`; compartir nombre nunca reemplaza contenido implícitamente. No se permite mover entre espacios.

1. `POST /uploads`: `{space_id,parent_id?,node_id?,name,size,sha256}`. SHA-256 hexadecimal del archivo completo; `size` entre 0 y 1 TiB. Reserva la cuota lógica; devuelve `{id,offset,max_chunk_bytes,expires_in}`. Incluir `Idempotency-Key`. Para reemplazar un archivo, incluir `If-Match` con la revisión de sus metadatos.
2. `PATCH /uploads/{id}/content`: binario de 1 byte a 8 MiB con `Content-Length`, `Upload-Offset` y `X-Chunk-SHA256`. Bloques secuenciales; un único escritor por carga. Devuelve `{offset}` solo después de flush, fsync y commit del offset. Si se pierde la respuesta, consultar `GET /uploads/{id}` y continuar desde `received`. Un reenvío con offset anterior devuelve `409`, sin duplicar bytes.
3. `POST /uploads/{id}/complete`: encola de forma durable la comprobación de tamaño/hash completo y devuelve `202`. Consultar `GET /uploads/{id}` hasta `state=ready`; `result_node` es el ID del archivo. `completion_error` informa un rechazo o fallo reintentable. Repetir complete tras ready devuelve `200` con `{id,upload_id}` sin crear otra versión. El trabajador verifica los permisos de nuevo antes de publicar. El archivo vacío no necesita PATCH. La verificación corre fuera del tiempo máximo de una petición HTTP para permitir archivos grandes.
4. `DELETE /uploads/{id}` cancela una carga pendiente y libera reserva; repetir es seguro. Las cargas expiran tras 24 h. Un conflicto de revisión al finalizar conserva la carga pendiente hasta cancelarla o expirar: no sobrescribe la edición ajena.

`GET /nodes/{id}` devuelve ETag con revisión (`"1"`). En renombrado, movimiento, papelera, restauración y restauración de versión enviar `If-Match`. Falta de cabecera: `428`; versión obsoleta: `412`; ciclo de carpetas: `409`. Estas escrituras y la creación de organizaciones/carpetas/cargas requieren `Idempotency-Key` (1–128 bytes), válido 7 días. Repetir clave/payload devuelve el resultado original; cambiar payload con la misma clave devuelve `409`. Las operaciones de asignar/quitar permisos tienen semántica de estado final y son naturalmente repetibles.

`GET /nodes/{id}/versions` lista versiones; `POST /nodes/{id}/versions/{version}/restore` crea una versión nueva que referencia el contenido anterior sin copiar bytes. Las versiones se conservan hasta eliminar definitivamente el archivo. Los listados devuelven `items` y `next_cursor`, con orden UUID estable. Son vistas en vivo, no snapshots: una creación/movimiento concurrente puede requerir refrescar desde el inicio para ver todo.

`GET /nodes/{id}/content?version=UUID` descarga la versión elegida o actual, siempre como adjunto binario. Soporta un solo `Range` (incluido sufijo), `If-Range` y streaming de 64 KiB. El ETag de **contenido** es el SHA-256; para modificar usar la revisión ETag de **metadatos**. Cancelar la descarga deja de leer el archivo. Una revocación impide nuevas solicitudes; no recupera bytes ya entregados ni detiene una respuesta ya autorizada.

## Consistencia y mantenimiento

Los temporales y objetos viven en el mismo volumen persistente. Publicar hace rename y sincroniza archivo/directorios en Linux antes de confirmar en PostgreSQL. El estado `pending` nunca aparece en listados de archivos; solo las versiones `ready` son descargables. Si se pierde el commit tras publicar, el mismo upload ID permite al reconciliador completar la operación. Si el disco pierde contenido confirmado, se marca `missing` y se falla de forma cerrada: hace falta restaurar una copia, no inventar bytes.

El trabajador recupera publicaciones interrumpidas, expira cargas, reintenta borrados y detecta contenido ausente o truncado. La integridad SHA-256 se verifica en cada bloque y finalización. El mantenimiento de fondo comprueba existencia/tamaño; no hace lectura/hash periódico de todo el disco. Los temporales sin confirmar se truncan al offset confirmado en el siguiente reintento.

Papelera retiene 30 días por defecto (`TRASH_RETENTION_DAYS`, entre 1 y 36500) antes de permitir `POST /nodes/{id}/purge` (gestor y `If-Match`). Al confirmar purge, el árbol queda bloqueado para restauración/escritura; el trabajador elimina hojas y versiones por lotes. Los objetos sin referencias se recogen después de 24 horas de antigüedad; la cuota usada baja después del borrado físico confirmado. Las reservas no son una preasignación física: si el volumen se llena, la carga falla sin publicar contenido incompleto. Se mantiene un margen de 64 MiB, pero otros procesos también pueden consumirlo.

Las escrituras estructurales se serializan por espacio con advisory locks PostgreSQL para impedir ciclos y movimientos incompatibles. Las cargas se bloquean por sesión; la transferencia no bloquea todo el espacio. Los valores iniciales son 16 conexiones SQL, 128 solicitudes, 8 transferencias y 2 hashes Argon2id simultáneos por proceso. Son límites conservadores iniciales, no una capacidad garantizada. El cierre espera solicitudes HTTP activas; un corte forzado se recupera desde estado persistente.

## Copias, actualizaciones y operación

La garantía de durabilidad de producción requiere Linux y un sistema de archivos local que respete fsync y rename atómico, con `uploads/` y `objects/` en el mismo volumen. En Windows se pueden desarrollar y probar los flujos, pero no se sincronizan directorios y no se promete resistencia equivalente a pérdida de energía. Mantener el almacenamiento accesible solo por el usuario de servicio y no permitir escrituras de otros usuarios. Los archivos se entregan sin ejecución, extracción, inspección activa ni renderizado de HTML.

Para una copia consistente inicial: detener el backend/trabajador, crear `pg_dump` y copiar todo `STORAGE_ROOT` mientras sigue detenido, cifrar y enviar ambas partes a un destino independiente, y arrancar de nuevo. Guardar identificador de pareja y checksum de ambos backups. Para restaurar, parar el backend, restaurar PostgreSQL y el directorio de la **misma** copia en un entorno aislado, iniciar reconciliación y verificar cuentas, permisos, versiones y descargas antes de reabrir acceso. El reconciliador no sustituye una copia de seguridad.

Proteger las credenciales de PostgreSQL, la configuración y la clave privada TLS. El cifrado del volumen y backups corresponde al administrador; esta versión no incluye cifrado de extremo a extremo ni gestión propia de claves. No exponer PostgreSQL, métricas ni logs con datos sensibles en Internet.

Las migraciones están embebidas y SQLx verifica su historial. No modificar migraciones ya desplegadas. Antes de una nueva migración: copia consistente, ensayo sobre una restauración con datos y despliegue con mantenimiento. Una reversión de binario requiere compatibilidad con el esquema; nunca ejecutar automáticamente un downgrade destructivo.

## Comprobación

```text
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
# DATABASE_URL debe apuntar a PostgreSQL de pruebas con permiso CREATE DATABASE.
cargo test --locked
```

Los tests SQLx crean bases aisladas y aplican migraciones automáticamente. No usar credenciales de producción. El CI añade `cargo audit`. Las consultas usan parámetros enlazados y se prueban contra PostgreSQL; no se generan metadatos SQLx offline de comprobación de queries en compilación.

Para medir tu VPS: usar un build release, poblar carpetas e historiales crecientes y aumentar carga concurrente de listados y transferencias; registrar p50/p95/p99, MB/s, RAM, CPU, IOPS, esperas SQL, respuestas 429 y errores. El test de carga ignorado en `tests/load.rs` permite obtener una primera línea base de listados contra una BD de pruebas. No equivale a una prueba de red ni a una capacidad máxima certificada.

La primera entrega no incorpora S3, búsquedas de contenido, previews, enlaces públicos, comentarios ni clientes. Tampoco se ha auditado externamente. Para múltiples instancias, todas deben usar el mismo almacenamiento coherente y DB; los límites locales se multiplican por proceso y hay que volver a medirlos. El bloqueo por espacio prioriza consistencia y puede convertirse en el límite de escritura de una organización muy activa.
