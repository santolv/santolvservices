# Área Compartido

Shell y motor de archivos son dos partes compartidas, con carpetas separadas. Un cambio al shell valida sus cuatro pantallas; uno al motor valida Drive y Fotos.

- Carpeta principal: `frontend/shared/`.
- Código relacionado: `backend/src/file_store/`.
- Puesto local preparado: `.worktrees/compartido/`, rama inicial `codex/compartido`.
- Rama de nuevas tareas: `codex/compartido-<tarea>-<persona>`.
- Guía común: [Colaboración](../../COLABORACION.md).

## Capa de diseño de la suite

- `frontend/shared/shell.css` define los tokens comunes (`--ui-*`): paleta clara
  y oscura de estilo Apple, tipografía, radios, ancho del lateral (224 px) y
  escala de títulos.
- `frontend/shared/suite.css` se inyecta desde Core al final del `<head>` de
  todas las páginas con shell, después de los estilos de cada servicio. Unifica
  barra superior y menús, laterales, títulos, buscadores, controles segmentados,
  botones cápsula, hojas de diálogo y estados vacíos, y conecta las paletas
  históricas de cada servicio con los tokens comunes.
- Cada servicio conserva su distribución y su color de acento (Calendario rojo,
  Notas amarillo, Tareas, Drive, Contactos y Correo azul). Los colores de tipos
  de archivo, carpetas y la fototeca 4:3 no dependen de esta capa.
- La pantalla de acceso no usa el shell; replica los mismos valores en
  `frontend/home/login.html`.

## Comprobaciones

Desde la raíz del puesto:

```sh
node backend/tests/shell-client.cjs
git diff --check
```

Si cambia Rust, añadir `cargo fmt --manifest-path backend/Cargo.toml --check`,
`cargo clippy --manifest-path backend/Cargo.toml --locked --all-targets -- -D warnings`
y `cargo test --manifest-path backend/Cargo.toml --locked` con PostgreSQL de pruebas.

La prueba del cliente no demuestra el aspecto visual ni todas las operaciones de
esta área. Comprobar los estados afectados y describir en la PR qué se verificó.
No editar contratos comunes o migraciones sin coordinar sus consumidores.
