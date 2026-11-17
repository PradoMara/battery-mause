//! Acceso minimo al registro (solo HKCU, sin permisos de administrador).

use std::ffi::c_void;
use windows::Win32::Foundation::*;
use windows::Win32::System::Registry::*;
use windows::core::PCWSTR;

pub const APP_KEY: &str = r"Software\MouseBattery";
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const APPROVED_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
pub const THEME_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
pub const RUN_NAME: &str = "MouseBattery";

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn get_dword(sub: &str, name: &str) -> Option<u32> {
    let (s, n) = (w(sub), w(name));
    let mut v = 0u32;
    let mut sz = 4u32;
    let r = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, PCWSTR(s.as_ptr()), PCWSTR(n.as_ptr()), RRF_RT_REG_DWORD, None, Some(&mut v as *mut u32 as *mut c_void), Some(&mut sz))
    };
    (r == ERROR_SUCCESS).then_some(v)
}

pub fn set_dword(sub: &str, name: &str, v: u32) {
    set(sub, name, REG_DWORD, &v.to_le_bytes());
}

pub fn set_string(sub: &str, name: &str, v: &str) {
    let bytes: Vec<u8> = w(v).iter().flat_map(|c| c.to_le_bytes()).collect();
    set(sub, name, REG_SZ, &bytes);
}

fn set(sub: &str, name: &str, ty: REG_VALUE_TYPE, data: &[u8]) {
    let (s, n) = (w(sub), w(name));
    unsafe {
        let mut key = HKEY::default();
        if RegCreateKeyExW(HKEY_CURRENT_USER, PCWSTR(s.as_ptr()), None, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut key, None) == ERROR_SUCCESS {
            let _ = RegSetValueExW(key, PCWSTR(n.as_ptr()), None, ty, Some(data));
            let _ = RegCloseKey(key);
        }
    }
}

pub fn delete_value(sub: &str, name: &str) {
    let (s, n) = (w(sub), w(name));
    unsafe {
        let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(s.as_ptr()), PCWSTR(n.as_ptr()));
    }
}

pub fn string_exists(sub: &str, name: &str) -> bool {
    let (s, n) = (w(sub), w(name));
    unsafe { RegGetValueW(HKEY_CURRENT_USER, PCWSTR(s.as_ptr()), PCWSTR(n.as_ptr()), RRF_RT_REG_SZ, None, None, None) == ERROR_SUCCESS }
}

pub fn first_binary_byte(sub: &str, name: &str) -> Option<u8> {
    let (s, n) = (w(sub), w(name));
    let mut buf = [0u8; 16];
    let mut sz = buf.len() as u32;
    let r = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, PCWSTR(s.as_ptr()), PCWSTR(n.as_ptr()), RRF_RT_REG_BINARY, None, Some(buf.as_mut_ptr() as *mut c_void), Some(&mut sz))
    };
    (r == ERROR_SUCCESS && sz > 0).then_some(buf[0])
}

/// Activo si existe en Run y el Administrador de tareas no lo desactivo
/// (StartupApproved: primer byte impar = deshabilitado).
pub fn autostart_enabled() -> bool {
    string_exists(RUN_KEY, RUN_NAME) && first_binary_byte(APPROVED_KEY, RUN_NAME).is_none_or(|b| b & 1 == 0)
}
