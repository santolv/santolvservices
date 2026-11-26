# Revisión de los diseños

Referencia principal: Calendario.html y Notas y tareas.html aportados por Santi.
Pautas consultadas: stageshow (craft y ejemplo calendar), Apple design y ponytail.

Aplicación: interfaz real HTML/CSS/JavaScript, sin kit React ni escenas simuladas.
Se conserva la barra global y toda la lógica API existente. No se importan los
scripts localStorage ni los datos ficticios de las referencias. Calendario usa
panel lateral, cuadrícula completa, selector segmentado y colores estables por
calendario. Notas usa listas agrupadas, editor existente y acento amarillo.

Verificación: cuatro pruebas de clientes existentes, sintaxis de ambos scripts,
identificadores únicos y diff sin errores. Capturas con Edge de Calendario a
1366×900 y Notas a 390×844 con datos sintéticos, sin guardar datos de usuario.
No se ha filmado un bucle: no se añade una demostración animada. No se han probado
operaciones con cuentas privadas reales en el navegador. CI comprueba Rust y
PostgreSQL antes de integrar. La comprobación de localhost verifica disponibilidad
y protección de rutas, sin alterar notas ni eventos privados.
