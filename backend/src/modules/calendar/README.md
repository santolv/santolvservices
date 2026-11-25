# Servicio Calendario

Este directorio reserva la lógica Rust del servicio independiente de calendario.
Usará la autenticación, permisos, gestor de módulos y PostgreSQL del backend
central. No se registra todavía en `modules::REGISTERED` hasta aprobar su API y
modelo de datos.

El servicio debe soportar calendarios, eventos, zonas horarias, repetición,
recordatorios y permisos sin copiar la sesión ni crear otro backend.
