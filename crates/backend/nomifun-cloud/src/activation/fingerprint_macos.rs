//! macOS fingerprint sources via libc + IOKit (no subprocess).

use std::ffi::CStr;
use std::ptr;

use core_foundation::base::TCFType;
use core_foundation::data::CFData;
use core_foundation::string::CFString;
use core_foundation_sys::base::{CFGetTypeID, CFRelease, CFTypeRef, kCFAllocatorDefault};
use core_foundation_sys::data::{CFDataGetTypeID, CFDataRef};
use core_foundation_sys::string::{CFStringGetTypeID, CFStringRef};
use io_kit_sys::types::{io_iterator_t, io_object_t};
use io_kit_sys::{
    IOIteratorNext, IOObjectRelease, IORegistryEntryCreateCFProperty, IOServiceGetMatchingService,
    IOServiceGetMatchingServices, IOServiceMatching,
};
use libc::{AF_LINK, IFF_LOOPBACK, IFF_UP, freeifaddrs, getifaddrs, ifaddrs, sockaddr_dl, sysctlbyname};

/// Prefer `en0` (matches the former `ifconfig en0` path), else first Up non-loopback AF_LINK MAC.
pub(super) fn read_mac_address() -> Option<String> {
    unsafe {
        let mut ifap: *mut ifaddrs = ptr::null_mut();
        if getifaddrs(&mut ifap) != 0 || ifap.is_null() {
            return None;
        }
        let mut en0: Option<[u8; 6]> = None;
        let mut fallback: Option<[u8; 6]> = None;
        let mut current = ifap;
        while !current.is_null() {
            let ifa = &*current;
            if let Some((name, mac)) = link_mac(ifa) {
                if name == "en0" {
                    en0 = Some(mac);
                    break;
                }
                if fallback.is_none() {
                    fallback = Some(mac);
                }
            }
            current = ifa.ifa_next;
        }
        freeifaddrs(ifap);
        en0.or(fallback).map(|mac| {
            format!(
                "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
            )
        })
    }
}

unsafe fn link_mac(ifa: &ifaddrs) -> Option<(String, [u8; 6])> {
    if ifa.ifa_addr.is_null() {
        return None;
    }
    let flags = ifa.ifa_flags as i32;
    if flags & IFF_LOOPBACK != 0 || flags & IFF_UP == 0 {
        return None;
    }
    let addr = unsafe { &*ifa.ifa_addr };
    if i32::from(addr.sa_family) != AF_LINK {
        return None;
    }
    let sdl = unsafe { &*(ifa.ifa_addr as *const sockaddr_dl) };
    if sdl.sdl_alen != 6 {
        return None;
    }
    let name = if ifa.ifa_name.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(ifa.ifa_name) }
            .to_string_lossy()
            .into_owned()
    };
    let name_len = sdl.sdl_nlen as usize;
    let mac_slice = unsafe {
        std::slice::from_raw_parts(sdl.sdl_data.as_ptr().add(name_len).cast::<u8>(), 6)
    };
    if mac_slice.iter().all(|b| *b == 0) {
        return None;
    }
    let mut mac = [0u8; 6];
    mac.copy_from_slice(mac_slice);
    Some((name, mac))
}

/// IOPlatformExpertDevice `IOPlatformSerialNumber` (same source as system_profiler Hardware).
pub(super) fn read_serial_number() -> Option<String> {
    unsafe {
        let matching = IOServiceMatching(c"IOPlatformExpertDevice".as_ptr());
        if matching.is_null() {
            return None;
        }
        let service = IOServiceGetMatchingService(0, matching);
        if service == 0 {
            return None;
        }
        let key = CFString::new("IOPlatformSerialNumber");
        let prop = IORegistryEntryCreateCFProperty(
            service,
            key.as_concrete_TypeRef(),
            kCFAllocatorDefault,
            0,
        );
        IOObjectRelease(service);
        cf_prop_to_string(prop)
    }
}

/// `machdep.cpu.brand_string` via sysctlbyname (no `/usr/sbin/sysctl` process).
pub(super) fn read_cpu_brand() -> Option<String> {
    sysctl_string(c"machdep.cpu.brand_string")
}

fn sysctl_string(name: &CStr) -> Option<String> {
    unsafe {
        let mut size: libc::size_t = 0;
        if sysctlbyname(name.as_ptr(), ptr::null_mut(), &mut size, ptr::null_mut(), 0) != 0
            || size == 0
        {
            return None;
        }
        let mut buf = vec![0u8; size];
        if sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut size,
            ptr::null_mut(),
            0,
        ) != 0
        {
            return None;
        }
        if size > 0 && buf[size - 1] == 0 {
            buf.truncate(size - 1);
        } else {
            buf.truncate(size);
        }
        let value = String::from_utf8_lossy(&buf).trim().to_string();
        (!value.is_empty()).then_some(value)
    }
}

/// First IOAccelerator `model` — aligns with system_profiler SPDisplaysDataType Chipset Model.
pub(super) fn read_xpu_brand() -> Option<String> {
    unsafe {
        let matching = IOServiceMatching(c"IOAccelerator".as_ptr());
        if matching.is_null() {
            return None;
        }
        let mut iterator: io_iterator_t = 0;
        let kr = IOServiceGetMatchingServices(0, matching, &mut iterator);
        if kr != 0 || iterator == 0 {
            return None;
        }
        let mut found = None;
        loop {
            let service = IOIteratorNext(iterator);
            if service == 0 {
                break;
            }
            let model = registry_model(service);
            IOObjectRelease(service);
            if let Some(name) = model.filter(|s| !s.is_empty()) {
                found = Some(name);
                break;
            }
        }
        IOObjectRelease(iterator);
        found
    }
}

unsafe fn registry_model(service: io_object_t) -> Option<String> {
    let key = CFString::new("model");
    let prop = unsafe {
        IORegistryEntryCreateCFProperty(service, key.as_concrete_TypeRef(), kCFAllocatorDefault, 0)
    };
    cf_prop_to_string(prop)
}

fn cf_prop_to_string(prop: CFTypeRef) -> Option<String> {
    if prop.is_null() {
        return None;
    }
    unsafe {
        let type_id = CFGetTypeID(prop);
        if type_id == CFStringGetTypeID() {
            let s = CFString::wrap_under_create_rule(prop as CFStringRef);
            let value = s.to_string().trim().to_string();
            return (!value.is_empty()).then_some(value);
        }
        if type_id == CFDataGetTypeID() {
            let data = CFData::wrap_under_create_rule(prop as CFDataRef);
            let value = String::from_utf8_lossy(data.bytes())
                .trim_end_matches('\0')
                .trim()
                .to_string();
            return (!value.is_empty()).then_some(value);
        }
        CFRelease(prop);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_readers_return_usable_values() {
        let mac = read_mac_address().expect("mac");
        assert_eq!(mac.matches(':').count(), 5, "mac={mac}");

        let sn = read_serial_number().expect("sn");
        assert!(!sn.is_empty());

        let cpu = read_cpu_brand().expect("cpu");
        assert!(!cpu.is_empty());

        let xpu = read_xpu_brand().expect("xpu");
        assert!(!xpu.is_empty());
    }

    #[test]
    fn macos_source_avoids_subprocess() {
        let source = include_str!("fingerprint_macos.rs");
        let code = source.split("#[cfg(test)]").next().expect("prod code");
        assert!(code.contains("getifaddrs"));
        assert!(code.contains("sysctlbyname"));
        assert!(code.contains("IOPlatformSerialNumber"));
        assert!(code.contains("IOAccelerator"));
        assert!(!code.contains("std::process"));
        assert!(!code.contains("Command::"));
    }
}
