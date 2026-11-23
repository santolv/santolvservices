# Revisión de seguridad de SantolvDrive

> Documento histórico. Para las correcciones posteriores y los riesgos residuales vigentes, consultar [INFORME_SEGURIDAD_02.md](INFORME_SEGURIDAD_02.md).

Fecha: 23 de noviembre de 2026. Alcance: revisión del código Rust, autenticación del HTML, permisos, almacenamiento local y configuración de despliegue. Se han realizado pruebas automatizadas, sin navegador ni capturas. No es una auditoría externa, un pentest completo ni una certificación de ausencia de vulnerabilidades.

## Resultado y decisión de sesiones

El modo recomendado es abrir el HTML desde `http://localhost:5500`, servido por un listener independiente escrito en Rust. La API de `8443` sigue entregando solo datos y contenidos. No se instala ningún programa adicional ni se incorpora ASM. PostgreSQL sigue siendo la excepción ya autorizada.

Las sesiones normales duran **2 horas absolutas**, sin prorrogarse indefinidamente al usarlas. Las credenciales temporales pendientes de cambio de contraseña reciben una sesión de **15 minutos**. La cookie tiene `HttpOnly`, `SameSite=Strict`, `Path=/api/v1`, `Max-Age` y ningún `Domain`. Se añade `Secure` cuando el backend funciona con HTTPS. El servidor comprueba la caducidad en PostgreSQL; modificar la fecha del navegador no prolonga la sesión.

El identificador de sesión contiene 256 bits aleatorios. PostgreSQL conserva su SHA-256, no el token original. La contraseña usa Argon2id con sal aleatoria. Al cerrar sesión se revoca el registro; cambiar o restablecer la contraseña y deshabilitar la cuenta revoca sus sesiones. El superusuario del servidor no puede iniciar una sesión del Drive.

Al recargar, el HTML consulta `/api/v1/me` con la cookie que envía el navegador. Recupera la identidad, la caducidad y un token CSRF estable para esa sesión, sin devolver la cookie ni guardar credenciales en `localStorage` o `sessionStorage`. El token CSRF está separado por contexto criptográfico del identificador de sesión; conocerlo no autentica una petición sin la cookie. Mantenerlo estable evita invalidar otras pestañas. Las sesiones antiguas con un esquema CSRF incompatible requieren iniciar sesión una vez; la migración limita la duración restante de las sesiones anteriores.

Las cookies HttpOnly reducen el robo directo del token desde JavaScript, pero una XSS en el origen de confianza todavía podría ejecutar operaciones como el usuario. `SameSite` no sustituye la validación de origen y CSRF. Véanse [configuración de cookies](https://developer.mozilla.org/en-US/docs/Web/Security/Practical_implementation_guides/Cookies) y [Set-Cookie](https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Set-Cookie).

## Cambios de protección aplicados

- Duración reducida desde las sesiones web previas de hasta siete días a dos horas; sesiones iniciales limitadas a quince minutos.
- Restauración de CSRF tras recargar o abrir otra pestaña, sin almacenar la sesión en JavaScript.
- Rechazo de cookies de sesión duplicadas, encabezados de autorización duplicados y combinación de cookie y bearer. Se limpia la cookie antigua de ámbito `/`.
- Se mantiene el rechazo de cookies con `Origin: null`; no se abre el acceso autenticado a cualquier archivo HTML local.
- Listener del HTML separado, ligado a loopback y a un `Host` exacto. Solo publica `/` e `/index.html`; no expone directorios, configuración ni archivos del almacenamiento.
- CSP con nonce nuevo por respuesta para los scripts, prohibición de incrustación en marcos, `nosniff`, `no-store` y conexiones limitadas al origen de la API configurada. Los estilos en línea siguen permitidos por el diseño actual.

## Hallazgos y riesgos pendientes

La gravedad indica impacto potencial **si se cumplen las condiciones descritas**. No todos los puntos son vulnerabilidades explotables desde Internet en la configuración actual de loopback.

| ID | Prioridad | Estado y condición | Riesgo y medida recomendada |
|---|---|---|---|
| SEG-01 | Alta antes de publicar | Configuración observada: HTTP solo en loopback. Pangolin no está instalado/configurado por este trabajo. | No exponer este HTTP directamente. El acceso público debe usar HTTPS verificado, cookie `Secure`, origen público exacto y protección también de las rutas API. El listener actual rechaza enlazarse a interfaces públicas. |
| SEG-02 | Alta antes del túnel | Confirmado por código: los límites de login usan la IP de la conexión TCP. | Un túnel puede hacer que todos lleguen desde la misma IP y agoten un límite compartido. Antes de publicar, definir qué proxy es de confianza y cómo autentica la IP original. No aceptar `X-Forwarded-For` de cualquiera ni desactivar los límites para compensarlo. |
| SEG-03 | Media | Confirmado por código: 15 peticiones de login por cuenta y 50 por IP en ventanas de 15 minutos; se cuentan también peticiones con contraseña correcta. | Reduce fuerza bruta, pero alguien que conozca un nombre puede agotar su ventana e impedir temporalmente accesos nuevos. Añadir control en el borde, monitorización y una política de retardo progresivo. El rate limit no equivale a MFA ni evita credenciales filtradas. No se ha añadido un bloqueo permanente automático. |
| SEG-04 | Media | Modo bearer de clientes móviles/HTML opaco conservado por compatibilidad de API. La nueva interfaz no lo usa para recordar sesiones. | Un origen `null` no identifica qué archivo local hace la llamada. No existe acceso sin token ni cookies ambientales, pero un HTML malicioso podría intentar phishing de credenciales. Para un despliegue público exclusivo de navegador, retirar este modo o habilitarlo solo mediante una política explícita. |
| SEG-05 | Media, condicionada | Comparación de nombres/rutas y operaciones de disco separadas en `storage.rs` y `workspace.rs`. No se realizó explotación concurrente. | Hay ventanas de comprobación/uso si otro proceso puede modificar el volumen simultáneamente. No dar escritura del directorio a usuarios del sistema ni montar volúmenes compartidos con actores no confiables. Para ese escenario, usar operaciones basadas en handles/directorios y revisar enlaces duros y puntos de reanálisis de Windows. |
| SEG-06 | Media: disponibilidad | Riesgo identificado por lectura de `workspace::owned_directory` y `sync_entry`; no se simuló un corte eléctrico en ese punto. | Una primera publicación interrumpida después de mover bytes y antes de confirmar el registro del directorio puede dejar un directorio no vacío que la reconciliación rechace por seguridad. El archivo puede existir pero quedar pendiente o no descargable. Añadir intención persistente también para la primera publicación y una prueba de reinicio en esa frontera. No es una demostración de fuga de datos. |
| SEG-07 | Media: mantenimiento | No se ejecutó `cargo audit`: no está instalado en el entorno. | No se certifica que las versiones de `Cargo.lock` estén libres de avisos conocidos. Ejecutar un análisis actualizado de [RustSec/cargo-audit](https://github.com/rustsec/rustsec/tree/main/cargo-audit) antes de publicar y tras actualizar dependencias. El escáner es una herramienta de desarrollo en Rust, no una dependencia de ejecución del producto. |
| SEG-08 | Media: automatización | Confirmado: el workflow está en `backend/.github/workflows/ci.yml`, dentro de un repositorio cuya raíz es el directorio superior. | Ese archivo no garantiza que GitHub ejecute los controles en cada push. Ubicar el workflow en `.github/workflows` de la raíz y configurar `working-directory: backend`; revisar permisos y fijación de acciones. Las comprobaciones locales ejecutadas no sustituyen un CI activo. |
| SEG-09 | Media, condicionada | Cookies limitadas por host/ruta, no por puerto. HTTP local carece de cifrado de transporte. | Un proceso local malicioso que actúe con los permisos del usuario está fuera de la protección de HttpOnly; tampoco debe ejecutarse un servicio no confiable en otro puerto de `localhost`. El modo actual sirve para uso local controlado, no para una máquina comprometida. [La política de origen no aísla cookies por puerto](https://developer.mozilla.org/en-US/docs/Web/Security/Defenses/Same-origin_policy). |
| SEG-10 | Baja: defensa adicional | El nuevo listener del HTML valida `Host`; el middleware de la API se apoya principalmente en autenticación, origen y Fetch Metadata. | Añadir una política explícita de hosts para la API al definir el dominio público y el proxy. No se ha demostrado un bypass de autenticación mediante DNS rebinding. |
| SEG-11 | Media: capacidad | Existen límites de concurrencia, de bloques, cuotas y tiempos de petición, pero no se realizó una prueba de saturación completa. | Medir agotamiento de conexiones SQL, sesiones, cargas abandonadas, operaciones por usuario y espacios de gran tamaño. El journal de movimientos puede diferir operaciones si no obtiene una conexión adicional a tiempo. Mantener respuestas 429/503 y reintentos controlados; no aumentar límites sin medir. |

## PostgreSQL y versiones observadas

La consulta `SHOW server_version` del servidor configurado devolvió **19.0**. Los binarios aislados usados por las pruebas informan **17.6**, y el instalador Rust fija un artefacto **17.12**. Son referencias distintas: no debe confundirse la versión del cliente/herramienta de pruebas con la del servidor real. No se han reemplazado binarios ni actualizado la base de datos en este trabajo.

La cadena de versión, por sí sola, no verifica la procedencia del binario ni sus parches. Antes de publicar, contrastar el servidor instalado con los avisos de su proveedor y el [registro oficial de seguridad de PostgreSQL](https://www.postgresql.org/support/security/). Las [notas oficiales de 17.12](https://www.postgresql.org/docs/17/release-17-12.html) describen correcciones de seguridad y consideraciones de actualización de esa rama; no prueban que el servidor que informa 19.0 esté cubierto por ellas.

Huella SHA-256 del `Cargo.lock` revisado: `cb22f2557f41fa9e91c115e837c2439100085e5179c29374faefb797745d5351`.

## Controles existentes revisados

Consultas SQL parametrizadas; roles de espacios y permisos heredados; revocación de cuentas; rutas con identificadores internos y nombres validados; rechazo de enlaces simbólicos en los puntos revisados; directorios privados; comprobación de tamaño/SHA-256 en cargas; contenido descargado como adjunto con `nosniff`; ausencia de previsualización activa de HTML/SVG; bloqueo de cookies en origen opaco; límites de CPU para contraseñas y transferencias; separación entre superusuario local y usuarios del Drive. No se encontró por esta lectura una inyección SQL o una XSS directa demostrada. Eso no acredita cobertura de todos los caminos ni de dependencias transitivas.

No hay recuperación de archivos tras el borrado definitivo ni copias de seguridad automáticas, por decisión de producto. La reconciliación no puede reconstruir bytes perdidos por fallo de disco. Las credenciales de PostgreSQL y claves TLS se conservan en archivos privados del servidor: no publicarlos, no incluirlos en el repositorio y no ejecutarlo habitualmente como administrador del sistema.

## Requisitos para el futuro despliegue con Pangolin

1. Definir un dominio HTTPS real y servir frontend y API bajo el mismo origen público, encaminando `/api/v1` y las descargas a la API; proteger también las rutas de salud o limitar su exposición.
2. Configurar ese origen explícitamente en el backend. La validación local actual **no se debe saltar** ni se debe poner CORS `*` para hacer funcionar el túnel. Esta entrega no implementa un modo de confianza de proxy público ni configura Pangolin.
3. Mantener los listeners internos en loopback y PostgreSQL fuera del túnel. Definir el cifrado/verificación del tramo entre túnel y backend. Activar `Secure` por configuración verificada, sin fiarse de un `X-Forwarded-Proto` arbitrario.
4. Aplicar acceso de Pangolin y autenticación del Drive como capas independientes; exigir MFA si la política/proveedor lo permite. No convertir una cabecera de Pangolin en permisos de administrador del Drive.
5. Verificar IP real, límites de intentos, renovación/caducidad, cierre de sesión y ausencia de accesos que eviten el túnel. Resolver SEG-01, SEG-02 y la comprobación de dependencias antes de darlo por preparado para Internet.

## Límites de la revisión

Resultado de validación de esta entrega: 42 pruebas Rust activas correctas, `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, comprobaciones de lógica del HTML y peticiones HTTP de cookies, restauración de CSRF y revocación. Tres pruebas de instalación y una de carga permanecen ignoradas en la ejecución ordinaria. No se ejecutó `cargo audit`.

Se prueban restauración tras recarga, caducidad del servidor, persistencia SQL, CSRF, credenciales duplicadas/mixtas, orígenes no autorizados, cierre de sesión, permisos y aislamiento del listener estático. Se mantiene la suite de archivos y organizaciones. La validación HTTP no sustituye pruebas en distintos navegadores: políticas de cookies, extensiones y configuraciones corporativas pueden cambiar el comportamiento. No se han ejecutado pruebas visuales, un ataque de carga contra producción, una explotación de CVE ni una auditoría criptográfica externa.
