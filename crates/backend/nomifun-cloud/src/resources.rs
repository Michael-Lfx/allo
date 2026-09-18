//! Best-effort host resource snapshot for activation telemetry.

use sysinfo::{Disks, System};

/// Total physical RAM in mebibytes, when readable.
pub fn total_ram_mb() -> Option<u64> {
    let mut system = System::new();
    system.refresh_memory();
    let bytes = system.total_memory();
    if bytes == 0 {
        None
    } else {
        Some(bytes / (1024 * 1024))
    }
}

/// Largest available free space across mounted disks, in gibibytes.
pub fn largest_disk_free_gb() -> Option<u64> {
    let disks = Disks::new_with_refreshed_list();
    let free_bytes = disks
        .list()
        .iter()
        .map(|disk| disk.available_space())
        .max()
        .unwrap_or(0);
    if free_bytes == 0 {
        None
    } else {
        Some(free_bytes / (1024 * 1024 * 1024))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_probes_return_positive_or_none() {
        if let Some(ram) = total_ram_mb() {
            assert!(ram > 0);
        }
        if let Some(free) = largest_disk_free_gb() {
            assert!(free > 0);
        }
    }
}
