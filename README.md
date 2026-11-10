# Super Cerebro

Espacio de conocimiento que combina la base de Obsidian con páginas, bloques y colecciones inspirados en Notion. El servidor es Rust, sin base de datos: las páginas siguen siendo archivos Markdown en `app/vault/`.

## Ejecutar

```powershell
cd app
cargo run
```

Abre [Mis archivos](http://127.0.0.1:7878/). Desde ahí puedes crear carpetas y documentos de texto, hojas de cálculo, presentaciones y PDF, subir y descargar archivos, y abrir [Super Cerebro](http://127.0.0.1:7878/cerebro). Ambos usan la misma bóveda. El gestor tiene tema claro y oscuro.

## Versiones y respaldo

Desde la raíz del proyecto, guarda una versión con `git add -u` y `git commit -m "Describe el cambio"`. Añade los archivos nuevos con `git add ruta`. Consulta los puntos de restauración con `git log --oneline` y abre una versión en una rama aparte con `git switch -c recuperar <commit>`.

Git guarda el código de forma local. Puedes exportar el historial con `git bundle create ../super-cerebro.bundle --all` y copiar el archivo a otra unidad. Para respaldar tus documentos y notas, copia también `app/vault/` o la ruta configurada en `SUPER_CEREBRO_VAULT`.

## Grafo y bloques de notas

El grafo abre en 3D. Arrastra el fondo o usa las flechas con el grafo enfocado para girar. Arrastra con Mayús para desplazar la cámara. El botón **3D** cambia a la vista plana.

En un bloque vacío, escribe `/calendario` para insertar un calendario con eventos, `/gráfico` para editar un gráfico de barras por pares como `Enero: 12`, o `/lienzo` para crear o incrustar un canvas editable.

En WordCraft, **Chat con IA** abre un panel junto al documento. **Copiar consulta** reúne la pregunta y el texto del DOCX guardado para pegarlo en el asistente que elijas. Si hay cambios pendientes, guarda primero en WordCraft. Las preguntas se guardan en el navegador.

## Servidor

Compila desde `app` con `cargo build --release`. Ejecuta `target/release/notionobsidian` (`notionobsidian.exe` en Windows) con estas variables de entorno.

| Variable | Valor |
| --- | --- |
| `SUPER_CEREBRO_VAULT` | Ruta absoluta a la carpeta persistente de la bóveda. |
| `SUPER_CEREBRO_HOST` | IP de escucha. Por defecto `127.0.0.1`. |
| `SUPER_CEREBRO_PORT` | Puerto HTTP. Por defecto `7878`. |
| `SUPER_CEREBRO_USER` y `SUPER_CEREBRO_PASSWORD` | Credenciales de acceso a la bóveda. Obligatorias al escuchar fuera de localhost. |
| `SUPER_CEREBRO_TOOLS` | Accesos adicionales en el gestor. Ejemplo `[{"name":"Calendario","url":"https://calendario.example.com"}]`. |

Para publicar el servicio, configura las credenciales y pon un proxy inverso HTTPS delante. Si el proxy se ejecuta en el mismo servidor, puedes dejar la escucha en localhost. Conserva la cabecera `Host` de la petición al reenviarla. `GET /healthz` comprueba que el proceso responde. La bóveda y el estado del espacio de trabajo se comparten entre quienes tengan acceso.

## Editores Craft

Desde **Mis archivos**, usa **Nuevo** o elige un editor en **Herramientas**. También puedes subir un archivo existente y abrirlo desde la lista.

| Editor | Formato |
| --- | --- |
| WordCraft | .docx |
| DeckCraft | .pptx |
| GridCraft | .xlsx |
| PdfCraft | .pdf |

Los editores se ejecutan en el navegador con WebAssembly. Abren los archivos de la bóveda y **Guardar** escribe allí los cambios. Un guardado que llega desde una versión antigua se rechaza y permite descargar una copia. **Guardar como** y las exportaciones crean archivos junto al original y rechazan nombres que ya existen. El límite de carga y guardado es de 32 MiB por archivo.

Para instalar las cuatro compilaciones oficiales, ejecuta desde `app`:

```powershell
pwsh -File ./install-craft.ps1
```

El instalador comprueba los SHA-256 y extrae los paquetes en `app/craft-apps/`. Para desplegar en otro servidor, copia esa carpeta al directorio desde el que arrancas el ejecutable o configura `SUPER_CEREBRO_CRAFT_ROOT` con su ruta absoluta. Si instalas los ZIP manualmente, deja los archivos de cada paquete, incluido `index.html`, en `wordcraft/`, `deckcraft/`, `gridcraft/` y `pdfcraft/` bajo esa ruta. Usa localhost o HTTPS y un navegador con WebGPU o WebGL2.

## Chat con IA

Abre **Chat con IA** en la barra lateral o usa **Preguntar sobre este archivo** en una nota, colección o lienzo. Las preguntas se guardan por conversación en el navegador. **Copiar consulta** incluye la pregunta y, si hay un archivo adjunto, su contenido actual.

La interfaz está preparada para trabajar sin proveedor. La copia va al portapapeles cuando la pides. Las consultas se guardan localmente y no generan respuestas.

## Menciones de archivos

En una nota, escribe @, pulsa el botón Mencionar archivo o usa Ctrl+Alt+M. El buscador muestra archivos de la bóveda. Elige uno o usa Crear archivo para añadir un documento, una presentación, una hoja de cálculo o un PDF en la carpeta de la nota. La tarjeta abre el archivo original.

En WordCraft, **Mis archivos** está en la esquina superior izquierda del editor. Super Cerebro comprueba cada 4 segundos si hay cambios y guarda el DOCX mientras el editor tiene el foco. También puedes pulsar **Guardar**. WordCraft desactiva su autoguardado nativo en la versión web.

## Dos formas de trabajar, una sola bóveda

- **Markdown:** se conservan el editor CodeMirror, la vista en vivo, el modo fuente, la lectura, las pestañas, las divisiones, los enlaces internos, backlinks, búsqueda, grafo, notas diarias y lienzos.
- **Bloques:** las notas y páginas Markdown se abren en el mismo editor de bloques. Los cambios se guardan en el archivo original con autoguardado y borradores locales.
- **Páginas:** el panel de páginas muestra la jerarquía lógica; el panel de archivos conserva las carpetas físicas originales. Una subpágina guarda su referencia `parent` en YAML y queda enlazada desde su página padre.
- **Colecciones:** cada fila es una página Markdown que puede enlazarse, abrirse por bloques y aparecer en el grafo. No hay una segunda copia del contenido.

## Páginas y bloques

- Texto, seis niveles de encabezado Markdown, listas con viñetas o números, tareas, citas, notas destacadas, desplegables, código, divisores y tablas sencillas.
- Menú `/`, con búsqueda por tipos y alias como `h1`, `todo`, `code`, `page` y `database`.
- Reordenar por arrastre o menú; duplicar, convertir y eliminar bloques; deshacer/rehacer los cambios de estructura.
- El texto del bloque se edita sin sus marcadores estructurales: un encabezado sigue siendo un encabezado y una tarea conserva su estado.
- Imágenes por URL, enlaces internos, subpáginas y colecciones incrustadas mediante `[[ruta.base]]`.
- Emblemas, cinco estilos de portada y propiedades YAML editables.
- Plantillas de proyecto, reunión, diario y lectura; guardar páginas como plantillas propias.
- Mover páginas en la jerarquía desde su menú sin perder las referencias.

## Colecciones

- Vistas de tabla, tablero, galería, calendario y lista; añadir y renombrar vistas.
- Filtros combinados con Y y ordenación independientes por vista.
- Propiedades de texto, selección, selección múltiple, fecha, número, casilla, URL y relaciones entre páginas.
- Fórmulas sencillas: suma, resta, multiplicación, número de relaciones y días hasta una fecha.
- Acumulaciones sobre páginas relacionadas: recuento, suma y promedio de una propiedad numérica.
- Tablero agrupable por una propiedad de selección; arrastrar tarjetas cambia su valor.
- Calendario navegable que usa una propiedad de fecha configurable.
- Cada registro abre una página con propiedades del tipo definido en la colección.
- Duplicar y archivar registros; recuperar los archivados desde las opciones de vista.
- Plantillas propias para nuevos registros.
- Importación CSV con previsualización y exportación de la vista visible como CSV. Los archivos existentes no se sobrescriben.
- Renombrar colecciones y carpetas actualiza la pertenencia de sus páginas y las referencias.

## Acabado visual

La interfaz conserva el espacio de trabajo anterior e incorpora la paleta Obsidian con fondos neutros y acento violeta, portadas ornamentales y títulos con serifas. Incluye temas oscuro y claro y un escritorio con accesos a páginas, colecciones y plantillas.

## Atajos

En una pestaña normal, usa Ctrl+Alt+N para crear notas. Para capturar Ctrl+N, Ctrl+O, Ctrl+P, Ctrl+T y Ctrl+W, abre la paleta y elige **Activar atajos en pantalla completa**. El navegador debe admitir Keyboard Lock. Pulsa Escape para volver.

| Atajo | Acción |
| --- | --- |
| Ctrl+Alt+P | Paleta de comandos |
| Ctrl+Alt+O | Abrir nota |
| Ctrl+Alt+N | Nueva nota Markdown |
| Ctrl+Alt+T / Ctrl+Alt+W | Nueva pestaña / cerrar pestaña |
| Ctrl+Alt+RePág / Ctrl+Alt+AvPág | Cambiar pestaña |
| Ctrl+S | Guardar |
| Ctrl+Alt+E | Alternar edición y lectura |
| Ctrl+F | Buscar en el editor Markdown |
| Ctrl+Alt+F | Buscar en la bóveda |
| Ctrl+Alt+G | Grafo |
| Ctrl+B / Ctrl+I / Ctrl+Alt+K | Negrita / cursiva / enlace interno |
| Ctrl+Alt+, | Ajustes |
| Ctrl+Alt+J / Ctrl+Alt+L | Navegación anterior / siguiente |
| / en un bloque vacío | Elegir tipo de bloque |
| Enter / Mayús+Enter en un bloque | Nuevo bloque / salto de línea |

En macOS se puede usar Cmd en lugar de Ctrl.

## Formatos y almacenamiento

- Notas y registros: `.md`, con propiedades YAML cuando son necesarias.
- Lienzos: `.canvas`, JSON.
- Colecciones: `.base`, JSON propio de esta aplicación con campos, vistas y plantillas. **No es el formato nativo de Obsidian Bases.**
- Espacio de trabajo: `vault/.obsidian/workspace.json`.
- Preferencias y borradores de recuperación: almacenamiento local del navegador.

La pertenencia a una colección usa `_database`, y las subpáginas usan `parent`. Se reservan también `icon`, `cover`, `editor`, `template` y `archived` para la presentación y organización. Los enlaces de relación usan `[[ruta/de/la/nota]]` y participan en el grafo.

## Desarrollo y validación

```powershell
cd app
npm ci
npm run check
npm run test
npm run build
cargo test
cargo run
```

En modo debug, el servidor carga los recursos de interfaz desde `src/`: después de modificar JavaScript basta con `npm run build` y recargar el navegador; los cambios de CSS se ven al recargar. En modo release, los recursos quedan incorporados al ejecutable mediante `include_str!`; recompila Rust para actualizarlo.

- `app/src/main.rs`: servidor HTTP, almacenamiento y API.
- `app/src/ui/app.js`: espacio de trabajo y funciones de Obsidian.
- `app/src/ui/notion.js`: escritorio, páginas, bloques y colecciones.
- `app/src/ui/knowledge.js`: YAML, bloques, filtros, cálculos, relaciones y CSV.
- `app/src/ui/editor.js`: editor Markdown.
- `app/src/ui/markdown.js`: renderizado saneado de Markdown.
- `app/src/ui/app.css`: estructura original.
- `app/src/ui/notion.css`: estilo y componentes nuevos.
- `app/tests/knowledge.test.mjs`: pruebas de persistencia del contenido, cálculos, vistas, referencias y CSV.

- `app/src/server.rs` valida la configuración y controla el acceso y las escrituras.
- `app/src/craft.rs` sirve los editores web y guarda sus archivos en la bóveda.
- `app/src/hub.html` y `app/src/ui/hub.js` forman el gestor de archivos.

Los módulos antiguos `src/js/` y `src/css/` se conservan como referencia y no se cargan.

## Alcance

Es una implementación independiente. No incluye colaboración en tiempo real, permisos compartidos, Notion AI, sincronización con cuentas de Notion, Obsidian Sync/Publish ni plugins de terceros. Las fórmulas son las operaciones descritas arriba, no el lenguaje completo de Notion. Los desplegables se guardan como HTML `details`, y las imágenes se insertan por URL. No están implementados todos los bloques, automatizaciones o formatos de importación de Notion.

Archivar un registro es reversible. Eliminar un archivo o carpeta desde el explorador pide confirmación y es permanente.
