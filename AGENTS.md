# Instrucciones del proyecto

- El backend debe escribirse en Rust, siguiendo la base técnica de santolvdrive.
- El usuario ha autorizado el mismo sistema de frontend que santolvdrive:
  HTML, CSS y JavaScript propios, sin framework ni compilación WASM.
  No volver a pedir permiso por estos lenguajes o por SVG/JSON necesarios para la web.
- Consultar antes de introducir otros lenguajes o cambiar sustancialmente este sistema.
- El alcance actual es el plan multiplataforma dentro de `frontend/PLAN.md`,
  centrado en la interfaz y con las dependencias futuras del servidor identificadas.
  No desarrollar el backend ni implementar la interfaz hasta recibir esa petición.
- Usar `https://github.com/santolv/santolvdrive` como referencia técnica, comprobando
  el código real y distinguiendo las dependencias del servidor de las del navegador.
- Mantener el plan actualizado al completar cada fase o cambiar una decisión.
- Al finalizar cada trabajo que modifique archivos, realizar las comprobaciones
  pertinentes, revisar el diff y crear un commit local con los cambios de ese trabajo.
- Después del commit, subirlo a `origin` en la rama de trabajo correspondiente.
  El repositorio autorizado es `https://github.com/santolv/santolvphotos.git`.
- No incluir cambios ajenos, secretos, originales privados, directorios de compilación
  ni credenciales. No usar force push ni reescribir historia sin autorización.
- Si falla la autenticación o el push, conservar el commit local e indicar qué falta.
  No afirmar que los cambios están publicados sin comprobarlo.
- Identidad inicial local: nombre `santolv`; correo `santi@local`, reutilizado del
  último commit público de santolvdrive. La autenticación de GitHub es independiente.
