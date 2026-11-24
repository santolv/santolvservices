# Plan de interfaz de SantolvPhotos

Fecha: 24 de noviembre de 2026.

## Objetivo y alcance

Construir una interfaz web para una biblioteca personal de fotos y vídeos que
pueda alojarse en servidores propios. Google Fotos e Immich sirven de referencia
funcional: biblioteca cronológica, visor, álbumes y gestión de la colección.
La identidad visual será propia.

Este documento planifica únicamente `frontend/`. No incluye desarrollar la API,
base de datos, almacenamiento, indexación, transcodificación ni despliegue del
servidor. La primera interfaz funcionará con datos de demostración locales,
identificados como tales. No se presentará como una aplicación con almacenamiento
real, autenticación real o copias de seguridad.

## Estado y decisiones

- [x] Comprobar Rust: rustc y Cargo 1.99.1; toolchain estable Windows MSVC.
- [x] Revisar las dependencias y el servidor de interfaz de santolvdrive.
- [x] Crear este plan local y registrar el flujo de commits y publicación.
- [ ] Confirmar los archivos auxiliares permitidos antes de implementar la web.
- [ ] Implementar las fases siguientes cuando el propietario lo solicite.

Rust es el lenguaje principal y la lógica propia se escribirá en Rust.
**La propuesta web todavía no está aprobada para implementación:** requiere
HTML mínimo, CSS y JavaScript de arranque generado por las herramientas de
WebAssembly. Se pedirá autorización explícita explicando esa función antes de
crear esos archivos. La petición de revisar santolvdrive no equivale a aprobarlos.
No se propone escribir lógica propia en JavaScript o TypeScript.

Si se mantiene una restricción absoluta sobre esos archivos, habrá que revisar
la plataforma y valorar una interfaz nativa Rust, por ejemplo con egui/eframe.
Eso cambiaría la experiencia de acceso desde navegador y móvil; no se elegirá
silenciosamente como sustituto de la web autohospedada.

## Qué reutilizamos de SantolvDrive

Referencia revisada: rama `main`, commit
`b9c19e100e15c85534ede1a38e331411d4d2edad`.

| Evidencia | Hallazgo | Aplicación a este frontend |
| --- | --- | --- |
| `backend/Cargo.toml` | Rust edición 2024; mínimo declarado 1.94 | Mantener Rust 2024; comprobar el mínimo real de las nuevas dependencias al implementarlas |
| Manifiesto del backend | Axum 0.8, Tokio 1, Tower 0.5, SQLx 0.9, Rustls y PostgreSQL | Referencia futura del servidor; no son librerías de interfaz ni se añadirán ahora |
| Manifiesto del backend | Serde 1, serde_json 1, uuid 1 y time 0.3 | Reutilizar estas familias de crates donde haya necesidad real en los modelos de interfaz |
| `frontend/pruebahtml/README.md` | Un HTML con CSS y JavaScript propios, sin framework frontend | Referencia de flujos y acabado, no base de código compatible con la nueva restricción |
| `backend/src/frontend.rs` | Axum entrega un HTML desde un listener separado limitado a localhost, con CSP y nonce | Mantener separación conceptual entre interfaz y API; no copiarlo como despliegue público |

El servidor actual de Drive solo sirve `/` e `/index.html`, limita el HTML a
1 MiB y no publica un directorio de recursos. Una aplicación WASM con archivos
separados necesitará otra forma de servirlos y una CSP compatible. Esto es una
dependencia para la integración futura, no una tarea de backend de esta fase.

Tomaremos como referencia las sesiones por cookies HttpOnly y la protección
CSRF de Drive para el futuro cliente. No se guardarán contraseñas ni tokens de
sesión en localStorage. No se presupone que sus endpoints sirvan para fotos.

## Tecnologías propuestas

| Herramienta o crate | Para qué se utilizaría | Procedencia o condición |
| --- | --- | --- |
| Rust + Cargo | Componentes, lógica, estado y pruebas | Ya disponibles |
| Leptos en modo CSR | Interfaz reactiva ejecutada en el navegador mediante WASM | Nueva dependencia propuesta; pendiente la decisión web |
| leptos_router | Navegación y enlaces a biblioteca, álbumes y ajustes | Misma familia y versión compatible con Leptos |
| Trunk | Desarrollo local y empaquetado estático de la aplicación Rust/WASM | Nueva herramienta; comprobar e instalar al empezar, no ahora |
| wasm-bindgen, web-sys y wasm-bindgen-futures | Acceso desde Rust a archivos, eventos y tareas del navegador | Añadir solo las APIs necesarias |
| serde / serde_json | Modelos y datos de demostración; futura serialización | Familia utilizada en Drive; JSON solo cuando sea necesario |
| uuid / time | Identificadores y fechas de la biblioteca | Familia utilizada en Drive; verificar funciones compatibles con WASM |
| web-sys o gloo-net | Futuro cliente HTTP asíncrono | Elegir una sola vía al integrar; no copiar reqwest del servidor sin evaluar WASM |
| HTML mínimo y CSS propio | Entrada del navegador, disposición y diseño adaptable | Requieren aprobación antes de crearlos |

No se añadirán React, Vue, Node, npm, Tailwind ni servicios externos como
requisitos de esta propuesta. El código de arranque JavaScript que genere WASM
debe explicarse y aprobarse aunque no lo escribamos a mano.

El target `wasm32-unknown-unknown` y Trunk no están disponibles en el entorno
comprobado. Su instalación quedará para el comienzo de la implementación.
Las versiones concretas se comprobarán entonces y se fijarán en Cargo.lock;
no se elegirán números de versión sin validar su compatibilidad.

## Diseño y navegación

Interfaz en español, adaptable a ordenador, tableta y móvil. En escritorio:
barra lateral, búsqueda superior y área de contenido. En móvil: navegación
compacta y controles táctiles. Tipografía del sistema y recursos servidos
localmente, sin depender de CDN para usar la aplicación.

Secciones iniciales: Fotos, Álbumes, Favoritos, Papelera y Ajustes. La subida
estará accesible desde la biblioteca. El visor abrirá sobre el contexto actual
y conservará la posición de desplazamiento al cerrarse.

Estados comunes: cargando, vacío, error, sin conexión, selección activa y acción
pendiente. Cada acción de demostración indicará que solo modifica datos locales
y que se reinician al recargar, salvo una preferencia visual explícita.

## Fases y criterios de entrega

### 0. Confirmar la base web

- Resolver la autorización sobre HTML/CSS y JavaScript generado.
- Confirmar Leptos CSR y la estrategia de compilación.
- Mantener inicialmente un solo paquete Cargo en `frontend/`.

**Se completa cuando:** las decisiones permiten implementar sin introducir
lenguajes o dependencias no autorizados.

### 1. Esqueleto y sistema visual

- Crear la aplicación Rust, navegación y estructura de componentes.
- Definir colores, espacios, tamaños, foco visible, botones, menús y diálogos.
- Implementar tema claro/oscuro y adaptación a pantallas pequeñas.
- Usar iconos locales; si se reutilizan Lucide/SVG, conservar licencia y
  confirmar su encaje con los formatos permitidos.

**Se completa cuando:** todas las rutas iniciales se pueden abrir y recargar,
la navegación funciona con teclado y no hay desbordamiento horizontal en móvil.

### 2. Biblioteca cronológica

- Crear modelos Rust para fotos, vídeos, fechas, dimensiones y favoritos.
- Preparar datos de ejemplo y recursos locales sin fotos privadas del usuario.
- Mostrar una cuadrícula por días/meses, con proporciones variadas y vídeos
  identificados. Definir fecha de captura frente a fecha de incorporación.
- Añadir carga diferida de miniaturas, paginación de demostración y errores
  de imagen. Evaluar virtualización si las mediciones la justifican.

**Se completa cuando:** se puede recorrer una colección de ejemplo amplia sin
cargar los originales de toda la biblioteca, y se muestran estados vacíos y errores.

### 3. Visor de foto y vídeo

- Abrir/cerrar, avanzar/retroceder, zoom y panel de información.
- Atajos de teclado, navegación táctil y restauración del foco al salir.
- Reproducir vídeos de muestra compatibles con el navegador.
- Mostrar nombre, fecha y dimensiones sin inventar metadatos ausentes.

**Se completa cuando:** el visor conserva selección y posición de la biblioteca,
permite usar Escape/flechas y gestiona medios no compatibles sin bloquearse.

### 4. Organización y búsqueda local

- Selección individual y múltiple; favorito y envío a papelera con confirmación
  cuando corresponda. Restaurar desde papelera; borrado solo en la demostración.
- Crear/renombrar álbumes y añadir o quitar fotos sin duplicar sus contenidos.
- Buscar por nombre y filtrar por fecha, tipo y favoritos sobre los datos demo.
- Conservar los filtros al entrar y salir del visor.

**Se completa cuando:** los cambios locales se reflejan coherentemente en todas
las vistas y se indica que todavía no persisten en un servidor.

### 5. Subida y pantallas de cuenta

- Selector de archivos y arrastrar/soltar con previsualización local.
- Cola visual con progreso, cancelación y errores claramente simulados.
- Liberar URLs temporales y evitar cargar archivos grandes completos en memoria.
- Pantallas de acceso, sesión caducada, ajustes y uso de almacenamiento con
  estados de demostración; no fingir autenticación ni espacio real.

**Se completa cuando:** la experiencia puede revisarse sin backend y ninguna
pantalla afirma que una foto ha quedado guardada o respaldada realmente.

### 6. Preparación para integrar y revisión final

- Separar el proveedor de datos de demostración de los componentes Rust.
- Documentar qué datos y operaciones necesita la interfaz, sin implementar
  endpoints, base de datos ni decisiones de servidor.
- Preparar el paquete estático y las instrucciones de desarrollo y compilación.
- Revisar carga de recursos, errores, navegación, temas y uso desde móvil.

**Se completa cuando:** la interfaz demo es compilable, revisable y tiene una
frontera clara para sustituir los datos locales por una API futura.

## Organización prevista de frontend/

```text
frontend/
  PLAN.md
  README.md             # Instrucciones cuando exista una aplicación
  Cargo.toml
  Cargo.lock
  src/
    main.rs
    app.rs
    routes.rs
    models.rs
    state.rs
    components/
    pages/
    data/               # Proveedor demo y futura interfaz de datos
  public/               # Muestras e iconos autorizados y sus licencias
  tests/                # Pruebas Rust con valor funcional
```

Si se autoriza la propuesta web, se añadirán `index.html`, estilos CSS y
`Trunk.toml`. La estructura es una previsión; hoy solo se crea este documento.

## Comprobación y flujo de trabajo

En cada entrega: revisar el diff, comprobar lo que se haya modificado, actualizar
las casillas del plan, crear commit local y hacer push al repositorio autorizado.
Revisar el estado remoto antes de subir; no sobrescribir cambios ajenos.
Si la autenticación falla, conservar el commit e indicar el inicio de sesión necesario.

Cuando exista código: `cargo fmt --check`, comprobación del target WASM,
Clippy y compilación de producción con Trunk. Usar `--locked` cuando exista
Cargo.lock. Probar en Rust las reglas de selección, filtros, fechas y álbumes;
adaptar el ejecutor al target en vez de intentar ejecutar WASM como binario nativo.
Revisión en navegador de teclado/foco, recarga de rutas y tamaños aproximados
de 360, 768 y 1440 píxeles. No introducir otro lenguaje para pruebas sin permiso.
Un documento de planificación se verifica por contenido, enlaces y diff;
no requiere generar una aplicación ni instalar sus herramientas.

## Funciones posteriores, fuera de esta primera interfaz

Reconocimiento facial, búsqueda semántica, mapas, edición de fotos, enlaces
compartidos, sincronización automática móvil, reproducción de todos los codecs,
generación de miniaturas y copias de seguridad requieren trabajo adicional.
No se mostrarán como funciones operativas durante la fase demo.

## Fuentes

- [Manifiesto de SantolvDrive](https://github.com/santolv/santolvdrive/blob/b9c19e100e15c85534ede1a38e331411d4d2edad/backend/Cargo.toml).
- [Servidor de interfaz de SantolvDrive](https://github.com/santolv/santolvdrive/blob/b9c19e100e15c85534ede1a38e331411d4d2edad/backend/src/frontend.rs).
- [Documentación de la interfaz de Drive](https://github.com/santolv/santolvdrive/blob/b9c19e100e15c85534ede1a38e331411d4d2edad/frontend/pruebahtml/README.md).
- [Libro oficial de Leptos](https://book.leptos.dev/).
- [Despliegue estático CSR con Leptos](https://book.leptos.dev/deployment/csr.html).
- [Repositorio oficial de Trunk](https://github.com/trunk-rs/trunk).
