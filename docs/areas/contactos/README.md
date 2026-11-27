# Contactos

Agenda privada de personas y empresas. La portada utiliza la barra y paleta de
Santolv, con lista alfabética, búsqueda, favoritos y una ficha adaptable a móvil.
Cada ficha admite nombre, apellidos, correo, teléfono, empresa, puesto, dirección,
sitio web y notas. Permite descargar un archivo vCard, copiar el correo, llamar
mediante el dispositivo y abrir una redacción en Correo cuando esté habilitado.

El módulo `contacts` usa Rust y PostgreSQL. Core gestiona autenticación,
permisos y activación. La migración 0023 crea su esquema con el servicio
desactivado. Para habilitarlo, ejecutar `services enable contacts` en la
consola administrativa del servidor.

- `GET /api/v1/contacts` lista fichas por nombre con `q`, `favorite`, `limit`
  y `cursor` opcionales. Responde con `items` y `next_cursor`.
- `POST /api/v1/contacts` crea una ficha a partir de sus campos.
- `GET /api/v1/contacts/{id}` obtiene una ficha privada.
- `PUT /api/v1/contacts/{id}` actualiza los campos con `{ revision, fields }`.
- `PATCH /api/v1/contacts/{id}/favorite` recibe `{ revision, favorite }`.
- `DELETE /api/v1/contacts/{id}?revision=…` elimina la ficha.

Las escrituras verifican propietario y revisión. El buscador trata los caracteres
de consulta como texto literal. Correo consume el listado mediante los mismos
permisos, sin dependencia obligatoria entre módulos.
