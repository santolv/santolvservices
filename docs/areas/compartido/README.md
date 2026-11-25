# Área Compartido

Shell y motor de archivos son dos partes compartidas, con carpetas separadas. Un cambio al shell valida sus cuatro pantallas; uno al motor valida Drive y Fotos.

- Carpeta principal: `frontend/shared/`.
- Código relacionado: `backend/src/file_store/`.
- Puesto local preparado: `.worktrees/compartido/`, rama inicial `codex/compartido`.
- Rama de nuevas tareas: `codex/compartido-<tarea>-<persona>`.
- Guía común: [Colaboración](../../COLABORACION.md).

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
