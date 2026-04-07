#![no_std]
#![no_main]

use core::panic::PanicInfo;
use core::time::Duration;
use wasabi::allocator::ALLOCATOR;
use wasabi::error;
use wasabi::executor::Executor;
use wasabi::executor::Task;
use wasabi::executor::TimeoutFuture;
use wasabi::hpet::global_timestamp;
use wasabi::info;
use wasabi::init::init_allocator;
use wasabi::init::init_basic_runtime;
use wasabi::init::init_display;
use wasabi::init::init_hpet;
use wasabi::init::init_paging;
use wasabi::print::set_global_vram;
use wasabi::println;
use wasabi::uefi::init_vram;
use wasabi::uefi::locate_loaded_image_protocol;
use wasabi::uefi::EfiHandle;
use wasabi::uefi::EfiSystemTable;
use wasabi::warn;
use wasabi::x86_64::hlt_loop;
use wasabi::x86_64::init_exceptions;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    error!("PANIC: {info:?}");
    hlt_loop()
}

#[no_mangle]
fn efi_main(image_handle: EfiHandle, efi_system_table: &EfiSystemTable) {
    println!("Booting WasabiOS...");
    println!("image_handle: {:018X}", image_handle);
    println!("efi_system_table: {:#p}", efi_system_table);

    let loaded_image_protocol = locate_loaded_image_protocol(image_handle, efi_system_table)
        .expect("Failed to locate Loaded Image Protocol");

    println!("image_base = {:#018X}", loaded_image_protocol.image_base);
    println!("image_size = {:#018X}", loaded_image_protocol.image_size);

    info!("info");
    warn!("warn");
    error!("error");

    let mut vram = init_vram(efi_system_table).expect("Failed to initialize VRAM");
    init_display(&mut vram);

    // let mut w = BitmapTextWriter::new(&mut vram);
    set_global_vram(vram);
    let acpi = efi_system_table
        .acpi_table()
        .expect("Failed to get ACPI table");
    let memory_map = init_basic_runtime(image_handle, efi_system_table);
    info!("Hello, Non UEFI world!");

    ALLOCATOR.init_with_nmap(&memory_map);
    init_allocator(&memory_map);

    let (_gdt, _idt) = init_exceptions();
    init_paging(&memory_map);

    init_hpet(acpi);
    let t0 = global_timestamp();

    let task1 = Task::new(async move {
        for i in 100..=103 {
            info!("{i} hpet/main_counter = {:?}", global_timestamp() - t0);
            // yield_execution().await
            TimeoutFuture::new(Duration::from_secs(1)).await
        }
        Ok(())
    });

    let task2 = Task::new(async move {
        for i in 200..=203 {
            info!("{i} hpet/main_counter = {:?}", global_timestamp() - t0);
            // yield_execution().await
            TimeoutFuture::new(Duration::from_secs(2)).await
        }
        Ok(())
    });

    let mut executor = Executor::new();
    executor.enqueue(task1);
    executor.enqueue(task2);

    Executor::run(executor);

    // hlt_loop();
}
