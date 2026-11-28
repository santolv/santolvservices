# Editores ofimáticos (ArtCraft)

Estado: **fase 1 — editores integrados con Drive, edición individual**. La edición colaborativa en
tiempo real todavía **no** está implementada; su diseño y plan están en
[../collaboration/README.md](../collaboration/README.md).

## Qué se integra

| Editor | Servicio | Página | Release fijada | Commit |
|---|---|---|---|---|
| WordCraft | `documents` | `/documents/` | v0.19.1 | `5d658dc033f3139926839bffd6ae0eef558c210c` |
| GridCraft | `spreadsheets` | `/spreadsheets/` | v0.19.1 | `476448d7d9f3a5f8e93fcf7d98b47fd52293fe7f` |
| DeckCraft | `presentations` | `/presentations/` | v0.19.1 | `4a980c33ddf37eb64157777fcaeab6d19baa5878` |
| PdfCraft | `pdf` | `/pdf/` | v0.22.0 | `ac140f809d554981a5276d748e6193fcf4d4bdfa` |

Son las releases oficiales de GitHub (todas en fase alfa, autorizado por el propietario), con
licencia MIT OR Apache-2.0. La fuente de verdad es `backend/office-editors.toml`.

Se reutiliza **la aplicación web original completa** de cada proyecto (motor e interfaz egui,
compilados a WebAssembly), sin modificar su código. La suite solo aporta:

- la página que la aloja, con la barra superior común (`<!-- SANTOLV_SHELL -->`);
- el puente de host `frontend/shared/office-host.js`, común a los cuatro;
- la ruta `/apps/<editor>/…` que sirve el paquete WebAssembly;
- la apertura desde Drive y el guardado en Drive.

## Cómo funciona

1. Drive abre un `.docx`, `.xlsx`, `.pptx`, `.pdf` (y formatos afines) en
   `/<servicio>/?node=<id>` si el servicio está activado para la cuenta.
2. Core sirve la página solo con sesión válida y acceso al servicio, como cualquier módulo.
3. `office-host.js` lee el nodo y su contenido con la API de Drive (permisos de Drive) e importa el
   paquete del editor desde `/apps/<editor>/`.
4. El editor se ejecuta en modo «página» (`data-host="page"` en su lienzo), parte del contrato
   `craft-web-host` común a ArtCraft: avisa `<app>:ready`, el host le envía `<app>:open` con el
   archivo, y cada guardado llega como `<app>:save` en vez de descargarse.
5. Guardar el mismo archivo crea una nueva versión del nodo con la subida reanudable de Drive y
   `If-Match` con la revisión leída: si otra sesión guardó antes, Drive responde conflicto y no se
   sobrescribe nada. «Guardar como» y las exportaciones crean un archivo nuevo en la misma carpeta.
6. El indicador inferior distingue «Cambios sin guardar», «Guardando…» y «Guardado en Drive».

## Despliegue de los paquetes

Los paquetes compilados **no se versionan**. `tools/build-office-editors.ps1` clona cada release
por etiqueta, comprueba que apunta al commit fijado, compila con `cargo build --locked` (el
`Cargo.lock` del propio proyecto), genera el JavaScript con `wasm-bindgen` y los instala en
`backend/servidor/apps/<editor>/` junto con `RELEASE.txt`. El servidor los sirve desde la carpeta
`apps/` situada junto al ejecutable.

Herramientas: objetivo `wasm32-unknown-unknown` y `wasm-bindgen-cli` de la misma versión que el
`wasm-bindgen` bloqueado por cada editor (0.2.129 en WordCraft v0.19.1). No se usa `trunk` ni
`wasm-opt`; por eso el paquete de WordCraft ocupa unos 90 MB sin optimizar.

## Límites conocidos de esta fase

- Edición individual: si dos personas abren el mismo archivo, el segundo guardado recibe conflicto
  de revisión y debe guardar con otro nombre. No hay pérdida silenciosa, pero tampoco fusión.
- Paquetes grandes (WordCraft ≈ 90 MB): la primera carga es lenta fuera de la red local.
- La compatibilidad con Office es la de cada release de ArtCraft; no se ha verificado todavía con
  pruebas de ida y vuelta propias.
- Requieren WebGPU o WebGL2 en el navegador.
