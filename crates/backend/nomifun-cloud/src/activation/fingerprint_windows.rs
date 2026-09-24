//! Windows fingerprint sources via Win32 / CPUID / DXGI (no PowerShell).

use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_SUCCESS};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1,
};
use windows::Win32::NetworkManagement::IpHelper::{
    GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
    GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::System::SystemInformation::{
    FIRMWARE_TABLE_PROVIDER, GetSystemFirmwareTable,
};
use windows::core::HRESULT;

/// Prefer an Up adapter with a real 6-byte MAC; if none are Up, fall back to
/// any non-loopback adapter that still exposes a physical MAC (lowest IfIndex).
pub(super) fn read_mac_address() -> Option<String> {
    let mut size = 0u32;
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    // SAFETY: size-query call with null buffer; ERROR_BUFFER_OVERFLOW is expected.
    let status = unsafe {
        GetAdaptersAddresses(0, flags, None, None, &mut size)
    };
    if status != ERROR_BUFFER_OVERFLOW.0 && status != ERROR_SUCCESS.0 {
        return None;
    }
    if size == 0 {
        return None;
    }

    let mut buffer = vec![0u8; size as usize];
    // SAFETY: buffer is sized from the prior probe; pointer is valid for `size` bytes.
    let status = unsafe {
        GetAdaptersAddresses(
            0,
            flags,
            None,
            Some(buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>()),
            &mut size,
        )
    };
    if status != ERROR_SUCCESS.0 {
        return None;
    }

    let mut best_up: Option<(u32, [u8; 6])> = None;
    let mut best_any: Option<(u32, [u8; 6])> = None;
    // SAFETY: GetAdaptersAddresses initialized a linked list of IP_ADAPTER_ADDRESSES_LH.
    let mut current = buffer.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
    while !current.is_null() {
        // SAFETY: `current` walks the list filled by GetAdaptersAddresses; union
        // field `IfIndex` is valid for dual-stack LH adapters.
        let (mac, if_index, is_up, next) = unsafe {
            let adapter = &*current;
            (
                adapter_physical_mac(adapter),
                adapter.Anonymous1.Anonymous.IfIndex,
                adapter.OperStatus == IfOperStatusUp,
                adapter.Next,
            )
        };
        if let Some(mac) = mac {
            if is_up
                && (best_up.is_none() || best_up.is_some_and(|(idx, _)| if_index < idx))
            {
                best_up = Some((if_index, mac));
            }
            if best_any.is_none() || best_any.is_some_and(|(idx, _)| if_index < idx) {
                best_any = Some((if_index, mac));
            }
        }
        current = next;
    }

    best_up.or(best_any).map(|(_, mac)| {
        format!(
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
        )
    })
}

fn adapter_physical_mac(adapter: &IP_ADAPTER_ADDRESSES_LH) -> Option<[u8; 6]> {
    // IF_TYPE_SOFTWARE_LOOPBACK
    if adapter.IfType == 24 {
        return None;
    }
    if adapter.PhysicalAddressLength != 6 {
        return None;
    }
    let mut mac = [0u8; 6];
    mac.copy_from_slice(&adapter.PhysicalAddress[..6]);
    if mac.iter().all(|b| *b == 0) {
        return None;
    }
    Some(mac)
}

/// SMBIOS Type 1 system serial, then Type 2 baseboard — matches typical Win32_BIOS.SerialNumber.
pub(super) fn read_serial_number() -> Option<String> {
    let table = read_raw_smbios()?;
    let type1 = smbios_string_at(&table, 1, 0x07);
    if let Some(serial) = type1.filter(|s| !s.is_empty()) {
        return Some(serial);
    }
    smbios_string_at(&table, 2, 0x07).filter(|s| !s.is_empty())
}

fn read_raw_smbios() -> Option<Vec<u8>> {
    // MSDN multi-char 'RSMB' → 0x52534D42
    const RSMB: FIRMWARE_TABLE_PROVIDER =
        FIRMWARE_TABLE_PROVIDER(u32::from_be_bytes(*b"RSMB"));
    // SAFETY: size probe with null destination.
    let size = unsafe { GetSystemFirmwareTable(RSMB, 0, None) };
    if size == 0 {
        return None;
    }
    let mut buffer = vec![0u8; size as usize];
    // SAFETY: buffer length equals the probed byte count.
    let written = unsafe { GetSystemFirmwareTable(RSMB, 0, Some(buffer.as_mut_slice())) };
    if written == 0 || written > size {
        return None;
    }
    buffer.truncate(written as usize);
    if buffer.len() < 8 {
        return None;
    }
    let payload_len = u32::from_le_bytes(buffer[4..8].try_into().ok()?) as usize;
    let start: usize = 8;
    let end = start.saturating_add(payload_len).min(buffer.len());
    Some(buffer[start..end].to_vec())
}

fn smbios_string_at(table: &[u8], type_id: u8, string_offset: usize) -> Option<String> {
    let mut offset = 0usize;
    while offset + 4 <= table.len() {
        let entry_type = table[offset];
        let formatted_len = table[offset + 1] as usize;
        if formatted_len < 4 {
            break;
        }
        if offset + formatted_len > table.len() {
            break;
        }
        let formatted = &table[offset..offset + formatted_len];
        let str_start = offset + formatted_len;
        let mut str_end = str_start;
        while str_end + 1 < table.len() {
            if table[str_end] == 0 && table[str_end + 1] == 0 {
                str_end += 2;
                break;
            }
            str_end += 1;
        }
        if str_end > table.len() {
            break;
        }
        if entry_type == type_id
            && string_offset < formatted.len()
            && let Some(s) = decode_smbios_string(&table[str_start..str_end], formatted[string_offset])
        {
            return Some(s);
        }
        if entry_type == 127 {
            break;
        }
        offset = str_end;
    }
    None
}

fn decode_smbios_string(string_area: &[u8], index: u8) -> Option<String> {
    if index == 0 {
        return None;
    }
    let area = string_area.strip_suffix(&[0, 0]).unwrap_or(string_area);
    let mut current = 1u8;
    for part in area.split(|b| *b == 0) {
        if part.is_empty() {
            continue;
        }
        if current == index {
            let value = std::str::from_utf8(part).ok()?.trim();
            return (!value.is_empty()).then(|| value.to_string());
        }
        current = current.saturating_add(1);
    }
    None
}

/// WMI `Win32_Processor.ProcessorId` format: `{EDX:08X}{EAX:08X}` from CPUID leaf 1.
pub(super) fn read_cpu_chip_id() -> Option<String> {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    {
        #[cfg(target_arch = "x86_64")]
        let regs = std::arch::x86_64::__cpuid(1);
        #[cfg(target_arch = "x86")]
        let regs = std::arch::x86::__cpuid(1);
        Some(format!("{:08X}{:08X}", regs.edx, regs.eax))
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
    {
        None
    }
}

/// First hardware DXGI adapter, skipping Microsoft Basic / RDP / software adapters.
pub(super) fn read_xpu_brand() -> Option<String> {
    // SAFETY: DXGI factory creation; no COM apartment required for CreateDXGIFactory1.
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1().ok()? };
    let mut index = 0u32;
    loop {
        let adapter = match unsafe { factory.EnumAdapters1(index) } {
            Ok(adapter) => adapter,
            Err(err) if err.code() == HRESULT(0x887A0002u32 as i32) /* DXGI_ERROR_NOT_FOUND */ => {
                break;
            }
            Err(_) => break,
        };
        index += 1;
        let desc = unsafe { adapter.GetDesc1().ok()? };
        let flags = DXGI_ADAPTER_FLAG(desc.Flags as i32);
        if flags.contains(DXGI_ADAPTER_FLAG_SOFTWARE) {
            continue;
        }
        let name = wchar_to_string(&desc.Description);
        if name.is_empty() || is_virtual_gpu(&name) {
            continue;
        }
        return Some(name);
    }
    None
}

fn wchar_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len]).trim().to_string()
}

fn is_virtual_gpu(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("microsoft basic")
        || lower.contains("remote desktop")
        || lower.contains("virtual")
}

#[cfg(test)]
mod tests {
    use std::mem::MaybeUninit;

    use super::*;

    #[test]
    fn windows_readers_return_usable_values() {
        let mac = read_mac_address().expect("mac");
        assert_eq!(mac.matches(':').count(), 5, "mac={mac}");
        assert!(!mac.contains('-'));

        let sn = read_serial_number().expect("sn");
        assert!(!sn.is_empty());

        let cpu = read_cpu_chip_id().expect("cpu");
        assert_eq!(cpu.len(), 16);
        assert!(cpu.chars().all(|c| c.is_ascii_hexdigit()));

        let xpu = read_xpu_brand().expect("xpu");
        assert!(!xpu.is_empty());
        assert!(!is_virtual_gpu(&xpu));
    }

    #[test]
    fn decode_smbios_string_picks_indexed_entry() {
        let area = b"Vendor\0Version\0SerialXYZ\0\0";
        assert_eq!(
            decode_smbios_string(area, 3).as_deref(),
            Some("SerialXYZ")
        );
        assert_eq!(decode_smbios_string(area, 0), None);
    }

    #[test]
    fn adapter_physical_mac_rejects_loopback_keeps_down() {
        let mut adapter = unsafe { MaybeUninit::<IP_ADAPTER_ADDRESSES_LH>::zeroed().assume_init() };
        adapter.OperStatus = IfOperStatusUp;
        adapter.IfType = 24;
        adapter.PhysicalAddressLength = 6;
        adapter.PhysicalAddress[..6].copy_from_slice(&[1, 2, 3, 4, 5, 6]);
        assert_eq!(adapter_physical_mac(&adapter), None);

        adapter.IfType = 6;
        adapter.OperStatus = windows::Win32::NetworkManagement::Ndis::IfOperStatusDown;
        assert_eq!(adapter_physical_mac(&adapter), Some([1, 2, 3, 4, 5, 6]));

        adapter.OperStatus = IfOperStatusUp;
        assert_eq!(adapter_physical_mac(&adapter), Some([1, 2, 3, 4, 5, 6]));
    }
}
