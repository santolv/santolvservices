# Santolv Services

Un backend Rust y una cuenta para Drive y Fotos. El servidor está en `backend/`;
las interfaces HTML/CSS/JavaScript se mantienen separadas:

| Dirección | Interfaz | Contenido |
| --- | --- | --- |
| `/` y `/index.html` | `frontend/index.html` | Acceso, cuenta y selector de servicios |
| `/drive/` | `santolvdrive/frontend/pruebahtml/index.html` | Archivos, carpetas, compartidos y papelera |
| `/photos/` | `santolvphotos/frontend/index.html` | Fototeca, álbumes, favoritos, subidas y papelera |

El inicio no muestra herramientas de los servicios. Rust entrega el documento
correspondiente con CSP y las tres interfaces se incluyen en el ejecutable al
compilar. Comparten cookies HttpOnly, sesiones y protección CSRF; no guardan
tokens en localStorage ni en enlaces. Cerrar sesión revoca el acceso común.

## Almacenamiento

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
Los directorios locales antiguos `santolvdrive/backend/servidor`, `target` y `dist`
se conservan como datos/artefactos ignorados; no se mueven ni se incluyen en Git.

## Ejecutar

Desde `backend/`:

```powershell
cargo build --locked
cargo run --locked -- install --directory servidor
cargo run --locked -- serve --config servidor/server.json
```

Si ya existe una instalación, reutiliza su configuración. Consulta
[backend/README.md](backend/README.md) para instalación y usuarios. El ejecutable
sigue llamándose `santolvdrive` para conservar los comandos existentes.
Por defecto: web `http://localhost:5500`, API `http://localhost:8443`.
Se conserva el despliegue limitado a loopback. En configuración HTTPS se mantiene
el modo API existente; la interfaz integrada descrita aquí es para HTTP local. `serve --frontend <archivo.html>`
mantiene el ensayo con una sola interfaz; las otras rutas web quedan desactivadas.

## Fotos conectado

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
```

Se comprueban permisos, separación de interfaces y servicios, persistencia de
Fotos, originales, papelera y migración reanudable sin sobrescritura. Las pruebas
de instalación y carga marcadas `ignored` se ejecutan por separado.
