# MouseBattery

Muestra el porcentaje de batería de tu mouse inalámbrico en la bandeja de Windows, para saber cuándo cargarlo.
Un solo `.exe` de ~140 KB, ~2 MB de RAM, sin red, sin permisos de administrador.

**Mouse soportado por ahora:** ATK Dragonfly A9 2.0 / A9 Plus (receptor 2.4 GHz, `373B:1216`).
Soporte para otros mouse: pendiente (ver "Agregar un mouse").

## Uso

1. Descarga o compila `MouseBattery.exe` y ábrelo. Aparece un icono con el porcentaje junto al reloj
   (si no lo ves, fíjalo desde el menú `^` de la bandeja).
2. **Pasa el cursor** sobre el icono: panel con batería, estado, mouse y conexión.
3. **Clic derecho:** intervalo de lectura (30 s a 1 h, por defecto 5 min), qué filas muestra el panel,
   parpadeo de aviso, inicio con Windows, idioma (español / inglés), actualizar ahora, salir.

| Situación | Icono |
|---|---|
| Normal | Número (100 % se muestra como 99; el panel da el valor exacto) |
| 30 % y 20 % | Círculo amarillo parpadeando 10 s y el panel se abre una vez |
| 10 % o menos | Círculo rojo fijo hasta que cargue |
| Cable conectado | Rayo (el porcentaje por cable aún no se puede leer: muestra el último valor) |
| Sin lectura | Gris con una X (se reintenta cada 30 s) |

Con el mouse dormido el receptor no responde hasta que lo mueves; la app reintenta sola.

Al primer arranque se registra para iniciar con Windows. Puedes quitarlo en el menú o en
*Administrador de tareas → Aplicaciones de inicio*. Si mueves el `.exe`, desmárcalo y márcalo de nuevo.
La configuración se guarda en `HKCU\Software\MouseBattery`.

## Compilar

```bash
cargo build --release      # -> target/release/MouseBattery.exe
cargo test
```

## Seguridad

- Sin código de red: sin sockets, telemetría ni actualizaciones.
- Sin administrador: solo escribe en `HKCU` y en el mouse envía **una trama fija de lectura** de batería.
- Única dependencia: la crate oficial `windows` de Microsoft (`Cargo.lock` incluido).
- El `.exe` no está firmado: Windows SmartScreen avisará la primera vez. Compara el SHA-256 publicado
  (`Get-FileHash MouseBattery.exe`).

## Cómo se leyó la batería del A9 (protocolo)

Capturado del configurador web oficial, solo lectura. Reporte de salida ID 8 por la interfaz vendor `0xFF05`:
`[suma, 72, len, 00, 01, reg_lo, reg_hi]` (suma = bytes 1..6 mod 256). Registro `07 01` → la respuesta trae
el porcentaje en el byte 7. Detalles en `src/a9.rs`.

## Agregar un mouse

Cada familia de receptor propietario necesita su propio módulo (como `src/a9.rs`). `src/bin/probe.rs` es una
herramienta de sondeo HID para descubrir el protocolo (`probe VID PID`; el modo `poke` envía un reporte
y **escribe al dispositivo: úsalo solo si sabes lo que haces**). `tools/webhid-capture.js` registra el tráfico
WebHID del configurador oficial (pasivo).

## Pendiente

Bluetooth genérico (batería estándar de Windows), lectura de batería por cable, DPI en el panel, Linux.

## Licencia

MIT.
