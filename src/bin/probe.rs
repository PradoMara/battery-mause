//! Sondeo HID de SOLO LECTURA para descubrir como un receptor expone la bateria.
//!
//! No envia ningun reporte al dispositivo: solo lista interfaces, lee sus
//! capacidades y escucha reportes de entrada que el dispositivo emita solo.
//!
//! Uso: probe [VID_hex] [PID_hex] [segundos_de_escucha]
//!      probe 373B 1216 20

use std::ffi::c_void;
use windows::Win32::Devices::DeviceAndDriverInstallation::*;
use windows::Win32::Devices::HumanInterfaceDevice::*;
use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::IO::*;
use windows::Win32::System::Threading::*;
use windows::core::PCWSTR;

struct Iface {
    path: Vec<u16>,
    path_str: String,
    usage_page: u16,
    usage: u16,
    in_len: u16,
    out_len: u16,
    feat_len: u16,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let vid = args.get(1).map_or(0x373B, |s| u16::from_str_radix(s, 16).unwrap());
    let pid = args.get(2).map_or(0x1216, |s| u16::from_str_radix(s, 16).unwrap());
    let secs: u32 = args.get(3).map_or(15, |s| s.parse().unwrap());

    let ifaces = enumerate(vid, pid);
    println!("== Interfaces HID {vid:04X}:{pid:04X}: {} ==", ifaces.len());
    for (i, f) in ifaces.iter().enumerate() {
        println!(
            "[{i}] UsagePage=0x{:04X} Usage=0x{:04X} in={}B out={}B feature={}B\n    {}",
            f.usage_page, f.usage, f.in_len, f.out_len, f.feat_len, f.path_str
        );
    }

    // Modo escritura (autorizado por el usuario): probe VID PID SEG poke INDICE HEX...
    // Envia UN reporte de salida con los bytes dados (rellenado con ceros) y escucha la respuesta.
    if args.get(4).map(String::as_str) == Some("poke") {
        let f = &ifaces[args[5].parse::<usize>().unwrap()];
        let mut data: Vec<u8> = args[6..].iter().map(|s| u8::from_str_radix(s, 16).unwrap()).collect();
        data.resize(f.out_len as usize, 0);
        poke(f, &data, secs);
        return;
    }

    // Solo escuchamos interfaces "vendor-defined" (UsagePage >= 0xFF00):
    // Windows bloquea mouse/teclado y ademas no tienen la bateria.
    let vendor: Vec<&Iface> = ifaces.iter().filter(|f| f.usage_page >= 0xFF00 && f.in_len > 0).collect();
    if vendor.is_empty() {
        println!("\nNo hay interfaces vendor con reportes de entrada para escuchar.");
        return;
    }
    println!("\n== Escuchando {secs}s en {} interfaz(ces) vendor (mueve/usa el mouse) ==", vendor.len());
    let handles: Vec<_> = vendor
        .iter()
        .filter_map(|f| open_read(f).map(|h| (h, f)))
        .collect();
    listen(&handles, secs);
}

fn enumerate(vid: u16, pid: u16) -> Vec<Iface> {
    let mut out = Vec::new();
    unsafe {
        let guid = HidD_GetHidGuid();
        let set = SetupDiGetClassDevsW(Some(&guid), PCWSTR::null(), None, DIGCF_PRESENT | DIGCF_DEVICEINTERFACE).unwrap();
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
            let mut path: Vec<u16> = std::slice::from_raw_parts(p, len).to_vec();
            let path_str = String::from_utf16_lossy(&path);
            path.push(0);

            // Acceso 0: permite consultar atributos sin abrir el dispositivo para E/S.
            let Ok(h) = CreateFileW(
                PCWSTR(path.as_ptr()),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            ) else {
                continue;
            };
            let mut attr = HIDD_ATTRIBUTES { Size: size_of::<HIDD_ATTRIBUTES>() as u32, ..Default::default() };
            if HidD_GetAttributes(h, &mut attr) && attr.VendorID == vid && attr.ProductID == pid {
                let mut pp = PHIDP_PREPARSED_DATA::default();
                if HidD_GetPreparsedData(h, &mut pp) {
                    let mut caps = HIDP_CAPS::default();
                    let _ = HidP_GetCaps(pp, &mut caps);
                    out.push(Iface {
                        path,
                        path_str,
                        usage_page: caps.UsagePage,
                        usage: caps.Usage,
                        in_len: caps.InputReportByteLength,
                        out_len: caps.OutputReportByteLength,
                        feat_len: caps.FeatureReportByteLength,
                    });
                    let _ = HidD_FreePreparsedData(pp);
                }
            }
            let _ = CloseHandle(h);
        }
        let _ = SetupDiDestroyDeviceInfoList(set);
    }
    out
}

fn open_read(f: &Iface) -> Option<HANDLE> {
    unsafe {
        CreateFileW(
            PCWSTR(f.path.as_ptr()),
            GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            None,
        )
        .ok()
    }
}

fn poke(f: &Iface, data: &[u8], secs: u32) {
    unsafe {
        let Ok(h) = CreateFileW(
            PCWSTR(f.path.as_ptr()),
            (GENERIC_READ | GENERIC_WRITE).0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            None,
        ) else {
            println!("No se pudo abrir la interfaz con escritura");
            return;
        };
        let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" ");
        let rev = CreateEventW(None, true, false, PCWSTR::null()).unwrap();
        let mut rov = OVERLAPPED { hEvent: rev, ..Default::default() };
        let mut rbuf = vec![0u8; f.in_len.max(1) as usize];
        if f.in_len > 0 {
            let _ = ReadFile(h, Some(&mut rbuf), None, Some(&mut rov));
        }
        let wev = CreateEventW(None, true, false, PCWSTR::null()).unwrap();
        let mut wov = OVERLAPPED { hEvent: wev, ..Default::default() };
        println!("TX ({}B): {}", data.len(), hex(data));
        let _ = WriteFile(h, Some(data), None, Some(&mut wov));
        let mut sent = 0u32;
        let ok = GetOverlappedResultEx(h, &wov, &mut sent, 2000, false).is_ok();
        println!("escritura: {}", if ok { format!("ok, {sent} bytes") } else { "fallo/timeout".into() });
        let start = std::time::Instant::now();
        while start.elapsed().as_secs() < secs as u64 {
            let mut got = 0u32;
            if GetOverlappedResultEx(h, &rov, &mut got, 300, false).is_ok() {
                println!("RX ({got}B): {}", hex(&rbuf[..got as usize]));
                let _ = ResetEvent(rev);
                let _ = ReadFile(h, Some(&mut rbuf), None, Some(&mut rov));
            }
        }
        let _ = CancelIoEx(h, None);
        let _ = CloseHandle(h);
    }
}

fn listen(handles: &[(HANDLE, &&Iface)], secs: u32) {
    unsafe {
        struct Slot {
            ov: OVERLAPPED,
            buf: Vec<u8>,
        }
        let mut slots: Vec<Box<Slot>> = Vec::new();
        for (h, f) in handles {
            let ev = CreateEventW(None, true, false, PCWSTR::null()).unwrap();
            let mut s = Box::new(Slot { ov: OVERLAPPED { hEvent: ev, ..Default::default() }, buf: vec![0u8; f.in_len as usize] });
            let _ = ReadFile(*h, Some(&mut s.buf), None, Some(&mut s.ov));
            slots.push(s);
        }
        let events: Vec<HANDLE> = slots.iter().map(|s| s.ov.hEvent).collect();
        let start = std::time::Instant::now();
        while start.elapsed().as_secs() < secs as u64 {
            let r = WaitForMultipleObjects(&events, false, 500);
            let i = r.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
            if i < slots.len() {
                let mut got = 0u32;
                let (h, f) = &handles[i];
                if GetOverlappedResult(*h, &slots[i].ov, &mut got, false).is_ok() {
                    let hex: Vec<String> = slots[i].buf[..got as usize].iter().map(|b| format!("{b:02X}")).collect();
                    println!("[{:>5.1}s] iface UP=0x{:04X} {}B: {}", start.elapsed().as_secs_f32(), f.usage_page, got, hex.join(" "));
                }
                let _ = ResetEvent(slots[i].ov.hEvent);
                let s = &mut slots[i];
                let _ = ReadFile(*h, Some(&mut s.buf), None, Some(&mut s.ov as *mut _ as *mut OVERLAPPED));
            }
        }
        for (h, _) in handles {
            let _ = CancelIoEx(*h, None);
            let _ = CloseHandle(*h);
        }
        let _ = std::ptr::null::<c_void>();
    }
    println!("== Fin de la escucha ==");
}
