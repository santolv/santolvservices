# Segunda revisión de seguridad de SantolvDrive

Fecha: 23 de noviembre de 2026. Sustituye las conclusiones pendientes del [primer informe](INFORME_SEGURIDAD.md) para esta versión. Se ha revisado y endurecido la instalación **local**. Esto no es una certificación, ni demuestra ausencia de fallos desconocidos. No se ha configurado ni publicado Pangolin.

## Correcciones aplicadas

| Referencia anterior | Resultado de esta revisión |
|---|---|
| SEG-01: publicación del HTTP local | Se mantiene exclusivamente loopback. El backend no adquiere confianza en HTTPS, identidad o IP a partir de cabeceras externas. No se ha abierto acceso a Internet. El dominio público y su terminación TLS siguen siendo un requisito de despliegue pendiente. |
| SEG-02: IP tras un túnel | Se rechazan `Forwarded`, `X-Forwarded-For`, `X-Forwarded-Host`, `X-Forwarded-Proto` y `X-Real-IP` con 403. No se aceptan valores falsificados ni se interpreta como real una IP proporcionada por el cliente. Un futuro túnel requiere un modo explícito de confianza y datos de su configuración; no funcionará simplemente saltándose esta protección. |
| SEG-03: fuerza bruta y bloqueo por nombre | Se retira el límite que cualquier IP podía agotar contra una cuenta. Los fallos se asocian al par IP + hash del nombre y generan esperas de 1, 2, 4 y hasta 8 segundos. Se responde 429 durante la espera, sin mantener tareas durmiendo. Una identificación correcta limpia ese historial. Permanece el límite de 50 peticiones de login por IP/15 minutos y el límite de dos operaciones Argon2 concurrentes. Se registran fallos sin contraseñas, cookies ni nombre de cuenta en claro. |
| SEG-04: origen opaco | Deshabilitado: se rechaza todo `Origin: null`, se retira de CORS y `local_file:true` ya no permite iniciar sesión. El HTML abierto con doble clic dirige al usuario a la interfaz HTTP local. Los clientes nativos conservan bearer sin `Origin` ni Fetch Metadata; una página web no puede activar ese modo. |
| SEG-05: rutas/enlaces | Las comprobaciones de rutas detectan también puntos de reanálisis de Windows, además de enlaces simbólicos. El cálculo de checksum rechaza entradas que no sean archivos normales y esos enlaces. Se conserva la exigencia de volumen privado. Esto refuerza la protección, pero **no elimina todas las carreras frente a otro proceso con permisos de escritura**, descritas abajo. |
| SEG-06: primera publicación interrumpida | Nueva tabla `workspace_publications`: la intención y el destino se confirman en PostgreSQL **antes** de crear el directorio y mover los bytes. Tras reiniciar se reanuda la publicación, se verifica el checksum si los bytes ya están publicados y se retira la intención al confirmar el registro de ubicación. Los destinos ajenos, incluso existentes antes de la intención, se rechazan. El reconciliador no marca como perdido un objeto cuya publicación esté pendiente. Incluye una prueba que simula bytes publicados sin el commit del registro y ejecuta el mantenimiento posterior. |
| SEG-07: dependencias sin revisar | Se ejecutó `cargo-audit 0.23.0`. Detectó **RUSTSEC-2026-0592** en `url 2.5.8`; se actualizó a `2.5.9` y su dependencia `form_urlencoded` a `1.2.3`. Se exige la versión mínima corregida en Cargo.toml. La nueva auditoría termina sin vulnerabilidades ni advertencias conocidas. |
| SEG-08: CI fuera de la raíz | Workflow trasladado a `.github/workflows/ci.yml` de la raíz del repositorio; directorio de ejecución `backend`, permisos `contents: read`, credenciales de checkout no persistidas, acciones fijadas por SHA, límite de ejecución y cancelación de ejecuciones obsoletas. Añadidas comprobaciones de frontend y `cargo audit --deny warnings`. El estado de una ejecución remota debe comprobarse en GitHub; no se presenta como un resultado local. |
| SEG-09: cookies HTTP locales | Se conserva la cookie HttpOnly breve de dos horas y se mantiene el listener privado. No se simula aislamiento de cookies por puerto ni se oculta la ausencia de TLS local. El riesgo de un equipo local comprometido no se resuelve con atributos de cookie. |
| SEG-10: Host de la API | Se exige un único `Host` autorizado. Se acepta el origen configurado y, solo para una instalación loopback, sus nombres locales con el puerto correcto. Se rechazan cabeceras duplicadas y hosts inesperados. El listener del HTML mantiene su propia validación. |
| SEG-11: agotamiento de recursos | Máximo de 10 sesiones por usuario; al crear otra se revocan las anteriores que excedan el límite. Máximo de 32 cargas pendientes por usuario y espacio. Se conservan cuotas, concurrencia y expiración. Se ejecutó una medición aislada de listados; no equivale a una certificación de capacidad. |

Todo el código de ejecución incorporado es Rust; no se añadió ASM. `cargo-audit` es una herramienta de desarrollo escrita en Rust. Node ya existente se usa exclusivamente para pruebas del HTML. PostgreSQL sigue siendo la excepción de ejecución autorizada.

## Dependencia vulnerable corregida

[RUSTSEC-2026-0592](https://rustsec.org/advisories/RUSTSEC-2026-0592.html) describe una denegación de servicio por desbordamiento de pila al obtener el origen de URLs `blob:` anidadas con versiones de `url` anteriores a 2.5.9. Se encontró una versión afectada en Cargo.lock; **no se demostró que una petición remota del Drive alcanzara esa ruta**. Se actualizó preventivamente, sin afirmar una explotación no verificada.

Resultado posterior: **0 vulnerabilidades y 0 advertencias conocidas**, 282 dependencias analizadas, base RustSec de 1.563 avisos, commit `52384ab5581a4487b62c549eef95cf7ba7e8bad5`. Comando: `cargo audit --deny warnings`. El resultado caduca cuando aparecen nuevos avisos o cambian las dependencias.

SHA-256 del Cargo.lock revisado: `93c6f5cee59cb72f1edccc871c7fad034c0299ab180eb4c61ace7f88ec7d2a86`.

## Pruebas y alcance

- Suite Rust: 44 pruebas activas, incluyendo aislamiento de cuentas, permisos, expiración, CSRF, rechazo de origen opaco/hosts/cabeceras de proxy, esperas tras errores de contraseña, límites de sesiones/cargas y recuperación de la publicación. Las pruebas de instalación que descargan/provisionan programas no forman parte de esta ejecución ordinaria.
- Comprobaciones del HTML mediante Node, sin navegador ni capturas. Prueba HTTP real de acceso por cookie y de cliente nativo, subida, descarga, papelera, borrado y listener estático separado.
- `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings` y auditoría RustSec.
- En la comprobación final se corrigió además el orden de validación en ediciones simultáneas: se comprueba el ETag antes de la sincronización pendiente, para que una revisión obsoleta reciba 412 de manera predecible y no quede enmascarada por un 409 de disco.
- Medición **debug**, en PostgreSQL aislado, 10.000 nodos, concurrencia 16 y 400 peticiones: 3,026 s, p50 115,331 ms, p95 151,808 ms y p99 179,029 ms. Es una línea base local obtenida mediante el router en proceso, no rendimiento de red ni de un VPS real. No se han medido exhaustivamente transferencias, saturación de disco ni todos los vectores de DoS.

## Posibles fallos que siguen existiendo

| ID | Prioridad | Condición, efecto y siguiente medida |
|---|---|---|
| RES-01 | Alta antes de Internet | **Falta el despliegue público real.** No hay dominio ni configuración de Pangolin verificados. Mantener acceso local. Para publicar, fijar dominio HTTPS, routing de frontend/API y controles del túnel, verificar cookie Secure y definir proxy/IP de confianza. Esta entrega rechaza las cabeceras de proxy en vez de confiar en ellas. |
| RES-02 | Media | **Fuerza bruta distribuida y credenciales filtradas.** La espera por IP/cuenta no impide intentos desde muchas IP ni usar una contraseña robada. Añadir MFA en el acceso público y límites en el borde; medir umbrales por organización. No se ha implementado una autenticación fuerte multifactor propia ni una detección de contraseñas filtradas. |
| RES-03 | Media | **DoS desde una IP compartida.** Cincuenta peticiones de login/15 minutos incluyen las correctas. Una máquina o salida compartida puede agotarlas. Las sesiones ya abiertas no se revocan por ello. Ajustar la política al dimensionar el despliegue; no interpretar como IP cliente una cabecera no autenticada. |
| RES-04 | Media, condicionada | **Carreras en el disco contra procesos con acceso al volumen.** Comprobar enlaces y después abrir/mover no constituye una operación indivisible. Otro proceso con los mismos permisos del servicio podría sustituir una ruta o introducir enlaces duros. Ejecutar con cuenta dedicada y volumen privado, sin dar acceso a scripts externos no confiables. Para volúmenes compartidos hostiles se necesita rediseñar las operaciones sobre handles/directorios por plataforma y probarlo específicamente. No se ha demostrado explotación remota de esta condición. |
| RES-05 | Media | **XSS en el origen confiable o modificación del HTML local.** CSP con nonce, escape de contenido y HttpOnly reducen superficie; una XSS que ejecute dentro del origen autorizado aún podría hacer operaciones con la sesión. Proteger archivos del frontend y revisar cualquier nueva inserción de HTML. Los estilos inline siguen permitidos. No se han probado todas las combinaciones de navegadores, extensiones o políticas corporativas. |
| RES-06 | Media: disponibilidad | **Durabilidad ante corte eléctrico.** La prueba de recuperación simula una frontera concreta del commit; no es una matriz de todos los cortes posibles. En Windows los directorios no se sincronizan como en Linux. Una avería de disco o corrupción persistente puede impedir descargar; no se generan copias de seguridad ni se reconstruyen bytes perdidos, de acuerdo con la decisión del producto. |
| RES-07 | Media: capacidad | **Saturación y equidad incompletamente medidas.** Los límites evitan crecimiento ilimitado de sesiones activas/cargas por espacio, pero un usuario con permisos sobre muchos espacios puede multiplicar sus cargas, consumir CPU/IO o provocar contención SQL. El journal puede quedar pendiente si no obtiene una segunda conexión en 250 ms; se reintenta. Añadir pruebas de carga reales y presupuestos globales por cuenta al definir un volumen objetivo. |
| RES-08 | Media: ciclo de vida | **La auditoría de dependencias solo conoce avisos publicados.** Puede haber vulnerabilidades desconocidas y regresiones futuras. Mantener Cargo.lock, revisar actualizaciones y comprobar resultados de CI. No desactivar `cargo audit` ni ignorar avisos sin análisis documentado. |
| RES-09 | Media, local | **Cookies por host, no por puerto.** Un servicio local no confiable o software con los permisos del usuario puede interferir con localhost; no se promete seguridad frente a un equipo comprometido. HTTPS público con dominio dedicado reduce la superficie de otros servicios, pero no remedia malware local. |

No se anuncian estos riesgos como corregidos cuando dependen de datos del túnel todavía inexistentes, del sistema operativo, del hardware o de una prueba que no se ha ejecutado.

## PostgreSQL

El servidor configurado informó 19.0 en la revisión anterior. La [política oficial consultada](https://www.postgresql.org/support/versioning/) identifica 19.0 como versión mantenida y vigente de la rama 19 a esta fecha; no corresponde reemplazarla por el binario 17.6 usado en las pruebas. El instalador fija 17.12 para una instalación nueva de su rama soportada. No se ha cambiado el clúster de datos ni se ha efectuado una actualización mayor innecesaria. La versión informada no acredita por sí sola procedencia, integridad del binario ni configuración del sistema.

## Recomendaciones de operación

Abrir el Drive en `http://localhost:5500/` y mantener API, frontend y PostgreSQL inaccesibles desde interfaces públicas. Conservar la cuenta de servicio separada y las ACL de sus directorios. Vigilar 429, errores de disco y tareas de publicación pendientes. Antes de abrir el túnel se necesita completar RES-01 con la configuración concreta, sin quitar controles de origen, host o proxy para resolver errores de conectividad.
