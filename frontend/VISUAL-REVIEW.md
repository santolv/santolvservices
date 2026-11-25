# Revisión visual de Santolv

## Dirección aprobada

- Producto: archivos y fotografías personales desde una cuenta.
- Carácter: sereno, familiar y preciso.
- Entorno: Finder y Fototeca, conservando la paleta propia.
- Protagonistas: fotografías y archivos, con controles discretos.
- Firma: navegación uniforme entre servicios.

## Alcance

Cinco páginas: `home/index.html`, `home/login.html`, `account/index.html`,
`drive/index.html` y `photos/index.html`. También se revisaron los fragmentos
`shared/shell.html`, `shared/sidebar-button.html` y `shared/upload-button.html`
y su hoja de estilos compartida. Estos últimos conservan su estructura y medidas.

Stageshow se aplica al encuadre, jerarquía, alineación y protagonismo del contenido
de la aplicación real. No se incorpora su kit de demostraciones, React, datos
ficticios ni representaciones de controles sin funcionamiento.

## Pasadas

1. Espaciado: ritmo de 4 px en los grupos principales de Inicio y Cuenta,
   formularios de acceso y encabezados de Drive y Photos. Se conservan las
   dimensiones acordadas de navegación y fotografías.
2. Color: fondos neutros, sin el lavado radial de Photos ni sombras azules
   decorativas al pasar el ratón. Botón principal de Drive con azul más oscuro
   para que su texto blanco tenga contraste suficiente.
3. Iconos: se conserva el sistema SVG existente y los colores semánticos de
   archivos y servicios. Los iconos de Photos ya no saltan ni giran.
4. Texto: Inicio identifica el contexto; su descripción invita a elegir una app.
   No se cambian errores ni mensajes de operaciones.
5. Tipografía: misma familia de sistema en acceso y Cuenta, pesos hasta 600,
   cifras tabulares, corrección del espaciado de letras inválido en Inicio y
   mínimo de 11 px para metadatos en las páginas revisadas.
6. Distribución: se mantienen orden, servicios, formularios y navegación;
   la fototeca sigue siendo 4:3 y la barra superior conserva su tamaño.
7. Detalles: campos de acceso y Cuenta de 44 px, radios de 8 px y foco visible
   en el acceso; la pantalla de acceso sigue siendo independiente del shell.
8. Imágenes: se mantienen las fotografías y los SVG reales del producto;
   no se añaden ilustraciones decorativas ni dependencias.
9. Movimiento: una entrada por opacidad de la biblioteca; se eliminan escalado
   al pasar el ratón, rebote de selección y transiciones de posición y columnas
   de Photos. Se conserva la carga de imágenes y la apertura de paneles y visor,
   con respeto a movimiento reducido.

Se retiran reglas decorativas sustituidas de Photos. Esta pasada no pretende
reescribir todas las capas históricas de CSS de Drive y Photos: requieren una
comparación visual completa para retirarlas sin alterar otros estados.

## Verificación

- Suites existentes de Inicio, Drive, Photos y shell compartido.
- Compilación Rust del ejecutable que incorpora las páginas.
- Comprobación de arranque y respuesta HTTP de localhost tras instalarlo.
- Las cuatro suites del cliente y la compilación release han pasado. También
  se comprobó la sintaxis JavaScript y el mínimo de texto en las cinco páginas.
- Localhost se actualizó y superó la comprobación HTTP de arranque.
- La comparación visual en ambos temas y anchos queda pendiente: el navegador
  agotó el tiempo de navegación a localhost. No se declara verificación visual
  ni una grabación de ensayo de stageshow.

## Referencias

Stageshow: criterios de composición y `references/craft.md`.
Unslop: auditoría, dirección y nueve pasadas de pulido.
Crédito de la metodología Unslop: Damir Sokolovsky
([@heydamir](https://x.com/heydamir)), CC BY 4.0. No se incorpora código de su kit.
