# Revisión de las cuatro referencias

Se aplican al producto real manteniendo HTML/CSS/JavaScript, el backend Rust y
PostgreSQL. Sin dependencias nuevas ni datos de demostración en producción.

Calendario: semana horaria, carriles de solapamiento, fila de todo el día,
controles Hoy/anterior/siguiente y botón lateral compartido. Editor compacto con
fichas de calendario, interruptor, grupos de fecha/hora y opciones avanzadas.
Notas y Tareas: fondo negro en oscuro, superficies opacas, buscador y segmentos,
editores con degradado y desenfoque trasero, acciones grises y Hecho amarillo.
Tareas: creación en línea, botón circular y bloque de completadas separado.

Se revisaron con Edge las tres páginas y sus editores en claro y oscuro a
1366×900 y 390×900 usando el JavaScript real y respuestas sintéticas de la API.
Se comprobaron el envío de eventos de día completo, el panel ocultable, la creación
rápida y completar tareas; sin errores JavaScript ni desbordamiento horizontal.
La barra mantiene sus botones de 32×32. No se han modificado cuentas privadas.
Pruebas adicionales: distribución de solapamientos, archivo persistente, vínculos
privados entre dos miembros de un calendario compartido, restricciones de módulos,
denegaciones y borrado de notas sin perder tareas. Las pruebas Rust usan un
PostgreSQL exclusivo. Las capturas no sustituyen estas pruebas de API.
