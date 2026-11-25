# Área Inicio

Selector de servicios, actividad reciente y único formulario de acceso. Mantener Inicio sin barra lateral y el acceso sin opciones privadas.

- Carpeta principal: `frontend/home/`.
- Código relacionado: `backend/src/core/frontend.rs y backend/src/core/app.rs`.
- Puesto local preparado: `.worktrees/inicio/`, rama inicial `codex/inicio`.
- Rama de nuevas tareas: `codex/inicio-<tarea>-<persona>`.
- Guía común: [Colaboración](../../COLABORACION.md).

## Comprobaciones

Desde la raíz del puesto:

```sh
node backend/tests/home-client.cjs
git diff --check
```


La prueba del cliente no demuestra el aspecto visual ni todas las operaciones de
esta área. Comprobar los estados afectados y describir en la PR qué se verificó.
No editar contratos comunes o migraciones sin coordinar sus consumidores.
