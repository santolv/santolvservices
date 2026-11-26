# Trabajo en Santolv Services

- Backend exclusivamente Rust y PostgreSQL. Frontend HTML/CSS/JavaScript y SVG propios, sin añadir frameworks o dependencias innecesarias.
- Este repositorio contiene el producto completo. El backend central está en `backend/`; no crear backends dentro de Drive o Fotos.
- Antes de editar, leer `docs/COLABORACION.md` y la ficha del área en `docs/areas/`. Trabajar desde una carpeta y rama propias. Nunca ejecutar checkout, reset, stash o limpieza sobre el trabajo de otro chat.
- `main` es la integración. Usar ramas `codex/<area>-<tarea>-<persona>` y pull requests a `main`. No enviar directamente a main salvo autorización explícita del usuario. No usar force push ni reescribir historia ajena.
- Los puestos locales `.worktrees/<area>` son copias completas independientes de Git. No son módulos instalados ni nuevas aplicaciones. Una misma carpeta no puede ser editada simultáneamente por dos chats.
- Mantener cada cambio en su área; los cambios a contratos API, shell, motor de archivos, dependencias y migraciones se coordinan según la guía. No alterar otros servicios por conveniencia.
- Usar el sistema de autenticación, permisos y gestor de módulos del Core. Conservar la dependencia Fotos → Drive y los datos privados por usuario.
- La barra superior, subir archivos y ocultar el lateral son componentes de `frontend/shared/`. No duplicarlos en los servicios.
- Conservar colores de tipos de archivo, carpetas, tema global y fototeca 4:3. Los datos reales y operaciones existentes deben seguir funcionando.
- Ejecutar comprobaciones pertinentes, revisar el diff, crear un commit con cambios de la tarea y subir la rama a origin. Informar si no se puede publicar. Para documentación no ejecutar pruebas que necesiten datos reales.
- Para cambios Rust, ejecutar los mismos comandos del workflow vigente: `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings` y `cargo test --locked` desde `backend/`, con PostgreSQL de pruebas. Repetir tras integrar otra rama. No silenciar advertencias para declarar las comprobaciones superadas. Informar de cualquier comprobación fallida o pendiente.
- Un push publica commits, pero no abre una pull request. Al entregar una tarea de código, comprobar que el commit está en la rama remota correspondiente y abrir o actualizar su PR hacia `main`. Informar con la URL real y el estado de CI. Si faltan permisos o acceso, describir el bloqueo sin afirmar que existe una PR.
- Nunca versionar credenciales, originales privados, server.json de instalación, `target/`, `dist/` ni `servidor/`. No compartir bases o almacenamiento entre servidores de desarrollo concurrentes.
- No reinstalar, detener ni reemplazar el servidor de otro puesto. La publicación en localhost compartido se hace desde la integración y con autorización.
- Los árboles históricos `santolvdrive/` y `santolvphotos/` no son el código activo; editar `frontend/` y `backend/`.
