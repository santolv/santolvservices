# Área Core

Autenticación, usuarios, permisos, API, gestor de servicios, PostgreSQL e instalación. Los cambios comunes se integran antes de sus consumidores o son compatibles.

- Carpeta principal: `backend/src/core/`.
- Código relacionado: `backend/src/main.rs, lib.rs, installer.rs, provisioning.rs, dependencies.rs y tls.rs`.
- Puesto local preparado: `.worktrees/core/`, rama inicial `codex/core`.
- Rama de nuevas tareas: `codex/core-<tarea>-<persona>`.
- Guía común: [Colaboración](../../COLABORACION.md).

## Comprobaciones

Desde la raíz del puesto:

```sh
cargo fmt --manifest-path backend/Cargo.toml --check
git diff --check
```

Si cambia Rust, añadir `cargo fmt --manifest-path backend/Cargo.toml --check`,
`cargo clippy --manifest-path backend/Cargo.toml --locked --all-targets -- -D warnings`
y `cargo test --manifest-path backend/Cargo.toml --locked` con PostgreSQL de pruebas.

La prueba del cliente no demuestra el aspecto visual ni todas las operaciones de
esta área. Comprobar los estados afectados y describir en la PR qué se verificó.
No editar contratos comunes o migraciones sin coordinar sus consumidores.
