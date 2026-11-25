# Área Drive

Espacios, archivos, carpetas, miembros y compartidos. Cargas, publicación y borrado viven en file_store y requieren coordinación. Mantener colisiones y arrastre.

- Carpeta principal: `frontend/drive/`.
- Código relacionado: `backend/src/modules/drive/`.
- Puesto local preparado: `.worktrees/drive/`, rama inicial `codex/drive`.
- Rama de nuevas tareas: `codex/drive-<tarea>-<persona>`.
- Guía común: [Colaboración](../../COLABORACION.md).

## Comprobaciones

Desde la raíz del puesto:

```sh
node backend/tests/frontend-client.cjs
git diff --check
```

Si cambia Rust, añadir `cargo fmt --manifest-path backend/Cargo.toml --check`,
`cargo clippy --manifest-path backend/Cargo.toml --locked --all-targets -- -D warnings`
y `cargo test --manifest-path backend/Cargo.toml --locked` con PostgreSQL de pruebas.

La prueba del cliente no demuestra el aspecto visual ni todas las operaciones de
esta área. Comprobar los estados afectados y describir en la PR qué se verificó.
No editar contratos comunes o migraciones sin coordinar sus consumidores.
