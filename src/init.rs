extern crate alloc;

use crate::acpi::AcpiRsdpStruct;
use crate::graphics::draw_test_pattern;
use crate::graphics::fill_rect;
use crate::hpet::set_global_hpet;
use crate::hpet::Hpet;
use crate::info;
use crate::serial_info;
use crate::uefi::exit_from_efi_boot_services;
use crate::uefi::EfiHandle;
use crate::uefi::EfiMemoryType;
use crate::uefi::EfiMemoryType::*;
use crate::uefi::EfiSystemTable;
use crate::uefi::MemoryMapHolder;
use crate::uefi::VramBufferInfo;
use crate::x86_64::write_cr3;
use crate::x86_64::PageAttr;
use crate::x86_64::PAGE_SIZE;
use crate::x86_64::PML4;
use alloc::boxed::Box;
use core::cmp::max;

pub fn init_basic_runtime(
    image_handle: EfiHandle,
    efi_system_table: &EfiSystemTable,
) -> MemoryMapHolder {
    let mut memory_map = MemoryMapHolder::new();
    exit_from_efi_boot_services(image_handle, efi_system_table, &mut memory_map);
    memory_map
}

pub fn init_allocator(memory_map: &MemoryMapHolder) {
    let mut total_memory_pages = 0;

    for e in memory_map.iter() {
        if e.memory_type != EfiMemoryType::CONVENTIONAL_MEMORY {
            continue;
        }
        total_memory_pages += e.number_of_pages;
        serial_info!("{e:?}");
    }

    let total_memory_mib = total_memory_pages * 4096 / 1024 / 1024;
    serial_info!("Total memory: {} MiB", total_memory_mib);
}

pub fn init_display(vram: &mut VramBufferInfo) {
    let vw = vram.width;
    let vh = vram.height;

    fill_rect(vram, 0x000000, 0, 0, vw, vh).unwrap();

    draw_test_pattern(vram);
}

pub fn init_paging(memory_map: &MemoryMapHolder) {
    let mut table = PML4::new();
    let mut end_of_mem = 0x1_0000_0000u64;

    for e in memory_map.iter() {
        match e.memory_type {
            CONVENTIONAL_MEMORY | LOADER_CODE | LOADER_DATA => {
                end_of_mem = max(
                    end_of_mem,
                    e.physical_start + e.number_of_pages * PAGE_SIZE as u64,
                )
            }
            _ => (),
        }
    }

    table
        .create_mapping(0, end_of_mem, 0, PageAttr::ReadWriteKernel)
        .expect("Failed to create page mapping");

    table
        .create_mapping(0, 4096, 0, PageAttr::NotPresent)
        .expect("Failed to create page mapping");

    unsafe { write_cr3(Box::into_raw(table)) };
}

pub fn init_hpet(acpi: &AcpiRsdpStruct) {
    let hpet = acpi.hpet().expect("Failed to get HPET");
    let hpet = hpet
        .base_address()
        .expect("Failed to get HPET base address");

    info!("HPET is at {:#p}", hpet);
    let hpet = Hpet::new(hpet);
    set_global_hpet(hpet);
}
