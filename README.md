# Santolv Services

Un backend Rust y una cuenta para Drive y Fotos. El servidor está en `backend/`;
las interfaces HTML/CSS/JavaScript se mantienen separadas:

| Dirección | Interfaz | Contenido |
| --- | --- | --- |
| `/` y `/index.html` | `frontend/home/index.html` | Acceso, cuenta y selector de servicios |
| `/drive/` | `frontend/drive/index.html` | Archivos, carpetas, compartidos y papelera |
| `/photos/` | `frontend/photos/index.html` | Fototeca, álbumes, favoritos, subidas y papelera |
| `/account/` | `frontend/account/index.html` | Perfil personal, apariencia y seguridad |

## Dónde editar

| Si cambias… | Carpeta |
| --- | --- |
| Autenticación, usuarios, permisos y activación de servicios | `backend/src/core/` |
| Rutas y lógica propias de Drive | `backend/src/modules/drive/` |
| Rutas y lógica propias de Fotos | `backend/src/modules/photos/` |
| Archivos, transferencias y tareas que comparten los módulos | `backend/src/file_store/` |
| Barra superior, navegación, cuenta y tema global | `frontend/shared/` |
| Inicio, Drive o Fotos | `frontend/home/`, `frontend/drive/` o `frontend/photos/` |
| Esquema de PostgreSQL | `backend/migrations/` |

El registro de módulos está en `backend/src/modules/mod.rs`. La instalación
local vive aparte en `backend/servidor/`. Para ver cambios en localhost,
compila desde `backend/` con `cargo build --release --locked`, detén el servidor
y sustituye el ejecutable de `servidor/` por el de `target/release/` antes de arrancar.

Sin sesión, `/` entrega únicamente `frontend/home/login.html`. Las rutas de
Drive y Fotos redirigen al mismo formulario. El inicio autenticado muestra
los servicios disponibles para la cuenta y lo último trabajado, sin barra lateral. El
historial reúne elementos autorizados de Drive y Fotos y abre cada uno en su
servicio. Las pantallas de Drive y Fotos ofrecen Inicio para volver al selector.
El inicio no muestra herramientas de los servicios. Rust entrega el documento
correspondiente con CSP y las tres interfaces se incluyen en el ejecutable al
compilar. Comparten cookies HttpOnly, sesiones y protección CSRF; no guardan
tokens en localStorage ni en enlaces. Cerrar sesión revoca el acceso común.

La barra superior reúne el tema global, el menú de aplicaciones de cuatro
puntos y la cuenta. Desde Drive y Fotos, el botón Inicio vuelve al selector.
El menú muestra los servicios activos. Cambiar de cuenta cierra la sesión
actual y vuelve al formulario de acceso. Claro/oscuro se guarda en el navegador
y se sincroniza entre servicios y pestañas. Fotos permite elegir el tema del
sistema desde Ajustes y aplica esa elección a todos los servicios.

## Almacenamiento

### Monolito modular

- `backend/src/core/` reúne cuentas, autenticación, permisos, seguridad HTTP,
  conexión a PostgreSQL y el gestor. `backend/src/modules/drive/` contiene
  espacios, miembros y compartición de Drive. `backend/src/modules/photos/`
  contiene la biblioteca y favoritos de Fotos. `backend/src/file_store/` es
  el motor reutilizado de archivos, transferencias y trabajos de disco.
- Cada servicio tiene sus propias tablas. Drive conserva sus tablas existentes
  en `public` y Fotos usa `photos`. Comparten identidad, sesiones y configuración.
- Para añadir un servicio compilado, crea su carpeta, migración si necesita tablas
  y descriptor `Service`, y añádelo a `modules::REGISTERED`. La propiedad
  `uses_file_store` habilita el motor y sus rutas compatibles para ese módulo.

Desde `backend/`, con la instalación central:

```powershell
.\servidor\santolvdrive.exe services list
.\servidor\santolvdrive.exe services disable photos
.\servidor\santolvdrive.exe services enable photos
```

En Linux, usa `./servidor/santolvdrive` para los mismos comandos.

Los comandos requieren acceso local a la configuración del servidor. PostgreSQL
guarda el estado entre reinicios. Desactivar devuelve `503 service_disabled`
en nuevas peticiones al módulo, lo oculta al volver a cargar Inicio y espera que
terminen los lotes internos ya iniciados. Las peticiones admitidas previamente
pueden concluir. Los trabajos persistentes y los datos quedan disponibles al
reactivarlo. La API de Inicio sigue operativa con ambos módulos desactivados.

Los módulos se incorporan compilando el servidor y aplicando sus migraciones.
`services list` distingue entre instalado y activo. No hay operaciones de
instalación o activación en la API de usuarios. Para administrar los permisos
por cuenta desde `backend/`:

```powershell
.\servidor\santolvdrive.exe users modules nombre
.\servidor\santolvdrive.exe users deny-module nombre photos
.\servidor\santolvdrive.exe users allow-module nombre photos
```

El bloqueo persiste en PostgreSQL. Afecta a la página, el catálogo, la actividad
reciente y las operaciones del módulo, incluidas las de archivos compartidos.
Quitar el bloqueo por usuario vuelve a dar acceso si el módulo está activo.
Tras actualizar desde una versión con la cookie limitada a la API, entra de nuevo
desde Inicio para obtener la cookie de las páginas protegidas.

Fotos requiere que Drive esté instalado, activo y permitido para la cuenta.
Desactivar Drive también suspende las rutas y las tareas de Fotos. Al reactivar
Drive se vuelve a aplicar el estado configurado para Fotos.

La migración `0013_modules.sql` mueve los metadatos de Fotos en una transacción
y conserva sus IDs, favoritos y referencias a los originales. Los originales
siguen en la misma ruta del disco. Detén el ejecutable anterior y guarda una
copia de PostgreSQL y el almacenamiento antes de instalar esta versión. Para
volver al ejecutable anterior, restaura esa copia.

### Carpetas del servidor

```text
<storage_root>/
  usuarios/<UUID del usuario>/
    drive/
      archivos/<carpeta>/<nombre del archivo>/<archivo original>
      papelera/...
    photos/
      archivos/<álbum>/<nombre de foto>/<foto original>
      papelera/...
  organizaciones/<UUID del espacio>/archivos/...
  .temporal/{cargas,publicaciones,movimientos}/...
```

Se utiliza el ID inmutable de las cuentas existentes. El nombre de usuario sigue
sirviendo para entrar. Se conserva el sistema de nombres reales, publicación
durable y reconciliación de Drive; sus espacios compartidos permanecen en
`organizaciones/`. Los temporales internos se identifican por operación. Las
cuotas y contadores se aplican por espacio, separados para Drive y Fotos.

La migración `0012_services.sql` registra las rutas anteriores. Antes de aceptar
peticiones, Rust mueve `usuarios/<nombre>` o `usuarios_compat/<UUID>` a
`usuarios/<UUID>/drive`, conservando nombres y diarios de operaciones pendientes.
Se reanuda tras una interrupción. Si existen simultáneamente origen y destino,
devuelve `storage_layout_conflict` conservando ambos. Fotos crea su espacio al
abrirse por primera vez, sin duplicar usuarios ni el motor de subidas.

Para actualizar: detener la versión anterior, guardar una copia de PostgreSQL y
del almacenamiento y arrancar con **el mismo** `server.json` y `storage_root`.
No ejecutar ambas versiones sobre los mismos datos. La migración es de avance;
volver a una versión anterior requiere restaurar la copia completa.

## Instalación

`backend/dist/` contiene los instaladores y `backend/servidor/` la instalación
local de todos los servicios. El ejecutable lee el `server.json` de su propia
carpeta. La web abre en [localhost:5500](http://localhost:5500/).
Consulta la [guía para Windows y Linux](backend/README.md#instalación-local).

## Fotos conectado

- En Drive, **Mi unidad → Fotos** abre una carpeta protegida con tus álbumes y
  **Cámara**, donde aparecen las fotos sin álbum. Drive permite navegar y
  descargar. Las subidas y los cambios de biblioteca se hacen desde Fotos.
  Las dos vistas leen los mismos originales.
- Subidas por bloques con SHA-256 y confirmación durable antes de mostrar éxito.
- Biblioteca paginada en grupos de 100 elementos; originales autenticados al
  hacerse visibles, una descarga a la vez y caché de ocho imágenes.
- Favoritos persistentes con control de revisiones, álbumes como carpetas y
  subidas dentro del álbum abierto.
- Papelera y restauración mediante el motor existente y su conservación.
- JPEG, PNG, WebP, GIF y AVIF de hasta 30 MiB. Fechas de subida y búsqueda sobre
  los elementos cargados.

Pendientes: miniaturas derivadas en el servidor, EXIF, vídeo, HEIC/RAW y clientes
móviles. La cuadrícula descarga originales, sin prometer derivados ni copia
automática del carrete. Las copias de seguridad son independientes del servidor.
La eliminación permanente no tiene botón propio en Fotos todavía.

## GitHub y versiones

Repositorio activo:
[santolv/santolvservices](https://github.com/santolv/santolvservices).
Los historiales de Drive y Fotos ya están importados. No hay submódulos ni Git
anidados: servidor e interfaces se versionan juntos en un mismo commit.
Usar ramas `codex/<cambio>` y subirlas a este `origin`. Las ramas anteriores
`santolvdrive` y `santolvphotos` se conservan, sin reescribirlas ni sincronizarlas
automáticamente. Los workflows activos están en `.github/workflows/`: ejecutan
CI y compilación desde `backend/`. Las etiquetas `v*` activan los instaladores.

## Comprobaciones

```powershell
cd backend
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
# DATABASE_URL: PostgreSQL de pruebas, con permiso CREATEDB.
cargo test --locked
node tests/frontend-client.cjs
node tests/photos-client.cjs
node tests/home-client.cjs
```

Se comprueban permisos, separación de interfaces y servicios, persistencia de
Fotos, originales, papelera y migración reanudable sin sobrescritura. Las pruebas
de instalación y carga marcadas `ignored` se ejecutan por separado.
