# santolvphotos

Galería de fotos y vídeos autohospedada, inspirada en Google Fotos e Immich.

El sistema sigue la base de SantolvDrive: backend en Rust e interfaz web con
HTML, CSS y JavaScript propios, sin framework frontend.

Abre [frontend/index.html](frontend/index.html) para explorar el HTML base con
fotografías de muestra. Funciona localmente sin instalación. Consulta sus
[instrucciones](frontend/README.md) para conocer las funciones y límites de la demo.

El trabajo inicial se centra en la interfaz web/de PC, sin desarrollar todavía
el backend. La interfaz móvil y las aplicaciones descargables se decidirán después.
Consulta el [plan multiplataforma](frontend/PLAN.md) para conocer tecnologías,
pantallas, fases, compatibilidad y criterios de entrega.

## Repositorio y trabajo local

Repositorio: [santolv/santolvphotos](https://github.com/santolv/santolvphotos).
La rama principal es `main`.

Para obtener una copia nueva:

```sh
git clone https://github.com/santolv/santolvphotos.git
cd santolvphotos
```

Abre `frontend/index.html` directamente en el navegador. No se necesita instalar
dependencias para esta demo. El backend Rust todavía no está implementado.

Antes de enviar cambios, revisa los archivos incluidos y comprueba el diff:

```sh
git status
git diff --check
git diff
git add <archivos-revisados>
git diff --cached
git commit -m "Describe el cambio"
git push origin main
```

El push requiere una cuenta de GitHub con acceso de escritura. La identidad del
autor de los commits no proporciona autenticación. `.gitignore` excluye rutas
habituales de credenciales, datos privados y archivos generados; guarda los
originales privados fuera de `frontend/assets/`, que contiene muestras públicas.
