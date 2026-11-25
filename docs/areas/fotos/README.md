# Área Fotos

Fototeca, álbumes, favoritos y derivadas. Depende de Drive y utiliza file_store. Conservar 4:3 en galería, una derivada 1280 px y originales intactos.

- Carpeta principal: `frontend/photos/`.
- Código relacionado: `backend/src/modules/photos/`.
- Puesto local preparado: `.worktrees/fotos/`, rama inicial `codex/fotos`.
- Rama de nuevas tareas: `codex/fotos-<tarea>-<persona>`.
- Guía común: [Colaboración](../../COLABORACION.md).

## Comprobaciones

Desde la raíz del puesto:

```sh
node backend/tests/photos-client.cjs
git diff --check
```

Si cambia Rust, añadir `cargo fmt --manifest-path backend/Cargo.toml --check`,
`cargo clippy --manifest-path backend/Cargo.toml --locked --all-targets -- -D warnings`
y `cargo test --manifest-path backend/Cargo.toml --locked` con PostgreSQL de pruebas.

La prueba del cliente no demuestra el aspecto visual ni todas las operaciones de
esta área. Comprobar los estados afectados y describir en la PR qué se verificó.
No editar contratos comunes o migraciones sin coordinar sus consumidores.
