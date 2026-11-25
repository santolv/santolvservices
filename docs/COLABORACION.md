# Colaboración por áreas

Un repositorio, un backend modular y una rama de integración: `main`.
GitHub conserva e integra el código; las sesiones del servidor y sus datos no se
sincronizan a través de Git.

## Carpetas y responsabilidad

| Área | Interfaz | Rust / punto de integración | Ficha |
| --- | --- | --- | --- |
| Inicio | frontend/home/ | backend/src/core/frontend.rs y app.rs (coordinados con Core) | [Inicio](areas/inicio/README.md) |
| Cuenta | frontend/account/ | backend/src/core/auth.rs y admin.rs (coordinados con Core) | [Cuenta](areas/cuenta/README.md) |
| Drive | frontend/drive/ | backend/src/modules/drive/ | [Drive](areas/drive/README.md) |
| Fotos | frontend/photos/ | backend/src/modules/photos/ | [Fotos](areas/fotos/README.md) |
| Core | — | backend/src/core/ y arranque/instalación | [Core](areas/core/README.md) |
| Compartido | frontend/shared/ | backend/src/file_store/ | [Compartido](areas/compartido/README.md) |

Cada servicio conserva sus rutas y lógica. Core gobierna autenticación y permisos;
el motor de archivos es compartido. Inicio y Cuenta son pantallas del Core, no
servicios con otro servidor. No hace falta mover el código para separarlo de nuevo.

## Dos chats en este ordenador

Se preparan seis puestos en `.worktrees/`: inicio, cuenta, drive, fotos, core y
compartido. Cada puesto contiene el repositorio completo en una rama propia
`codex/<area>`. Abre o selecciona esa carpeta como proyecto al iniciar el chat;
mencionar el área sin cambiar de carpeta no aísla los archivos.

Ejemplo: un chat en `.worktrees/inicio` y otro en `.worktrees/fotos`.
Los commits no cambian los archivos del otro puesto. Estas ramas iniciales
pueden usarse para la primera tarea; para tareas posteriores crea una rama nueva.
Las carpetas locales no se suben a GitHub y no crean chats automáticamente.

Antes de abrir una tarea nueva, con el puesto limpio:

```sh
git fetch origin
git switch -c codex/fotos-ajuste-ana origin/main
```

Si dos chats trabajan en la misma área, necesitan DOS puestos y DOS ramas.
Desde la raíz, crea otro puesto (el nombre es un ejemplo):

```sh
git fetch origin
git worktree add -b codex/fotos-albumes-luis .worktrees/fotos-luis origin/main
git worktree list
```

No compartir una rama entre dos puestos. Antes de retirar un puesto, comprobar
que sus cambios están confirmados y publicados; no usar eliminación forzada.

## Dos personas en distintos ordenadores

Cada persona clona su copia y usa su propia cuenta de GitHub con permiso de
escritura en el repositorio. El propietario debe conceder ese acceso en GitHub;
las carpetas y ramas no conceden permisos.

```sh
git clone https://github.com/santolv/santolvservices.git
cd santolvservices
git switch -c codex/drive-busqueda-luis
```

Los comandos Git sirven en Windows, Linux y macOS. Para compilar hace falta
Rust; las pruebas de integración usan un PostgreSQL de pruebas.

## Integrar una tarea

1. Trabajar en la rama de la tarea y limitar el diff a su área.
2. Ejecutar sus comprobaciones y revisar `git diff --check`.
3. Confirmar y publicar la rama:

```sh
git add <archivos-de-la-tarea>
git commit -m "feat: describe el cambio"
git push -u origin HEAD
```

4. En GitHub abrir una pull request hacia `main`. La plantilla recoge área,
   contratos afectados y validación. El workflow existente comprueba Rust,
   PostgreSQL y los clientes de Inicio, Drive, Fotos y shell.
5. Antes de integrar, traer la última integración a la rama:

```sh
git fetch origin
git merge origin/main
```

Resolver allí los conflictos, repetir las pruebas afectadas y hacer push.
Integrar la PR cuando sus comprobaciones pasen. Git combina cambios compatibles,
pero no puede decidir qué comportamiento debe ganar si ambos cambian lo mismo.
No se ha activado un bloqueo obligatorio de main desde esta preparación:
la guía y la plantilla no sustituyen las reglas de protección de GitHub.

El puesto raíz se reserva para revisar/integrar. Después de integrar una PR,
actualizarlo con `git switch main` y `git pull --ff-only origin main`, siempre
que esté limpio. Los otros puestos no cambian hasta traer y mezclar main.

## Archivos donde hay que coordinarse

- `frontend/shared/`: barra superior, tema, botones, sesión y navegación.
- `backend/src/core/`: contratos de identidad, API, permisos y catálogo.
- `backend/src/file_store/`: archivos, cargas, cuotas, rutas físicas y trabajos.
- `backend/src/modules/mod.rs`: registro de servicios.
- `backend/src/lib.rs`, `backend/Cargo.toml` y `Cargo.lock`: ensamblado y dependencias.
- `backend/migrations/`: numeración global; reservar el siguiente número entre
  las tareas. Nunca editar una migración ya aplicada. Si dos ramas añaden el
  mismo número, resolverlo antes de que cualquiera se aplique a datos compartidos.
- `backend/tests/backend.rs`: fichero común; mantener pequeñas las adiciones.
- `README.md` y workflows: cambios de integración, sin reformatear secciones ajenas.

Una tarea que cambia una API debe incluir servidor, consumidores y pruebas
en la misma PR, o mantener compatibilidad hasta que se integre la siguiente.
No renombrar campos o rutas que otro módulo todavía utiliza.

## Probar sin interferir

- Cada worktree tiene su propio `backend/target/`; no configurar un target
  común si se compilan versiones diferentes a la vez.
- Los HTML están incluidos en el ejecutable Rust: cambiar un HTML requiere
  recompilar el servidor usado para verlo. No copiar ejecutables desde ramas
  sin integrar sobre el localhost compartido.
- Dos servidores de desarrollo necesitan distintos puertos API/frontend,
  configuración, base PostgreSQL y almacenamiento. No copiar la configuración
  privada del servidor instalado a Git. La configuración debe respetar sus
  orígenes y las cookies de cada servidor.
- Las pruebas de backend se ejecutan contra una base de pruebas con permisos
  para crear bases, nunca contra la instalación real.
- La instalación central `backend/servidor/` y sus datos quedan fuera de Git;
  los worktrees nuevos no incluyen una instalación ni usuarios de pruebas.

## Texto para iniciar un chat

> Trabaja en el área Fotos de esta carpeta. Lee AGENTS.md y
> docs/areas/fotos/README.md. Implementa [tarea] en una rama propia basada en
> origin/main. Coordina cualquier cambio al Core, motor de archivos o shell.
> Ejecuta las comprobaciones, crea commit y publica la rama para una PR a main.
