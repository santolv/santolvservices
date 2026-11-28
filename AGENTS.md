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

## Procedimiento obligatorio de entrega y despliegue

Instrucción permanente del propietario. Prevalece sobre cualquier otra forma de trabajo.

### Ramas y pull requests
- Trabajar desde la carpeta independiente del área en `.worktrees/<area>` y con su rama original `codex/<area>` (por ejemplo `.worktrees/compartido` → `codex/compartido`). No crear ramas nuevas ni reutilizar ramas creadas por agentes para tareas sueltas.
- Antes de editar, actualizar la rama del área con `git fetch origin` y `git merge --ff-only origin/main` (o `git merge origin/main` si ya tiene commits propios).
- Aplicar los cambios, ejecutar las comprobaciones, crear el commit y publicarlo con `git push`.
- Abrir la PR hacia `main`, revisar el diff y esperar a que GitHub Actions termine en verde.
- Integrar la PR en `main` y actualizar la carpeta principal con `git switch main` y `git pull --ff-only origin main`.

### Producción
- Producción es la instalación de `backend/servidor/`: interfaz en `http://localhost:5500` y API en `http://localhost:8443` (`bind` `127.0.0.1:8443`). Es el entorno que usa el propietario para probar la suite.
- No crear entornos alternativos, otras instancias ni otros puertos. En `localhost`, la cookie de sesión `drive_session` se comparte entre puertos: otra instancia sustituye la sesión del propietario y hace que deje de entrar.
- Desplegar siempre desde `main` ya integrado, desde la carpeta principal:
  1. `cargo build --release --locked` en `backend/`.
  2. Detener el servidor de producción (`taskkill /PID <PID de Santolv> /T /F`, que cierra también su PostgreSQL). Si PostgreSQL sigue activo, ejecutar `.\servidor\santolvdrive.exe database-stop`.
  3. Con todo detenido, copiar `data/`, `postgres/`, `santolvdrive.exe`, `server.json` y `postgres-state.json` a `backend/servidor/backup-<tarea>-<AAAAMMDD-HHMMSS>/`.
  4. Sustituir `backend/servidor/santolvdrive.exe` por `backend/target/release/santolvdrive.exe`.
  5. Arrancar `.\servidor\santolvdrive.exe serve` desde `backend/`, en segundo plano y con su salida en `servidor/arranque-<AAAAMMDD-HHMMSS>.out.log` y `.err.log`.
  6. Comprobar `http://127.0.0.1:8443/health/ready`, `http://localhost:5500/`, el inicio de sesión del propietario y los servicios afectados.
- Conservar siempre la base de datos, los usuarios, los archivos, las credenciales y `server.json`. No reinicializar la base ni borrar datos.
- Una tarea solo está terminada cuando está integrada en `main`, desplegada en producción, accesible en 5500/8443 y comprobada. Si algo lo impide, decirlo claramente sin dar el despliegue por hecho.
