# Modelo de amenazas: editores ofimáticos y colaboración

Alcance: los cuatro editores ArtCraft alojados en la suite (fase 1) y el motor colaborativo previsto
(fases siguientes). Se revisará en cada fase.

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

## Amenazas previstas para la colaboración (fases siguientes)

- Secuestro de WebSocket entre sitios: validar `Origin`, cookie de sesión y CSRF en el
  establecimiento de la conexión.
- Suscripción a documentos ajenos: autorización por documento al unirse y en cada mensaje.
- Revocación de permisos: cierre inmediato de las conexiones afectadas.
- Actualizaciones CRDT válidas pero no autorizadas: validación semántica por editor antes de
  integrar; los lectores no pueden enviar actualizaciones.
- Agotamiento de recursos con actualizaciones CRDT: límites de tamaño por mensaje, por documento y
  por usuario, colas acotadas con contrapresión y tiempo máximo de inactividad.
- Duplicados y desorden: identificadores únicos de operación y reconocimiento tras persistir.

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
- Edición simultánea sin motor colaborativo: el segundo guardado recibe un conflicto.
