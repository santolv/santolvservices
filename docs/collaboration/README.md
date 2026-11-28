# Motor colaborativo (Yrs)

Estado: **diseño; sin implementar todavía.** Esta página recoge lo averiguado al analizar ArtCraft y
el plan acordado. Cada fase se documentará aquí con resultados reales.

## Lo que hay que resolver

Ninguno de los editores ArtCraft tiene edición colaborativa. En WordCraft (≈333 000 líneas de
Rust), cada acción es un comando que pasa por `Session::run`; el deshacer se basa en instantáneas
del árbol del documento (bloques compartidos con `Arc`) y no existe un registro de operaciones
transmisible. Por tanto la colaboración necesita una capa de traducción entre el modelo de cada
editor y Yrs, sin reescribir los editores:

- **Documentos (WordCraft):** detectar tras cada comando los bloques cambiados (comparación de
  punteros `Arc`), y reflejarlos en Yrs: el texto de cada párrafo como `Y.Text` con formato (para
  escribir a la vez en el mismo párrafo) y el resto de bloques (tablas, imágenes, secciones) como
  valores con identidad estable. Los cambios remotos se aplican con las primitivas del propio
  modelo (`insert_text`, `delete_range`, `format_range`, `insert_block`, `remove_block`).
- **Hojas de cálculo (GridCraft):** mapa de celdas por identificador estable de fila y columna;
  conflicto en una misma celda resuelto por el orden determinista de Yrs; inserciones y borrados
  de filas como operaciones sobre listas de identificadores.
- **Presentaciones (DeckCraft):** lista de diapositivas y mapa de objetos por identificador.
- **PDF (PdfCraft):** anotaciones y operaciones de página como entradas independientes; la
  estructura del PDF original no se modifica concurrentemente.

Esto exige añadir a cada editor un punto de enganche (cambios locales y aplicación de remotos)
desde nuestra rama de integración, manteniendo el resto del código original.

## Servidor (backend Rust)

Carpeta `backend/src/collaboration/`, independiente de los editores:
`protocol` (mensajes binarios versionados), `transport` (WebSocket de Axum), `sessions`
(un documento = una sala con sus conexiones), `permissions` (lectura, comentario, edición,
administración sobre los permisos de Drive), `crdt` (Yrs), `persistence` (registro de
actualizaciones con identificador único en PostgreSQL), `snapshots`, `recovery`, `presence`.

Dependencias autorizadas: `yrs` 0.28.2 y transitivas, función `ws` de Axum (`tokio-tungstenite`),
`wasm-bindgen` y `js-sys`.

## Plan

1. Fase 1 (hecha): editores integrados con Drive, guardado con control de revisión.
2. Servidor colaborativo: protocolo, autenticación y autorización por mensaje, salas, persistencia,
   instantáneas, recuperación, límites de recursos; pruebas de seguridad, convergencia y carga.
3. Adaptador de WordCraft y prueba con varios navegadores.
4. Adaptadores de GridCraft, DeckCraft y PdfCraft.
5. Pruebas de carga (2, 10, 25, 50 y 100 usuarios) con resultados medidos, y auditoría.
