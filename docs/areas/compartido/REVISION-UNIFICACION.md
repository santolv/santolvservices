# Unificación visual de Calendario, Notas y Tareas

La paleta común está en frontend/shared/shell.css y toma como base el fondo de
Drive/Fotos: #fff en claro y #18181a en oscuro. Los tres módulos reutilizan las
mismas superficies, texto, líneas, controles, diálogos y degradado del fondo modal.
Se conservan los acentos semánticos de cada aplicación y las referencias del usuario.

Notas y Tareas ya no dependen de @scope: sus reglas tienen selectores explícitos
limitados al contenido y al editor, sin afectar a la barra global. Se retiraron
reglas duplicadas y se cubren filtros, relaciones y todos los campos del editor.
Calendario usa esa paleta y el estilo compartido también en gestionar calendarios,
acceso compartido, recordatorios y opciones avanzadas.

El botón del lateral comparte icono, tamaño y transición con Drive/Fotos. En móvil
Calendario superpone el panel sin empujar el contenido. La barra global conserva
sus botones de 32×32 y no se duplica.

Revisión con Edge y datos sintéticos: veinte combinaciones de cinco aplicaciones,
dos temas y anchuras 1366 y 390, sin desbordamiento ni errores de JavaScript.
Se revisaron los formularios secundarios, los editores y la geometría del lateral.
Se comprobó el desenfoque de 6px con transparencia normal. La preferencia del
sistema de reducir transparencia mantiene un fondo sólido accesible.

Pasan los seis clientes existentes y git diff --check. No se modifica Rust, API,
migraciones ni datos. Las capturas usan respuestas de prueba; no se han realizado
escrituras sobre cuentas privadas. La publicación local requiere recompilar el
HTML incluido en el servidor desde main, tras integrar la PR.
