# Santolv Services

Un servidor nativo en Rust para Inicio, Drive y Fotos, con PostgreSQL y una
sola cuenta. El código y la instalación están en `backend/`.

## Instalación local

Desde `backend/`, copia el instalador de tu plataforma a `dist/`. El flujo
**Single-file installers** de GitHub compila binarios para Windows x64 y
Linux x64/ARM64. Descarga el artefacto correspondiente y extrae su contenido.

**Windows x64**

```powershell
.\dist\santolvdrive-windows-x64.exe install --directory .\servidor
.\servidor\santolvdrive.exe serve
```

**Linux x64 o ARM64**

```bash
chmod +x ./dist/santolvdrive-linux-x64
./dist/santolvdrive-linux-x64 install --directory ./servidor
./servidor/santolvdrive serve
```

En ARM64 usa `santolvdrive-linux-arm64` en las dos primeras líneas.
Ejecuta el servidor e instalador con un usuario normal. El instalador reutiliza
PostgreSQL si está instalado. Si falta, Windows x64 descarga una copia portable
verificada. Linux con apt o dnf instala el paquete del sistema y puede pedir
permisos administrativos. En las demás distribuciones instala PostgreSQL con
su gestor y, si sus binarios no están en el PATH, exporta
`POSTGRES_BIN=/ruta/al/directorio/bin` antes de instalar.

Si partes del código fuente, ejecuta `cargo run --release --locked -- install --directory servidor`
desde `backend/` y usa el mismo comando de arranque de tu plataforma.

El instalador solicita las credenciales iniciales del administrador y prepara
`backend/servidor/` con el ejecutable, `server.json`, los archivos
(`data/`) y el clúster PostgreSQL (`postgres/`). Abre
[localhost:5500](http://localhost:5500/) en el mismo equipo. La API local
escucha en [localhost:8443](http://localhost:8443/). Los comandos se pueden
ejecutar desde cualquier carpeta usando el ejecutable instalado, que carga
`server.json` de su propia carpeta. `--config RUTA` elige otra configuración.

Para actualizar, detén el servidor, guarda una copia consistente de PostgreSQL
y del almacenamiento, y sustituye el ejecutable de `servidor/`. Reutiliza su
configuración, sus cuentas y sus datos. Consulta las [rutas y el esquema
modular](../README.md#almacenamiento).

Las sesiones normales duran dos horas y las iniciales pendientes de cambio de contraseña quince minutos. La cookie de sesión usa `HttpOnly`, `SameSite=Strict`, ruta `/` y `Secure` con HTTPS. El mismo servidor comprueba la cookie al servir las páginas protegidas y exige CSRF en las operaciones que cambian datos. `/api/v1/me` recupera el CSRF al recargar sin exponer el identificador de sesión. El cierre de sesión y el cambio o restablecimiento de contraseña revocan el acceso en el servidor. Consultar **INFORME_SEGURIDAD.md** para los requisitos antes de publicar mediante Pangolin.

Para una configuración anterior, detener el proceso y ejecutar con el binario nuevo:

```text
santolvdrive configure-local --config ./servidor/server.json
santolvdrive serve --config ./servidor/server.json
```

Se conserva una copia `server.before-local.json` y no se alteran cuentas ni archivos. Usar el archivo de configuración que realmente contenga la instalación anterior. El programa no desinstala programas ajenos ni elimina secretos de otros directorios.

## Dependencias y configuración

`install --check` informa sin escribir archivos. `install --directory ./servidor --dependencies-only` prepara solamente PostgreSQL, sin crear cuentas ni bases. Si ya hay binarios PostgreSQL, los reutiliza; `POSTGRES_BIN` permite indicar su directorio. Si falta: Windows x64 descarga EDB portable 17.12 por HTTPS con hash fijado, límites y extracción segura de runtime/licencias sin pgAdmin; Linux usa apt/dnf; macOS utiliza Homebrew si existe. Otros sistemas requieren PostgreSQL previamente instalado. Los gestores del sistema pueden solicitar contraseña administrativa y crear su servicio predeterminado; Drive mantiene su clúster independiente. En Unix ejecutar Drive como usuario sin privilegios root, como exige initdb.

`server.example.json` y `.env.example` son plantillas avanzadas. Las rutas relativas se resuelven desde el archivo de configuración. Las variables deben exportarse: el binario no interpreta `.env`. HTTP exige `allow_insecure_localhost=true`, origen de loopback y escucha de loopback; se rechazan `0.0.0.0`, `::` y direcciones de red. HTTPS local mediante Rustls sigue disponible con `allow_insecure_localhost=false` y certificado/clave válidos. No hay fallback a HTTP si falla TLS.

El origen opaco `null` y `local_file: true` se rechazan. Abrir el HTML con doble clic muestra la dirección HTTP de la interfaz; no inicia una sesión. La interfaz usa cookies HttpOnly y CSRF desde su origen explícito. Solo los clientes nativos sin Origin ni Fetch Metadata pueden solicitar bearer con `mobile: true`; su duración sigue las mismas reglas de sesión. HTTP solo se permite en loopback.

Los listeners conservan límites de 256 conexiones, cabeceras de 32 KiB con 30 segundos de plazo e inactividad de 60 segundos. PostgreSQL escucha en 127.0.0.1 con SCRAM-SHA-256 y rol limitado.

`santolvdrive serve` arranca la instancia PostgreSQL de esta instalación como
proceso hijo e imprime ambos PID. Drive, Fotos y el resto de módulos se ejecutan
dentro del proceso principal. Una instancia PostgreSQL externa se administra por
separado.

- Para un apagado ordenado, pulsa **Ctrl+C** en la consola de Santolv. En Unix,
  también puedes enviar `SIGTERM` al PID de Santolv.
- En Windows puedes finalizar el PID de Santolv desde **Detalles** del
  Administrador de tareas o ejecutar `taskkill /PID <PID de Santolv> /T /F`.
  Windows cierra los procesos PostgreSQL de esa instalación con Santolv.
  Una parada forzada puede requerir recuperación automática de PostgreSQL
  al volver a arrancar.
- Si vienes de una versión que dejaba PostgreSQL arrancado en segundo plano,
  detén primero el Santolv anterior. Si PostgreSQL sigue activo, ejecuta
  `santolvdrive database-stop --config ./servidor/server.json` antes de arrancar
  la nueva versión.

El directorio de instalación es privado para propietario/SYSTEM en Windows y propietario en Unix. `server.json`, sus backups y `postgres-state.json` contienen secretos y deben protegerse y respaldarse cifrados. Las migraciones son incrementales; la actualización conserva cuentas y contenido existente.

## Administración y acceso

Contactos (`contacts`) y Correo (`mail`) se instalan desactivados. Para habilitarlos,
ejecuta `santolvdrive services enable contacts` y `santolvdrive services enable mail`
con la configuración de la instalación. Cada servicio aparece en Inicio para las
cuentas autorizadas y se puede desactivar por separado. Correo guarda borradores
y organiza mensajes localmente. La conexión para enviar y recibir queda pendiente.

El **superusuario del servidor** es único y se conserva como identidad de administración y auditoría. No puede iniciar sesión por HTTP, no tiene unidad personal y no aparece en el HTML. La base de datos impide crear un segundo superusuario. La actualización revoca sus antiguas sesiones y conserva los usuarios, sus sesiones normales y todos los archivos. Las rutas /api/v1/admin/* se han retirado de la API y de OpenAPI.

La administración se realiza desde la terminal del propio servidor, con acceso del sistema operativo a su configuración privada. No es el usuario PostgreSQL y no se concede un rol superuser de PostgreSQL. Quien pueda leer server.json y ejecutar el programa con esa cuenta del sistema controla esta instalación.

Desde backend, usando la configuración existente:

```powershell
.\servidor\santolvdrive.exe users list
.\servidor\santolvdrive.exe users create
.\servidor\santolvdrive.exe users disable nombre
.\servidor\santolvdrive.exe users enable nombre
.\servidor\santolvdrive.exe users reset-password
.\servidor\santolvdrive.exe users audit
.\servidor\santolvdrive.exe users modules nombre
.\servidor\santolvdrive.exe users deny-module nombre photos
.\servidor\santolvdrive.exe users allow-module nombre photos
```

Create y reset-password piden el usuario y una contraseña temporal oculta, con repetición. Para automatización aceptan --stdin antes de --config y un objeto JSON {username,password} por la entrada estándar, con límite de 4096 bytes; nunca pasar contraseñas como argumentos. List y audit aceptan un cursor después del nombre de la acción, devuelto como next_cursor. Desactivar una cuenta revoca sus sesiones y bloquea las publicaciones pendientes, conservando sus archivos. Reactivarla no recupera las sesiones anteriores. Restablecer su contraseña exige cambiarla al entrar y no reactiva cuentas desactivadas.

El Drive solo tiene el acceso habitual de usuario y contraseña mediante POST /api/v1/auth/login. El HTML se abre desde el listener estático independiente y utiliza cookies HttpOnly con CSRF; los clientes nativos conservan bearer según las condiciones documentadas. Solo las cuentas de usuario pueden recibir sesiones. GET /api/v1/me muestra su identidad; POST /api/v1/auth/password cambia su propia contraseña y revoca las sesiones. Una contraseña temporal obliga a cambiarla antes de acceder a los archivos.

Nombres nuevos: 3–64 caracteres ASCII alfanuméricos, punto, guion y guion bajo, normalizados a minúsculas. Contraseñas de 12–1024 bytes, protegidas con Argon2id. No hay ninguna cuenta ni contraseña predeterminada. Un usuario normal no puede crear usuarios, ascender a administrador ni ejecutar acciones administrativas.

Para despliegues manuales, con las mismas variables del servidor configuradas:

```text
santolvdrive bootstrap-admin --config server.json
santolvdrive reset-admin --config server.json
```

Bootstrap solo funciona una vez y nunca promueve una cuenta existente. `reset-admin` exige acceso a la consola del servidor y sus credenciales de PostgreSQL: sirve para recuperar el administrador, revoca sus sesiones y queda auditado. Protege el acceso al sistema y a PostgreSQL: el rol administrativo separa operaciones dentro de la API, pero no es cifrado de extremo a extremo frente a quien controla el servidor o puede restablecer credenciales.

## Datos existentes y migración

Se conservan las migraciones SQL, cuentas, permisos y el contenido actual. La migración 0007 retira el historial anterior, como parte de la simplificación solicitada. Para adaptar una configuración anterior: detener el servidor y ejecutar `santolvdrive configure-local --config /ruta/server.json`. Conserva credenciales SQL, rutas y cuotas, guarda el original como `server.before-local.json` y sustituye únicamente la configuración de acceso por HTTP de loopback. No elimina ni convierte los datos. Reiniciar con el mismo archivo. Proteger también el backup, que contiene las credenciales SQL.

La migración `0002` de cuentas sigue conservando IDs y contraseñas y sustituyendo el identificador anterior por `username`; los clientes usan `{username,password}`. No hay emails, verificaciones ni recuperación pública. El administrador del servidor administra cuentas y no es un gestor de espacios.

Los roles de espacio son `1` lector, `2` editor y `3` gestor. Los miembros tienen acceso al espacio completo y los permisos por carpeta se suman y heredan; no hay denegaciones que resten acceso. Quitar un miembro conserva el contenido organizativo y elimina sus permisos directos, aunque otros permisos independientes pueden seguir otorgando acceso. No se expulsa ni degrada al propietario del espacio.

Compartir y gestionar miembros admite `username` o `user_id` como campos separados. Enviar exactamente uno: `{"username":"santi","role":1}` o `{"user_id":"UUID","role":1}`. El nombre se resuelve en el servidor y los permisos se guardan por el UUID estable; nunca se interpreta automáticamente un nombre como ID. Para permisos a espacios se usa `subject_space_id`, excluyente con ambos campos de usuario. El listado de permisos devuelve `username` y `user_id` por separado (y `space_name` para espacios). Solo un gestor puede consultar esa lista o resolver un destinatario al dar permisos. No se añade un directorio público de usuarios y no se puede compartir con el superusuario del servidor ni conceder acceso nuevo a cuentas deshabilitadas. Sí se pueden revocar sus permisos existentes.

El HTML inicia en Recientes. `POST /nodes/{id}/touch` registra una apertura autorizada y `GET /recent?after=...` devuelve el historial personal ordenado por uso, con cursor compuesto y límite de 200 referencias por usuario. Crear, modificar o descargar también registra actividad. Los permisos se verifican de nuevo al listar: los elementos en papelera, eliminados o cuyo acceso se ha revocado no se muestran. Se guarda en PostgreSQL, sin depender del almacenamiento del navegador, y empieza a registrar actividad desde esta actualización.

`DELETE /spaces/{space}` permite al propietario eliminar un espacio organizativo después de confirmarlo en el HTML. No se puede eliminar el espacio personal. Devuelve `202` vacío, retira inmediatamente el acceso y los miembros, cancela las cargas pendientes y elimina el contenido por lotes en segundo plano, reanudando tras un reinicio. Repetir la solicitud es seguro. El borrado incluye archivos, carpetas y papelera y no admite restauración. El directorio físico se retira al terminar; queda únicamente un registro mínimo del espacio para conservar la actividad registrada, sin conservar archivos. En el HTML se puede actuar desde la cabecera del espacio o con clic derecho en la barra lateral. El clic derecho del fondo también muestra acciones del Drive; los campos de texto mantienen las acciones de edición del navegador.

## Archivos, cargas y conflictos

Crear carpetas con `POST /spaces/{space}/nodes`. Listar con `GET .../nodes?parent=UUID&limit=100&after=UUID`; sin parent lista la raíz. `trash=true` lista los elementos marcados directamente como papelera. Las carpetas en papelera ocultan descendientes sin reescribirlos. Restaurar una carpeta conserva su estructura; para restaurar un hijo individual, su padre debe estar activo.

Los nombres se normalizan a Unicode NFC, aceptan hasta 255 bytes UTF-8 y rechazan separadores, controles, `.` y `..`. Se permiten duplicados en **todas** las operaciones: crear, mover, renombrar y restaurar. La identidad es el UUID, no el nombre. Una nueva versión debe especificar `node_id`; compartir nombre nunca reemplaza contenido implícitamente. No se permite mover entre espacios.

1. `POST /uploads`: `{space_id,parent_id?,node_id?,name,size,sha256}`. SHA-256 hexadecimal del archivo completo; `size` entre 0 y 1 TiB. Reserva la cuota lógica; devuelve `{id,offset,max_chunk_bytes,expires_in}`. Incluir `Idempotency-Key`. Para reemplazar un archivo, incluir `If-Match` con la revisión de sus metadatos.
2. `PATCH /uploads/{id}/content`: binario de 1 byte a 8 MiB con `Content-Length`, `Upload-Offset` y `X-Chunk-SHA256`. Bloques secuenciales; un único escritor por carga. Devuelve `{offset}` solo después de flush, fsync y commit del offset. Si se pierde la respuesta, consultar `GET /uploads/{id}` y continuar desde `received`. Un reenvío con offset anterior devuelve `409`, sin duplicar bytes.
3. `POST /uploads/{id}/complete`: encola de forma durable la comprobación de tamaño/hash completo y devuelve `202`. Consultar `GET /uploads/{id}` hasta `state=ready`; `result_node` es el ID del archivo. `completion_error` informa un rechazo o fallo reintentable. Repetir complete tras ready devuelve `200` con `{id,upload_id}` sin crear otra versión. El trabajador verifica los permisos de nuevo antes de publicar. El archivo vacío no necesita PATCH. La verificación corre fuera del tiempo máximo de una petición HTTP para permitir archivos grandes.
4. `DELETE /uploads/{id}` cancela una carga pendiente y libera reserva; repetir es seguro. Las cargas expiran tras 24 h. Un conflicto de revisión al finalizar conserva la carga pendiente hasta cancelarla o expirar: no sobrescribe la edición ajena.

`GET /nodes/{id}` devuelve ETag con revisión (`"1"`). En renombrado, movimiento, papelera, restauración y restauración de versión enviar `If-Match`. Falta de cabecera: `428`; versión obsoleta: `412`; ciclo de carpetas: `409`. Estas escrituras y la creación de organizaciones/carpetas/cargas requieren `Idempotency-Key` (1–128 bytes), válido 7 días. Repetir clave/payload devuelve el resultado original; cambiar payload con la misma clave devuelve `409`. Las operaciones de asignar/quitar permisos tienen semántica de estado final y son naturalmente repetibles.

No hay historial ni restauración de versiones. Reemplazar un archivo conserva únicamente el contenido nuevo; el ETag de metadatos sigue detectando escrituras simultáneas. Los listados devuelven `items` y `next_cursor`, con orden UUID estable. Son vistas en vivo: una creación o movimiento concurrente puede requerir refrescar desde el inicio.

`GET /nodes/{id}/content` descarga el contenido actual, siempre como adjunto binario. Soporta un solo `Range` (incluido sufijo), `If-Range` y streaming de 64 KiB. El ETag de **contenido** es el SHA-256; para modificar usar la revisión ETag de **metadatos**. Cancelar la descarga deja de leer el archivo. Una revocación impide nuevas solicitudes; no recupera bytes ya entregados ni detiene una respuesta ya autorizada.

## Consistencia y mantenimiento

Los temporales y objetos viven en el mismo volumen persistente. Publicar hace rename y sincroniza archivo/directorios en Linux antes de confirmar en PostgreSQL. El estado `pending` nunca aparece en listados de archivos; solo los contenidos `ready` son descargables. Si se pierde el commit tras publicar, el mismo upload ID permite al reconciliador completar la operación. Si el disco pierde contenido confirmado, se marca `missing` y se falla de forma cerrada: hace falta restaurar una copia, no inventar bytes.

El trabajador recupera publicaciones interrumpidas, expira cargas, reintenta borrados y detecta contenido ausente o truncado. La integridad SHA-256 se verifica en cada bloque y finalización. El mantenimiento de fondo comprueba existencia/tamaño; no hace lectura/hash periódico de todo el disco. Los temporales sin confirmar se truncan al offset confirmado en el siguiente reintento.

Mover a la papelera permite restaurar. Un gestor puede confirmar POST /nodes/{id}/purge con If-Match sin esperar: devuelve 202 vacío, oculta el elemento y encola el borrado físico por lotes. El contenido se elimina del disco y libera cuota al confirmar su retirada. Las referencias compartidas heredadas se conservan mientras sigan en uso. No se añade borrado automático: la papelera se vacía mediante la acción explícita del usuario. TRASH_RETENTION_DAYS se conserva para compatibilidad de configuración, pero no bloquea esa confirmación manual. Los contenidos huérfanos que no procedan de un borrado solicitado mantienen la recogida de seguridad tras 24 horas. Las reservas no preasignan capacidad física; no se publican archivos incompletos si el volumen se llena.

Las escrituras estructurales se serializan por espacio con advisory locks PostgreSQL para impedir ciclos y movimientos incompatibles. Las cargas se bloquean por sesión; la transferencia no bloquea todo el espacio. Los valores iniciales son 14 conexiones SQL generales y dos reservadas al journal, 128 solicitudes, ocho transferencias globales (dos por usuario) y dos hashes Argon2id simultáneos por proceso. Son límites conservadores iniciales, no una capacidad garantizada. El cierre espera solicitudes HTTP activas; un corte forzado se recupera desde estado persistente.

## Copias, actualizaciones y operación

La garantía de durabilidad de producción requiere Linux y un sistema de archivos local que respete fsync y rename atómico, con `.temporal/` y `archivos/` en el mismo volumen. En Windows se pueden desarrollar y probar los flujos, pero no se sincronizan directorios y no se promete resistencia equivalente a pérdida de energía. Mantener el almacenamiento accesible solo por el usuario de servicio y no permitir escrituras de otros usuarios. Los archivos se entregan sin ejecución, extracción, inspección activa ni renderizado de HTML.

Para una copia consistente inicial: detener el backend/trabajador, crear `pg_dump` y copiar todo `STORAGE_ROOT` mientras sigue detenido, cifrar y enviar ambas partes a un destino independiente, y arrancar de nuevo. Guardar identificador de pareja y checksum de ambos backups. Para restaurar, parar el backend, restaurar PostgreSQL y el directorio de la **misma** copia en un entorno aislado, iniciar reconciliación y verificar cuentas, permisos y descargas antes de reabrir acceso. El reconciliador no sustituye una copia de seguridad.

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

Para medir tu VPS: usar un build release, poblar carpetas y volúmenes de archivos crecientes y aumentar carga concurrente de listados y transferencias; registrar p50/p95/p99, MB/s, RAM, CPU, IOPS, esperas SQL, respuestas 429 y errores. El test de carga ignorado en `tests/load.rs` permite obtener una primera línea base de listados contra una BD de pruebas. No equivale a una prueba de red ni a una capacidad máxima certificada.

La primera entrega no incorpora S3, búsquedas de contenido, previews, enlaces públicos, comentarios ni clientes. Tampoco se ha auditado externamente. Para múltiples instancias, todas deben usar el mismo almacenamiento coherente y DB; los límites locales se multiplican por proceso y hay que volver a medirlos. El bloqueo por espacio prioriza consistencia y puede convertirse en el límite de escritura de una organización muy activa.

## Almacenamiento físico por usuario

```text
data/
  usuarios/nombre_usuario/
    archivos/
      Trabajo/
        informe.pdf/
          informe.pdf             # Único contenido del archivo
    papelera/                     # Elementos eliminados, por nombre
  .temporal/                      # Solo operaciones en curso del servidor
```

Cada usuario tiene una rama propia. Las organizaciones tienen una rama independiente en `organizaciones/<id>`. Los IDs de nodos y permisos permanecen en PostgreSQL; no hay marcadores `.drive-node` en las carpetas. Se conserva la carpeta por archivo solicitada anteriormente para posibles datos auxiliares. Mover o renombrar conserva el directorio completo.

No se generan copias de seguridad ni se conserva historial. Reemplazar un archivo retira el contenido anterior. Mover a la papelera permite restaurar; confirmar la eliminación definitiva retira los bytes mediante una tarea reanudable. Dos elementos con igual nombre en la papelera se distinguen con sufijos `(2)`, `(3)`, etc. Restaurar sobre un nombre ocupado devuelve un conflicto.

La migración 0007 retira referencias históricas y encola los nodos existentes. El trabajador adopta el archivo actual, comprueba su integridad antes de retirar una copia redundante y limpia únicamente los directorios internos antiguos que queden vacíos. Conserva las cuentas, permisos, carpetas y el estado de papelera. No ejecutar binarios antiguos después de migrar. La retirada del historial es irreversible.

PostgreSQL registra la ubicación de cada nodo y las operaciones pendientes. Un movimiento usa una intención persistente en la base de datos antes de cambiar el disco; después confirma la ubicación. `.temporal` contiene exclusivamente subidas incompletas, publicaciones pendientes y pasos de renombrado en curso. No es un historial y queda fuera de las carpetas de usuarios. El servidor necesita esta zona para rechazar contenido incompleto y reanudar una operación interrumpida.

La respuesta `storage_pending` indica trabajo pendiente en disco. Los cambios estructurales del espacio esperan a que se complete. Los destinos ocupados por contenido no registrado no se sobrescriben. Las rutas se validan, se rechazan enlaces simbólicos y los permisos del directorio de datos deben restringirse al proceso del servidor. Toda modificación de la estructura debe hacerse mediante la API.

La API entrega solo datos y contenidos, sin HTML. El frontend independiente se abre por `http://localhost:5500`; incluye menú Nuevo de ancho fijo, acciones con clic derecho y teclado, y selector de carpetas para mover. No se añaden dependencias ni programas: el backend sigue en Rust y PostgreSQL continúa siendo la excepción autorizada.
