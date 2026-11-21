# Validación del backend nativo — 20 de noviembre de 2026

Entorno local: Windows, Rust 1.99.1, PostgreSQL 17.6 temporal en loopback. PostgreSQL es la única excepción externa autorizada. No se instaló ni ejecutó Docker, ningún proxy ni un gestor de paquetes para comprobar el producto.

| Comprobación | Resultado |
| --- | --- |
| Compilación y Clippy | Correctos |
| Suite con PostgreSQL real | 13 pruebas unitarias y 7 de integración correctas |
| `cargo audit` | Sin vulnerabilidades ni avisos en el lockfile actualizado |
| HTTPS real con Rustls | Handshake, petición y respuesta correctos con un certificado confiado explícitamente solo en el cliente de prueba |
| Handshake incompleto en otro cliente | No impide que un cliente TLS válido complete su petición |
| Certificado o PEM inválido / clave no concordante | Rechazados; no se hace fallback a HTTP |
| Login real sobre HTTPS | Correcto; cabecera HSTS presente y dirección de conexión conservada |
| Cabecera X-Forwarded-For falsa | No cambia la IP empleada por los límites de login |
| Configuración insegura | HTTP fuera de loopback rechazado; TLS requiere certificado y clave |
| Permisos nativos de directorio | Probados en Windows sin ejecutar whoami ni icacls |
| Escritura de configuración | Reanudable para contenido idéntico; rechaza sobrescribir contenido diferente |

Se mantienen las pruebas de administración separada, contraseñas temporales, desactivación y bloqueo de publicaciones, sesiones, CSRF, permisos, idempotencia, cuotas, cargas reanudables, conflictos de versiones, papelera, reconciliación y migración con datos existentes. El administrador no tiene espacio personal y no puede acceder a archivos mediante su sesión.

Las pruebas TLS generan certificados temporalmente con código Rust; no usan OpenSSL ni un servicio de certificados. Las pruebas SQL crean bases aisladas y aplican las mismas migraciones. No se reescribió el modelo de datos ni se convirtió PostgreSQL a otro motor.

Las comprobaciones anteriores de copia/restauración de PostgreSQL y archivos siguen siendo antecedentes válidos del almacenamiento; esta sesión se centra en retirar los programas de despliegue y servir TLS directamente. El benchmark de listados en proceso no certifica rendimiento del nuevo transporte HTTPS ni de un VPS.

## Límites pendientes

No se ha probado el instalador interactivo completo en servidores Linux/macOS ni se ha ejecutado el workflow remoto. No se ha configurado un certificado público ni emisión/renovación ACME: el administrador aporta y renueva los PEM. No se ha instalado un servicio automático de sistema. No se simularon cortes eléctricos reales ni un disco físicamente lleno; la garantía de sincronización de directorios sigue requiriendo validación en el sistema de archivos de producción.

El código de producto es Rust, con consultas/migraciones SQL para PostgreSQL y configuración declarativa de CI. Se eliminaron los scripts y archivos de contenedores/proxies y las invocaciones de programas externos desde el instalador. Las bibliotecas usan APIs nativas del sistema; no se afirma que todas las dependencias transitivas carezcan de código nativo C/ensamblador, por ejemplo el proveedor criptográfico utilizado por Rustls.
Artefacto Windows actualizado: `dist/santolvdrive-windows-x64.exe`, compilado release con CRT estático y acompañado de SHA-256. `install --check` confirma runtime nativo, HTTPS Rustls, PostgreSQL como excepción y ningún programa externo instalado. PostgreSQL de pruebas quedó detenido al terminar. La generación de paquetes de otros sistemas queda preparada en CI; todavía no se certifica su ejecución en esos sistemas.
