# Modelo de amenazas: editores ofimáticos y colaboración

Alcance: los cuatro editores ArtCraft alojados en la suite (fase 1) y el servidor del motor
colaborativo (fase 2). Se revisará en cada fase.

## Activos

- Contenido de los documentos de cada usuario y espacio compartido.
- Integridad del historial de versiones de Drive.
- Sesiones (`drive_session`) y token CSRF.
- Disponibilidad del servidor (CPU, memoria, disco).

## Límites de confianza

| Límite | Lado de confianza | Lado no confiable |
|---|---|---|
| Navegador ↔ API (8443) | Backend Rust: identidad, permisos, revisiones | Todo lo que envía el cliente, incluido el editor WebAssembly |
| Navegador ↔ interfaz (5500) | Core: páginas y paquetes servidos | Peticiones, cabecera Host, rutas solicitadas |
| Archivo importado ↔ editor | — | Todo archivo es potencialmente malicioso |
| Código de terceros (ArtCraft) | Releases fijadas por commit | Su contenido se audita, no se presupone seguro |
| Pangolin ↔ backend | Autorización interna del backend | Pangolin no sustituye la autorización interna |

## Amenazas y medidas en la fase 1

| Amenaza | Medida |
|---|---|
| Acceso a archivos ajenos (IDOR) | El editor solo usa la API de Drive, que comprueba propietario o permisos del espacio en cada petición; el `node` de la URL no da acceso por sí mismo. |
| Escritura sin permiso de edición | Subidas sobre un nodo exigen permiso de escritura (`files::access(..., 2)`) en el servidor. |
| Sobrescritura de versiones más nuevas / carreras | `If-Match` con la revisión; conflicto en lugar de sobrescribir. Subidas idempotentes con `Idempotency-Key`. |
| CSRF | Operaciones de escritura con `X-CSRF-Token`; cookie `SameSite=Strict`. |
| XSS / ejecución de código | CSP con nonce. Solo las páginas de los editores añaden `'self' 'wasm-unsafe-eval'`; el resto de la suite no permite WebAssembly. Los paquetes se sirven con `nosniff`, tipo MIME fijo y solo extensiones permitidas. |
| Path traversal en `/apps/` | Editor de una lista cerrada, segmentos `[A-Za-z0-9._-]`, sin puntos iniciales, máximo 4 niveles, sin enlaces simbólicos (`symlink_metadata`). |
| Acceso a paquetes sin sesión | `/apps/` exige sesión y acceso al servicio del editor. |
| Archivos enormes / agotamiento de memoria | Límite de 512 MiB al abrir (host y editor) y cuotas de Drive al guardar. |
| Macros y contenido activo | Los editores ArtCraft no ejecutan macros VBA (WordCraft conserva sus partes sin ejecutarlas). Pendiente de verificar en las cuatro releases. |
| Recursos externos | CSP `connect-src` limitado a la API y al propio origen; `img-src` sin orígenes externos. |
| Suplantación de origen | Core comprueba la cabecera Host; la API valida orígenes en CORS. |
| Cadena de suministro | Releases fijadas por etiqueta y commit; compilación `--locked`; sin `trunk` ni `wasm-opt`. Auditoría de dependencias de ArtCraft: pendiente (ver riesgos). |

## Amenazas y medidas del motor colaborativo

Servidor en `backend/src/collaboration/`; diseño y límites en
[collaboration/README.md](../collaboration/README.md). Cada medida tiene una prueba en
`backend/tests/collaboration.rs` o en las pruebas unitarias de `protocol`.

| Amenaza | Medida |
|---|---|
| Secuestro de WebSocket entre sitios (CSWSH) | `Origin` debe ser exactamente el frontend de la suite. Solo se acepta la cookie `drive_session` (`SameSite=Strict`); `Authorization` se rechaza. El middleware común sigue comprobando Host, proxys y `Sec-Fetch-Site`. |
| Unirse a documentos ajenos (IDOR) | Permiso de Drive sobre el archivo (`files::access`) antes de aceptar la conexión. Las carpetas, los archivos en la papelera y la falta de permiso responden 404. El rol viene del servidor, nunca del cliente. |
| Escritura sin permiso | Los lectores reciben el documento, pero sus actualizaciones se rechazan (`read_only`). El rol se comprueba en cada actualización. |
| Revocación y fin de sesión | Cada 5 s se reverifican la sesión (cierre de sesión, caducidad, cuenta desactivada o con cambio de contraseña) y el permiso. Al perderse, se cierra la conexión con `session_ended` o `access_revoked`. |
| Suplantación en la presencia | El servidor fija `user`, `name`, `color` y `connection`; el cliente solo aporta su estado (JSON de 2 KiB como máximo). |
| Mensajes hostiles | Decodificación con límites, mensaje de 256 KiB como máximo y solo binario. Tras 5 errores (mensajes mal formados o actualizaciones Yrs inválidas) se cierra la conexión. |
| Agotamiento de recursos | 100 actualizaciones/s por conexión, presencia 20/s, pings 2/s. Documento de 32 MiB como máximo y 512 MiB entre todas las salas. 16 conexiones por cuenta, 2000 en total y 500 por documento. Intentos de conexión limitados por cuenta. Colas acotadas: escritura 1024 (`busy`) y salida 1024 (`too_slow`). Inactividad de 60 s. |
| Agotamiento del pool de la base | Un escritor por sala con confirmación en grupo: cada documento usa como mucho una conexión para escribir. |
| Duplicados, desorden y pérdida del ack | Id único de operación por documento (`UNIQUE(node_id, op_id)`); un reintento exacto recibe la secuencia original. Yrs integra el desorden. El ack solo se envía tras confirmar la escritura. |
| Id de operación reutilizado con otro contenido | Se guarda y se reparte con un id nuevo del servidor; memoria, registro y miembros no divergen. |
| Fallo de la base durante una escritura | La sala se invalida (`room_reset`), sus miembros se reconectan desde lo guardado y una sala invalidada no se compacta. |
| Varias salas para el mismo documento | El registro garantiza una única sala abierta por documento, incluso con carreras entre entrar y salir. |

## Comprobaciones realizadas en producción (28/11/2026)

- Apertura desde Drive (doble clic) de un PDF real en PdfCraft; creación, guardado y nuevo
  guardado de un `.docx` en WordCraft (revisiones 1 → 2 → 3) y reapertura con el contenido íntegro.
- Subida sobre el nodo con una revisión antigua: `412 revision_changed`, sin sobrescribir.
- `/apps/` sin sesión: 401 o 404. Con cabecera Host ajena: 403.
- `/apps/` con sesión: solo se sirve el paquete legítimo; `..%2F`, `%2e%2e`, `..%5C`, editor
  inexistente, extensiones no permitidas, archivos con punto inicial, más de 4 niveles y nombres de
  la instalación (`server.json`, ejecutable) responden 404.

## Riesgos residuales conocidos

- WordCraft ofrece descargar fuentes que faltan (por ejemplo Aptos) desde internet. La CSP bloquea
  esas conexiones externas; hay que proporcionar fuentes libres locales.

- Las releases de ArtCraft son alfa y su código (cientos de miles de líneas) no se ha auditado
  manualmente todavía; un fallo en sus lectores de formatos podría bloquear o hacer fallar la
  pestaña del navegador. El impacto queda limitado al navegador del usuario: el servidor no procesa
  esos formatos en esta fase.
- `cargo audit` de los proyectos ArtCraft: pendiente de ejecutar e incluir aquí.
- Edición simultánea en los editores: hasta que existan los adaptadores, el segundo guardado recibe
  un conflicto.
- `cargo audit` del backend: `smallstr` 0.3.1 está marcado como **sin mantenimiento**
  (RUSTSEC-2026-0215). No es una vulnerabilidad conocida y no tiene versión corregida. Es una
  dependencia obligatoria de `yrs` 0.28.2, la última versión, y no hay función que permita
  quitarla. Con `--deny warnings` el CI falla. Aceptar la excepción o sustituir la dependencia lo
  decide el propietario.
- Colaboración:
  - **Revocación:** se aplica en 5 s como máximo, no al instante.
  - **Contenido arbitrario:** sin validación semántica por editor, un usuario con permiso de edición
    puede escribir en el documento Yrs cualquier estructura dentro de los límites. La validará el
    adaptador de cada editor.
  - **Invalidación de la sala:** un editor que envíe más de 256 actualizaciones con dependencias que
    nunca llegan provoca `document_inconsistent`, y la sala se reconstruye. Es una interrupción
    temporal que solo puede causar alguien con permiso de edición.
  - **Cuota:** el almacenamiento colaborativo, de 32 MiB por archivo como máximo, no cuenta todavía
    en la cuota de Drive.
  - **Memoria:** el límite de 512 MiB mide el tamaño codificado, no la memoria real de Yrs, que es
    mayor.
  - **Varias instancias:** el registro de salas es del proceso, lo que basta para la única
    instalación de producción, pero no para varias instancias.
