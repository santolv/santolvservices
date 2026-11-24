# SantolvPhotos: plan de interfaz multiplataforma

Actualizado: 24 de noviembre de 2026.

## Objetivo y decisiones acordadas

Crear una galería de fotos y vídeos autohospedada, inspirada en Google Fotos e
Immich, accesible desde ordenador, móvil y tableta mediante los servidores del
propietario. La interfaz tendrá identidad visual propia.

Se utilizará **el mismo sistema de SantolvDrive**:

- Backend escrito en Rust.
- Interfaz HTML con CSS y JavaScript propios, sin framework frontend.
- Comunicación con la API por HTTP/JSON y transferencias de archivos.
- Recursos propios servidos localmente, sin dependencias de CDN.
- Cookies HttpOnly y protección CSRF como referencia para las sesiones web.

La aclaración del propietario autoriza HTML, CSS y JavaScript para la interfaz.
El requisito de Rust corresponde al backend. No hace falta volver a pedir permiso
por esos lenguajes. Otros lenguajes o un cambio importante de sistema sí se consultarán.

El trabajo inmediato sigue siendo la interfaz de `frontend/`. Aquí se documenta
también cómo encaja en el producto completo y qué funcionalidades dependerán del
servidor, pero no se implementa ni se detalla todavía un plan interno del backend.
No se necesita Leptos, Trunk, WebAssembly, React, Vue, TypeScript ni un gestor npm
para ejecutar la primera interfaz.

## Base comprobada en SantolvDrive

Repositorio: https://github.com/santolv/santolvdrive

Revisión consultada: `b9c19e100e15c85534ede1a38e331411d4d2edad`.

| Archivo | Qué utiliza | Qué tomaremos como base |
| --- | --- | --- |
| `backend/Cargo.toml` | Rust 2024, mínimo declarado 1.94 | Backend Rust; fijar versiones compatibles al implementarlo |
| El mismo manifiesto | Axum 0.8, Tokio 1, Tower 0.5, tower-http 0.6 | API, ejecución asíncrona y middleware |
| El mismo manifiesto | SQLx 0.9 con PostgreSQL; Serde 1, serde_json 1, uuid 1, time 0.3 | Persistencia y contratos de datos, para la futura fase del servidor |
| El mismo manifiesto | tokio-rustls 0.26, Argon2 0.5 y sha2 0.10 | Referencia de TLS, contraseñas e integridad de transferencias |
| `frontend/pruebahtml/index.html` y su README | HTML con CSS, JavaScript e iconos SVG incluidos, sin framework | Mismo formato inicial y APIs web estándar |
| `backend/src/frontend.rs` | Listener estático Rust separado de la API, CSP con nonce, validación de Host | Separación de responsabilidades y política de recursos explícita |

No se importará todo el backend de Drive ni se supondrá que sus endpoints de
archivos ya resuelven fotos, miniaturas, EXIF o álbumes. Se reutiliza el enfoque
y se evalúa cada librería cuando corresponda; no se instalan dependencias de
servidor para construir una maqueta de interfaz.

### Diferencia necesaria para acceder desde otros dispositivos

El listener actual de Drive está limitado a localhost, sirve solo `/` e
`/index.html` y limita el HTML a 1 MiB. Es adecuado como referencia local,
pero un móvil no puede acceder al localhost del ordenador.

Para el producto autohospedado se necesitará una dirección del servidor accesible
desde los dispositivos, HTTPS y configuración explícita de host/origen/proxy.
Se prevé una sola dirección HTTPS para el usuario, con encaminamiento interno
separado para interfaz y API. El servidor continuará en Rust; un proxy de entrada
es una opción de despliegue que se concretará más adelante.

La primera interfaz seguirá siendo un HTML independiente con estilos y lógica
incluidos. La fase PWA exigirá además manifiesto, iconos y, para la experiencia
sin conexión propuesta, un service worker: el servidor deberá permitir esos
recursos concretos. No se expondrá una carpeta arbitraria del disco.

## Qué significa multiplataforma

La primera entrega será una web adaptable compartida por todas las plataformas.
Posteriormente podrá instalarse como PWA donde el sistema y navegador lo permitan.
Instalar una PWA no convierte la web en un cliente nativo de sincronización.

| Plataforma | Primera entrega | Evolución prevista | Verificación |
| --- | --- | --- | --- |
| Windows | Navegador de escritorio | PWA donde se admita | Edge, Chrome y Firefox para la web |
| Linux | Navegador de escritorio | PWA según navegador/entorno | Chrome/Chromium y Firefox |
| macOS | Navegador de escritorio | Instalación web donde se admita | Safari y Chrome |
| Android | Web móvil con selector de fotos y vídeo | PWA y acceso desde pantalla de inicio | Chrome, tacto y selección múltiple |
| iPhone/iPad | Web adaptable en Safari | Añadir a inicio donde esté disponible | Safari, áreas seguras, orientación y teclado |

La matriz es un objetivo de comprobación, no una afirmación de que todos los
navegadores admitan las mismas funciones. Una función no soportada debe tener
alternativa o mostrar su limitación sin impedir consultar la biblioteca.

**Límite importante:** la web puede subir archivos seleccionados por el usuario,
pero no se prometerá acceso permanente al carrete ni copia automática fiable con
la aplicación cerrada. La actividad en segundo plano está limitada por el navegador
y el sistema. Para una experiencia de copia móvil comparable a Immich habrá una
fase independiente de clientes móviles, permisos y tareas de segundo plano.
Su tecnología no se elige todavía; se consultará antes de añadir otros lenguajes.

Un posible cliente de escritorio para vigilar carpetas y subir automáticamente
también quedará para después. No es necesario para consultar o subir fotos desde
el navegador en Windows, Linux o macOS.

## Arquitectura prevista del producto

```text
Ordenador / móvil / tableta
         |
         | HTTPS: HTML + CSS + JavaScript y llamadas a la API
         v
Dirección del servidor propio
         |-- Interfaz estática (servida por Rust)
         `-- API Rust: Axum / Tokio
                 |-- PostgreSQL: usuarios y metadatos
                 |-- Disco: originales y recursos derivados
                 `-- Trabajos de medios: miniaturas e indexación (fase futura)
```

Los originales se conservarán independientes de las miniaturas y las
previsualizaciones. La interfaz pedirá miniaturas para la cuadrícula y versiones
adecuadas para el visor; no descargará toda la colección original al abrir Fotos.
La generación de esos derivados corresponde al servidor futuro.

La interfaz tendrá una capa de acceso a datos sustituible: primero proveedor
de demostración, después cliente real. Los botones no mostrarán una operación
como guardada, subida o respaldada hasta recibir confirmación del servidor.

## Tecnologías de la interfaz

| Tecnología | Uso |
| --- | --- |
| HTML semántico | Estructura, formularios, navegación y diálogos |
| CSS propio: Grid, Flexbox y variables | Cuadrícula fotográfica, temas y adaptación de tamaños |
| JavaScript estándar | Estado, selección, navegación, visor, búsqueda y llamadas a la API |
| Fetch y, si se necesita progreso de subida, XMLHttpRequest | Consultas y transferencias sin librerías de cliente |
| File/Blob y URLs temporales | Selección y previsualización local con liberación de recursos |
| SVG locales Lucide/Feather | Iconos con sus licencias, siguiendo Drive |
| JSON y manifiesto web | Contratos de datos y futura instalación PWA |
| Service worker, en una fase posterior | Caché limitada de la interfaz y aviso de actualizaciones |

Empezar como Drive: `frontend/index.html` con `<style>` y `<script>`, organizado
internamente por responsabilidades. Las muestras y licencias estarán en recursos
locales. Si el crecimiento exige separar CSS/JS, se conservará el mismo sistema
sin framework y se coordinarán rutas y CSP con el servidor Rust.

La navegación inicial usará fragmentos (`#/photos`, `#/albums`, etc.) para
permitir recargas y enlaces sin exigir reescritura de todas las rutas del servidor.
Los nombres y metadatos se insertarán como texto, sin interpretar HTML recibido.

## Pantallas y experiencia

| Pantalla | Funciones iniciales |
| --- | --- |
| Acceso | Formulario, estado de conexión, errores y sesión caducada |
| Fotos | Cronología por día/mes, cuadrícula, selección y acceso al visor |
| Buscar | Nombre, fechas, tipo de medio y favoritos; indicar si el resultado es local o global |
| Álbumes | Listado, portada, creación, renombrado y gestión de fotos |
| Favoritos | Colección marcada por el usuario |
| Papelera | Restaurar y confirmar eliminación definitiva |
| Subidas | Cola, progreso, cancelación, errores y reintento |
| Ajustes | Tema, preferencias visuales, sesión y almacenamiento cuando exista API |
| Visor | Foto/vídeo, anterior/siguiente, zoom e información disponible |

En escritorio: barra lateral, búsqueda superior y cuadrícula amplia. En móvil:
navegación compacta, acciones alcanzables con el pulgar y controles táctiles.
Mantener modo claro/oscuro, tipografía del sistema y foco visible. No depender
de hover para acceder a una acción.

El visor conservará posición y filtros al cerrarse. Permitirá Escape, flechas,
gestos compatibles y retorno del foco al elemento de origen. Los vídeos mostrarán
su duración cuando se conozca y un mensaje útil si el formato no es reproducible.
No se garantizará HEIC, RAW o todos los códecs sin derivados del servidor.

## Fases de trabajo

### 0. Base y planificación — completada

- [x] Revisar el sistema real de SantolvDrive.
- [x] Confirmar backend Rust y frontend HTML/CSS/JavaScript.
- [x] Definir alcance web, evolución PWA y límites de sincronización móvil.
- [x] Guardar el plan y la regla de commit local más push al finalizar.

### 1. Estructura visual adaptable

- [ ] Crear `frontend/index.html`, menú, cabecera, navegación y estilos comunes.
- [ ] Preparar temas claro/oscuro, diálogos, menús y notificaciones.
- [ ] Diseñar Fotos, Álbumes, Favoritos, Papelera y Ajustes.
- [ ] Preparar muestras locales con licencia o permiso, sin fotos privadas.

**Entrega:** interfaz navegable en escritorio y móvil, con datos de demostración
identificados, sin desbordamiento horizontal y utilizable mediante teclado.

### 2. Biblioteca y visor

- [ ] Cuadrícula por días y meses, fotos verticales/horizontales y vídeos.
- [ ] Fecha de captura con alternativa explícita cuando falte; ordenar de forma estable.
- [ ] Carga diferida de miniaturas y paginación de ejemplo; valorar virtualización
  según medición con una colección grande de metadatos.
- [ ] Visor con navegación, zoom, reproducción e información.
- [ ] Estados vacío, cargando, imagen fallida, formato no compatible y sin conexión.

**Entrega:** recorrer y abrir medios sin cargar todos los originales, conservando
posición, filtros y foco al volver a la biblioteca.

### 3. Organización y búsqueda

- [ ] Selección individual/múltiple con teclado y tacto.
- [ ] Favoritos, álbumes y búsqueda por nombre/fecha/tipo sobre la colección demo.
- [ ] Papelera, restauración y confirmación de borrado, todo solo en memoria demo.
- [ ] Mostrar si la búsqueda cubre solo elementos cargados; no presentarla como
  búsqueda global hasta tener API para ello.

**Entrega:** las vistas mantienen un estado coherente y la interfaz explica
que los cambios demo no están guardados en un servidor.

### 4. Experiencia de subida y cuenta

- [ ] Selector múltiple, arrastrar/soltar y previsualización local.
- [ ] Cola, cancelación, progreso y errores, claramente simulados en modo demo.
- [ ] Preparar flujos de sesión caducada, permisos insuficientes y cuota agotada.
- [ ] Evitar leer vídeos completos en memoria y liberar URLs al retirar previsualizaciones.

**Entrega:** el flujo completo puede revisarse sin API; ninguna subida simulada
se anuncia como copia de seguridad real.

### 5. Integración con el backend — dependiente de una fase posterior del servidor

- [ ] Documentar contratos de medios, álbumes, paginación, sesión y transferencias.
- [ ] Sustituir el proveedor demo por HTTP manteniendo componentes y estados.
- [ ] Integrar cookies HttpOnly, recuperación de sesión y CSRF, sin tokens en
  localStorage/sessionStorage ni en enlaces.
- [ ] Gestionar rechazo, progreso, reintento y reanudación conforme a la API real.
- [ ] Conectar miniaturas, visor, vídeo y descarga con control de acceso.
- [ ] Comprobar persistencia tras recargar y aislamiento entre usuarios.

**Entrega:** las operaciones sobreviven a recargas porque están confirmadas en
el servidor; los errores no dejan falsos estados de éxito en la interfaz.
Esta fase no empieza hasta disponer de los servicios Rust correspondientes.

### 6. PWA e instalación multiplataforma

- [ ] Añadir manifiesto, iconos locales y experiencia de instalación donde exista soporte.
- [ ] Introducir service worker con caché de recursos estáticos y actualización controlada.
- [ ] Mostrar el estado sin conexión sin prometer acceso a toda la biblioteca.
- [ ] Mantener inicialmente fuera de caché los medios privados y respuestas de cuenta.
  Limpiar estado local de usuario al cerrar sesión y evitar cruces entre cuentas.
- [ ] Comprobar instalación y actualización en dispositivos reales cuando estén disponibles.

**Entrega:** funciona desde navegador y desde el acceso instalado en los entornos
verificados. No se promete sincronización automática del carrete con la app cerrada.

### 7. Revisión y preparación para uso propio

- [ ] Revisar accesibilidad, contraste, foco, tacto, orientación y áreas seguras.
- [ ] Medir carga y memoria con una colección amplia; limitar nodos y solicitudes activas.
- [ ] Revisar CSP, recursos propios, manejo de texto y ausencia de secretos en cliente.
- [ ] Documentar uso local y acceso al servidor desde otros dispositivos.
- [ ] Ensayar pérdidas de conexión y cambios de sesión durante una acción.

**Entrega:** frontend preparado para acompañar una versión del backend que haya
validado por separado almacenamiento, permisos, HTTPS y recuperación de datos.

## Necesidades futuras del servidor, sin implementarlas ahora

| Necesidad de interfaz | Capacidad que deberá ofrecer Rust |
| --- | --- |
| Biblioteca cronológica | Metadatos ordenados, fechas, paginación estable y permisos |
| Miniaturas y visor | Derivados de imagen, tamaños y entrega autenticada |
| Vídeo | Streaming con rangos y formatos compatibles; evaluar transcodificación |
| Subida fiable | Bloques, integridad, cancelación, reanudación y confirmación durable |
| Álbumes y favoritos | Relaciones persistentes sin duplicar originales |
| Papelera | Estado persistente, restauración y eliminación explícita |
| Buscar | Consulta sobre toda la biblioteca autorizada |
| Cuenta y cuota | Sesiones, aislamiento de usuarios y consumo real de almacenamiento |

La elección de bibliotecas de imagen, EXIF, HEIC/RAW y procesamiento de vídeo
requiere una revisión específica. No se presupone que las dependencias actuales
de Drive ya cubran esas tareas. Una herramienta externa como FFmpeg no se añade
por defecto; se explicará y acordará al planificar esa capacidad.

Copias de seguridad, restauración, actualizaciones e instalación del servidor
tendrán su propia fase. Subir una foto al servidor no equivale a tener un backup
independiente del servidor.

## Evolución posterior al producto web

1. Compartición privada de álbumes con permisos y revocación.
2. Metadatos ampliados, mapas y búsqueda avanzada cuando exista soporte real.
3. Clientes Android/iOS para copia automática, con estudio de permisos,
   restricciones de batería, segundo plano y distribución. Consultar tecnología.
4. Cliente de escritorio para vigilar carpetas, si se necesita.
5. Reconocimiento facial, búsqueda semántica y edición, como proyectos separados.

Estas funciones no formarán parte de la primera maqueta ni se mostrarán como
operativas antes de implementarse y comprobarse.

## Organización prevista

```text
frontend/
  PLAN.md
  README.md                  # Uso y verificación cuando exista interfaz
  index.html                 # HTML + CSS + JavaScript, como SantolvDrive
  assets/                    # Iconos y muestras autorizadas
  licenses/                  # Licencias de recursos reutilizados
  manifest.webmanifest       # Fase PWA
  service-worker.js          # Fase PWA
  tests/                     # Pruebas de estado/cliente cuando aporten valor
backend/                     # Reservado para la fase de implementación Rust
```

La organización es prevista. Este trabajo solo modifica documentación.

## Comprobación y flujo de entrega

Para documentación: revisar contenido, enlaces, coherencia con el código de
referencia y `git diff --check`. No instalar frameworks ni generar aplicaciones.

Cuando exista interfaz: revisar aproximadamente 360, 768 y 1440 píxeles,
teclado completo, controles táctiles, orientación y tamaños de texto ampliados.
Comprobar rutas al recargar, filtros, selección y fechas. Probar pérdida de red,
errores de transferencia, doble pulsación y sesión caducada donde corresponda.
Las pruebas del cliente podrán escribirse en JavaScript, igual que la interfaz;
si se necesita un ejecutor de desarrollo, documentarlo sin convertirlo en un
requisito de ejecución del producto. Las herramientas de QA no añaden un framework
a la interfaz. Marcar como pendientes los sistemas sin dispositivo de prueba.

En cada trabajo que modifique archivos: actualizar este plan si procede, revisar
el diff, ejecutar las comprobaciones pertinentes, crear commit local y hacer
push a `https://github.com/santolv/santolvphotos.git`. Verificar el resultado.
No usar force push ni incorporar cambios ajenos. Si falla el acceso, conservar
el commit local e indicar qué autorización o inicio de sesión hace falta.

## Fuentes revisadas

- [Manifiesto de SantolvDrive](https://github.com/santolv/santolvdrive/blob/b9c19e100e15c85534ede1a38e331411d4d2edad/backend/Cargo.toml).
- [Servidor estático Rust de Drive](https://github.com/santolv/santolvdrive/blob/b9c19e100e15c85534ede1a38e331411d4d2edad/backend/src/frontend.rs).
- [Documentación de su interfaz HTML/CSS/JavaScript](https://github.com/santolv/santolvdrive/blob/b9c19e100e15c85534ede1a38e331411d4d2edad/frontend/pruebahtml/README.md).
- [MDN: instalación de una PWA](https://developer.mozilla.org/en-US/docs/Web/Progressive_web_apps/Guides/Making_PWAs_installable).
- [MDN: funcionamiento sin conexión y en segundo plano](https://developer.mozilla.org/en-US/docs/Web/Progressive_web_apps/Guides/Offline_and_background_operation).
