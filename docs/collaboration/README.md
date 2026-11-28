# Motor colaborativo (Yrs)

Estado: **servidor implementado y probado** (`backend/src/collaboration/`). Pendiente: biblioteca
cliente del navegador y adaptadores de los editores ArtCraft (ver «Pendiente»).

## Arquitectura

```
navegador (editor + adaptador)                      backend Rust (puerto 8443, sin puertos nuevos)
  │  WebSocket  GET /api/v1/collab/{node}            ┌──────────────────────────────────────────┐
  └──────────────────────────────────────────────────► transport   Origin, cookie, Drive, límites │
                                                     │ permissions rol desde files::access      │
                                                     │ room        Yrs en memoria, una sala/doc │
                                                     │   └ writer  confirmación en grupo (batch)│
                                                     │ persistence registro + instantáneas (PG) │
                                                     └──────────────────────────────────────────┘
```

| Módulo | Responsabilidad |
|---|---|
| `protocol` | Mensajes binarios versionados (v1), decodificados con límites explícitos. |
| `transport` | Ruta WebSocket, comprobaciones previas, bucle de sesión, límites por conexión, reverificación periódica. |
| `permissions` | Rol (lector, editor, administrador) desde el permiso de Drive del archivo; sesión viva; nombre mostrado. |
| `room` | Una sala por documento abierto: documento Yrs, miembros, canal de difusión, escritor único, compactación, presupuestos de memoria. |
| `persistence` | Registro de actualizaciones (`collaboration.updates`) e instantáneas (`collaboration.snapshots`), migración `0027`. |

El motor no conoce los formatos de los editores: guarda y reparte actualizaciones Yrs. Cada editor
tendrá un adaptador que traduce sus cambios a Yrs y aplica los remotos.

## Protocolo v1

Mensajes WebSocket **binarios** (los de texto cierran la conexión): un byte de tipo y su contenido.

| Tipo | Sentido | Contenido |
|---|---|---|
| `0x01` Hello | cliente → servidor | versión (`1`) y vector de estado Yrs del cliente (≤ 64 KiB) |
| `0x02` Update | cliente → servidor | id de operación (UUID, 16 bytes, no nulo) y actualización Yrs v1 |
| `0x03` Awareness | cliente → servidor | objeto JSON (cursor, selección…), ≤ 2 KiB |
| `0x04` Ping | cliente → servidor | vacío |
| `0x81` Welcome | servidor → cliente | rol (1, 2, 3), id de conexión y lo que le falta al cliente |
| `0x82` Updates | servidor → cliente | número (u16), ids de operación y una actualización Yrs que las fusiona |
| `0x83` Ack | servidor → cliente | id de operación y secuencia persistida (i64) |
| `0x84` Awareness | servidor → cliente | JSON con la identidad puesta por el servidor: `connection`, `user`, `name`, `color`, `state` (o `left`) |
| `0x85` Pong | servidor → cliente | vacío |
| `0x86` Error | servidor → cliente | código ASCII (`read_only`, `rate_limited`, `busy`, `access_revoked`…) |

Flujo: el cliente envía Hello en 10 s y recibe Welcome con lo que le falta. Después envía sus
cambios con un id único por operación. Cuando el lote que contiene su cambio queda confirmado en
PostgreSQL, el servidor reparte un `0x82` a los demás miembros y le envía a él el `0x83`.

- **Reintentos:** un cliente que no recibió su `0x83` reenvía la misma operación con el mismo id.
  El servidor la reconoce y responde con la secuencia original, sin guardarla ni repartirla otra vez.
- **Sin conexión:** el cliente acumula cambios locales. Al reconectar envía Hello con su vector de
  estado, recibe lo que le falta y sube sus cambios pendientes.
- **Desorden:** Yrs integra actualizaciones que llegan antes que sus dependencias.

## Garantías

- **Confirmación tras persistir:** un `0x83` significa que la fila está confirmada en PostgreSQL.
  El reparto a los demás miembros también ocurre después de confirmar.
- **Convergencia:** todas las réplicas convergen (CRDT Yrs). Hay pruebas de inserciones simultáneas
  en la misma posición y de un borrado que compite con una inserción dentro del rango borrado.
- **Recuperación:** al reabrir una sala se carga la última instantánea más las actualizaciones
  posteriores.
- **Compactación:** cada 500 actualizaciones o 4 MiB, y al salir el último miembro. Es atómica
  (instantánea y borrado del registro en una transacción). El estado se codifica bajo el cerrojo de
  la sala y se escribe sin él.
- **Id reutilizado con otro contenido:** si llega un contenido distinto con un id conocido (por
  ejemplo, un miembro que reutiliza el id de otro), el servidor ya lo ha integrado en memoria. Por
  eso lo guarda y lo reparte con un id nuevo elegido por el servidor. Así memoria, registro y
  miembros nunca divergen.
- **Fallos de escritura:** si falla la escritura en la base, la sala se invalida: sus miembros se
  desconectan (`room_reset`) y la sala se reconstruye desde lo guardado. Una sala invalidada nunca
  se compacta.

## Seguridad

Antes de aceptar la conexión:
- **Origen:** `Origin` debe ser exactamente el frontend de la suite, contra el secuestro de
  WebSocket entre sitios (CSWSH).
- **Sesión:** solo se acepta la cookie `drive_session`. Una cabecera `Authorization` se rechaza.
- **Cuenta:** debe ser de tipo usuario y sin cambio de contraseña pendiente.
- **Acceso:** se exige acceso al servicio Drive y permiso de Drive sobre ese archivo. Un archivo en
  la papelera, una carpeta o la falta de permiso responden 404, sin revelar si el archivo existe.
- **Middleware común:** sigue aplicándose (Host, proxys, origen, `Sec-Fetch-Site`).

Durante la sesión:
- **Permisos:** los lectores reciben el documento, pero sus actualizaciones se rechazan
  (`read_only`).
- **Identidad:** el servidor fija la identidad de la presencia; el cliente solo envía su estado.
- **Reverificación:** cada 5 s se comprueban la sesión (cierre de sesión, caducidad, cuenta
  desactivada) y el rol, que puede cambiar en caliente o perderse (`access_revoked`).
- **Errores de protocolo:** 5 mensajes mal formados o actualizaciones Yrs inválidas cierran la
  conexión.

Límites (constantes en el código):

| Recurso | Límite |
|---|---|
| Mensaje WebSocket | 256 KiB |
| Documento (instantánea + registro) | 32 MiB |
| Memoria total de salas abiertas (codificada) | 512 MiB, luego `server_busy` |
| Actualizaciones por conexión | 100/s, ráfaga 300 |
| Presencia por conexión | 20/s, ráfaga 40; pings 2/s, ráfaga 10 |
| Intentos de conexión por cuenta | ráfaga 30, luego 1/s (`429 too_many_attempts`) |
| Conexiones | 16 por cuenta, 2000 en total, 500 por documento |
| Cola de escritura por sala | 1024 actualizaciones (`busy`); lotes de 64 |
| Cola de salida por conexión | 1024 mensajes; quien no lee a tiempo se desconecta (`too_slow`) |
| Inactividad | 60 s sin mensajes |

Detalle en [el modelo de amenazas](../security/office-threat-model.md).

## Rendimiento medido (28/11/2026)

Condiciones de la medición:
- **Ejecución:** prueba `load_same_document_and_separate_documents` de
  `backend/tests/collaboration.rs`, compilada en release, en un equipo de 12 núcleos con Windows 11 y
  el PostgreSQL local de pruebas.
- **Clientes:** WebSocket reales, cada uno con su propia cuenta y permiso de edición. Cada miembro
  hace 40 ediciones, una cada 50 ms (unas 20 por segundo y usuario, más que una persona escribiendo).
- **Arranque:** todos se conectan antes de empezar.
- **Mismo proceso:** servidor y clientes corren juntos, así que la CPU y la memoria medidas incluyen
  ambos.
- **Latencias:** confirmación = del envío al `0x83`; propagación = del envío a la recepción por otro
  miembro.

Latencias en milisegundos, como p50 / p95 / p99. La propagación solo se mide con un documento
compartido (con documentos separados no hay otros miembros a los que repartir).

| Usuarios | Documento | Aceptadas | Ops/s | Confirmación | Propagación | Recibido | Errores | Desconexiones |
|---|---|---|---|---|---|---|---|---|
| 2 | compartido | 80/80 | 32,8 | 0,91 / 2,46 / 7,68 | 0,90 / 2,45 / 7,67 | 8,4 KiB | 0 | 0 |
| 2 | separados | 80/80 | 32,5 | 0,93 / 1,76 / 2,68 | — | 2,0 KiB | 0 | 0 |
| 10 | compartido | 400/400 | 163,3 | 1,56 / 2,39 / 2,61 | 1,57 / 2,41 / 2,59 | 165,9 KiB | 0 | 0 |
| 10 | separados | 400/400 | 163,4 | 1,75 / 2,43 / 74,69 | — | 9,8 KiB | 0 | 0 |
| 25 | compartido | 1000/1000 | 411,5 | 3,11 / 4,70 / 5,08 | 3,12 / 4,73 / 5,10 | 996 KiB | 0 | 0 |
| 25 | separados | 1000/1000 | 407,1 | 3,42 / 4,96 / 5,81 | — | 24,4 KiB | 0 | 0 |
| 50 | compartido | 2000/2000 | 819,2 | 6,09 / 8,07 / 8,90 | 6,09 / 8,10 / 8,97 | 3,8 MiB | 0 | 0 |
| 50 | separados | 2000/2000 | 809,5 | 6,05 / 9,25 / 10,03 | — | 48,8 KiB | 0 | 0 |
| 100 | compartido | 4000/4000 | 1693,2 | 14,38 / 21,42 / 23,74 | 14,39 / 21,45 / 23,92 | 15,2 MiB | 0 | 0 |
| 100 | separados | 4000/4000 | 1600,5 | 11,80 / 18,36 / 21,31 | — | 97,7 KiB | 0 | 0 |

- **Recursos del proceso de prueba** (servidor y 100 clientes juntos, muestreo cada 0,5 s): memoria
  de trabajo de 56,3 MiB como máximo y memoria privada de 45,7 MiB como máximo. La CPU fue de 12,0 s
  en 40,7 s de ejecución, unos 0,3 núcleos de media sobre 12.
- **Ancho de banda** (recibido por los clientes, sin contar las cabeceras WebSocket): con 100
  usuarios en un mismo documento fueron 15,2 MiB para 396 000 entregas más 4000 confirmaciones, unos
  40 bytes por actualización entregada.
- **Ops/s:** son las operaciones aceptadas por segundo de edición (todos los miembros juntos).

### Cómo se llegó aquí

Las primeras mediciones de 100 usuarios en un mismo documento dieron 3447 de 4000 operaciones
aceptadas, desconexiones `too_slow` y una P95 de propagación de 599 ms. Se corrigió en cuatro pasos:

1. **Reparto independiente:** el reenvío a cada miembro pasó a una tarea propia, porque una conexión
   que esperaba su turno para guardar dejaba de vaciar su cola. Resultado: ninguna desconexión, pero
   P95 de unos 4 s, porque guardar de una en una limitaba a unas 650 ops/s.
2. **Confirmación en grupo:** un escritor por sala guarda lotes con un único `INSERT … unnest`.
   Resultado: P95 de 228–685 ms. Además, cada documento usa como mucho una conexión del pool para
   escribir, y no deja sin conexiones al resto de la suite.
3. **Compactación fuera del cerrojo:** la escritura de la instantánea ya no bloquea a los editores.
4. **Un mensaje por lote:** se reparte un mensaje por lote (actualizaciones fusionadas con
   `merge_updates_v1`), compartido sin copias (`Bytes`). Resultado: P95 de 21–41 ms en tres
   ejecuciones.

## Pendiente

1. **Biblioteca cliente** (`frontend/shared/`): conexión, reintentos con el mismo id, cola sin
   conexión, presencia y agrupación de pulsaciones.
2. **Adaptador de WordCraft**, y después GridCraft, DeckCraft y PdfCraft. Ninguno de los editores
   tiene edición colaborativa: en WordCraft (unas 333 000 líneas de Rust) cada acción pasa por
   `Session::run` y el deshacer usa instantáneas del árbol, sin un registro de operaciones
   transmisible. Hace falta un punto de enganche por editor: detectar los bloques cambiados
   (comparando punteros `Arc`), reflejarlos en Yrs (`Y.Text` por párrafo con formato, y los demás
   bloques con identidad estable) y aplicar los cambios remotos con las primitivas del propio modelo.
   - **GridCraft:** mapa de celdas por identificador estable de fila y columna.
   - **DeckCraft:** lista de diapositivas y mapa de objetos.
   - **PdfCraft:** anotaciones y operaciones de página.
3. **Validación semántica por editor** de lo que entra en el documento Yrs (ver riesgos).
4. **Guardado:** relación entre el documento Yrs y el archivo de Drive (`.docx`…), es decir, quién
   guarda la revisión y cuándo.
5. **Revocación instantánea:** hoy el plazo máximo es de 5 s, con la reverificación periódica.
