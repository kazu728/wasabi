#![no_std]
#![feature(custom_test_frameworks)]
#![test_runner(crate::test_runner::test_runner)]
#![reexport_test_harness_main = "test_main"]
#![no_main]

pub mod graphics;
pub mod qemu;
pub mod result;
pub mod test_runner;
pub mod uefi;
pub mod x86_64;

#[cfg(test)]
#[no_mangle]
fn efi_main(_image_handle: uefi::EfiHandle, _efi_system_table: &uefi::EfiSystemTable) {
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
