//! Backend ATK Dragonfly A9 2.0 (receptor 373B:1216, cable 373B:1263).
//!
//! Protocolo (descubierto capturando el configurador oficial):
//! reporte de salida ID 8 por la interfaz vendor 0xFF05 (63 bytes de datos):
//!   [suma, 0x72, long, 0x00, 0x01, reg_lo, reg_hi] + ceros
//! con suma = (bytes 1..=6) mod 256. Para la bateria: registro 07 01.
//! Respuesta ID 8: 72 00 05 00 07 01 [bateria %] [?] 0F ...
//! Solo se envia esa trama fija de LECTURA; nunca datos de fuentes externas.

use std::ffi::c_void;
use windows::Win32::Devices::DeviceAndDriverInstallation::*;
use windows::Win32::Devices::HumanInterfaceDevice::HidD_GetHidGuid;
use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::IO::*;
use windows::Win32::System::Threading::*;
use windows::core::PCWSTR;

pub const NAME: &str = "ATK A9 Plus 2.0";
const DONGLE: &str = "vid_373b&pid_1216";
const CABLE: &str = "vid_373b&pid_1263";

/// Trama de lectura del registro de bateria (ID de reporte 8 + 63 bytes).
const REQ: [u8; 64] = {
    let mut f = [0u8; 64];
    f[0] = 0x08;
    f[1] = 0x72u8.wrapping_add(0x02).wrapping_add(0x01).wrapping_add(0x07).wrapping_add(0x01); // suma
    f[2] = 0x72;
    f[3] = 0x02;
    f[5] = 0x01;
    f[6] = 0x07;
    f[7] = 0x01;
    f
};

pub enum Probe {
    Battery(u8),
    /// Cable conectado: el mouse deja de hablar con el receptor.
    Cable,
    Absent,
}

pub fn read() -> Probe {
    let paths = hid_paths();
    if paths.iter().any(|p| p.contains(CABLE)) {
        return Probe::Cable;
    }
    let Some(path) = paths.iter().find(|p| p.contains(DONGLE) && p.contains("mi_02&col04")) else {
        return Probe::Absent;
    };
    match query(path) {
        Some(p) => Probe::Battery(p),
        None => Probe::Absent,
    }
}

fn parse(resp: &[u8]) -> Option<u8> {
    // 08 72 00 05 00 07 01 [pct] ...
    if resp.len() > 7 && resp[0] == 0x08 && resp[1] == 0x72 && resp[5] == 0x07 && resp[6] == 0x01 && resp[7] <= 100 {
        Some(resp[7])
    } else {
        None
    }
}

fn hid_paths() -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let guid = HidD_GetHidGuid();
        let Ok(set) = SetupDiGetClassDevsW(Some(&guid), PCWSTR::null(), None, DIGCF_PRESENT | DIGCF_DEVICEINTERFACE) else {
            return out;
        };
        let mut idx = 0;
        loop {
            let mut ifd = SP_DEVICE_INTERFACE_DATA { cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32, ..Default::default() };
            if SetupDiEnumDeviceInterfaces(set, None, &guid, idx, &mut ifd).is_err() {
                break;
            }
            idx += 1;
            let mut need = 0u32;
            let _ = SetupDiGetDeviceInterfaceDetailW(set, &ifd, None, 0, Some(&mut need), None);
            let mut buf = vec![0u8; need as usize];
            let det = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*det).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            if SetupDiGetDeviceInterfaceDetailW(set, &ifd, Some(det), need, None, None).is_err() {
                continue;
            }
            let p = (*det).DevicePath.as_ptr();
            let mut len = 0;
            while *p.add(len) != 0 {
                len += 1;
            }
            out.push(String::from_utf16_lossy(std::slice::from_raw_parts(p, len)).to_ascii_lowercase());
        }
        let _ = SetupDiDestroyDeviceInfoList(set);
    }
    out
}

fn query(path: &str) -> Option<u8> {
    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let h = CreateFileW(
            PCWSTR(wide.as_ptr()),
            (GENERIC_READ | GENERIC_WRITE).0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            None,
        )
        .ok()?;
        let rev = CreateEventW(None, true, false, PCWSTR::null()).ok()?;
        let wev = CreateEventW(None, true, false, PCWSTR::null()).ok()?;
        let mut rov = OVERLAPPED { hEvent: rev, ..Default::default() };
        let mut wov = OVERLAPPED { hEvent: wev, ..Default::default() };
        let mut rbuf = [0u8; 64];
        let _ = ReadFile(h, Some(&mut rbuf), None, Some(&mut rov));
        let _ = WriteFile(h, Some(&REQ), None, Some(&mut wov));
        let mut n = 0u32;
        let mut result = None;
        let mut sent = 0u32;
        if GetOverlappedResultEx(h, &wov, &mut sent, 700, false).is_ok() {
            for _ in 0..4 {
                if GetOverlappedResultEx(h, &rov, &mut n, 700, false).is_err() {
                    break;
                }
                if let Some(p) = parse(&rbuf[..n as usize]) {
                    result = Some(p);
                    break;
                }
                let _ = ResetEvent(rev);
                let _ = ReadFile(h, Some(&mut rbuf), None, Some(&mut rov));
            }
        }
        let _ = CancelIoEx(h, None);
        let _ = CloseHandle(rev);
        let _ = CloseHandle(wev);
        let _ = CloseHandle(h);
        let _ = std::ptr::null::<c_void>();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_matches_captured_frame() {
        // Capturado del configurador: 7D 72 02 00 01 07 01
        assert_eq!(&REQ[..8], &[0x08, 0x7D, 0x72, 0x02, 0x00, 0x01, 0x07, 0x01]);
    }

    #[test]
    fn parses_battery_response() {
        let mut r = [0u8; 64];
        r[..10].copy_from_slice(&[0x08, 0x72, 0x00, 0x05, 0x00, 0x07, 0x01, 0x35, 0x56, 0x0F]);
        assert_eq!(parse(&r), Some(53));
        r[5] = 0x0F; // otro registro
        assert_eq!(parse(&r), None);
    }
}

#[cfg(test)]
mod live {
    use super::*;
    #[test]
    #[ignore]
    fn live_read() {
        let paths = hid_paths();
        for p in paths.iter().filter(|p| p.contains("vid_373b")) {
            eprintln!("PATH {p}");
        }
        match read() {
            Probe::Battery(p) => eprintln!("RESULT battery {p}"),
            Probe::Cable => eprintln!("RESULT cable"),
            Probe::Absent => eprintln!("RESULT absent"),
        }
    }
}
