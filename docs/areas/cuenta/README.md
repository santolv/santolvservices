# Área Cuenta

Perfil, preferencias y seguridad. Los cambios de autenticación se coordinan con Core. Comprobar manualmente guardar perfil y preferencias en claro/oscuro.

- Carpeta principal: `frontend/account/`.
- Código relacionado: `backend/src/core/auth.rs y backend/src/core/admin.rs`.
- Puesto local preparado: `.worktrees/cuenta/`, rama inicial `codex/cuenta`.
- Rama de nuevas tareas: `codex/cuenta-<tarea>-<persona>`.
- Guía común: [Colaboración](../../COLABORACION.md).

## Comprobaciones

Desde la raíz del puesto:

```sh
node backend/tests/shell-client.cjs
git diff --check
```


La prueba del cliente no demuestra el aspecto visual ni todas las operaciones de
esta área. Comprobar los estados afectados y describir en la PR qué se verificó.
No editar contratos comunes o migraciones sin coordinar sus consumidores.
