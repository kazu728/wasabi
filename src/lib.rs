#![no_std]
#![feature(custom_test_frameworks)]
#![feature(sync_unsafe_cell)]
#![test_runner(crate::test_runner::test_runner)]
#![reexport_test_harness_main = "test_main"]
#![no_main]

pub mod acpi;
pub mod allocator;
pub mod executor;
pub mod graphics;
pub mod hpet;
pub mod init;
pub mod mutex;
pub mod print;
pub mod qemu;
pub mod result;
pub mod serial;
pub mod test_runner;
pub mod uefi;
pub mod x86_64;

#[cfg(test)]
#[no_mangle]
fn efi_main(image_handle: uefi::EfiHandle, efi_system_table: &uefi::EfiSystemTable) {
    init::init_basic_runtime(image_handle, efi_system_table);

    // cargo test でコンパイラが #[test_case] を集め、それを実行する関数を自動生成する
    // その関数がtest_main で生成されるため呼出可能
    test_main();
}

#[cfg(test)]
mod tests {
    #[test_case]
    fn smoke_test() {
        assert_eq!(1 + 1, 2);
    }
}
