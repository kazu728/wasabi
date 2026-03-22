use crate::qemu::exit_qemu;
use crate::qemu::QemuExitCode;
use crate::serial::SerialPort;
use core::any::type_name;
use core::fmt;
use core::fmt::Write;
use core::panic::PanicInfo;

const TEST_RESULT_OK_MARKER: &str = "WASABI_TEST_RESULT:OK";
const TEST_RESULT_FAIL_MARKER: &str = "WASABI_TEST_RESULT:FAIL";

pub trait Testable {
    fn run(&self);
}

impl<T> Testable for T
where
    T: Fn(),
{
    fn run(&self) {
        serial_log(format_args!("{}...\t", type_name::<T>()));
        self();
        serial_log(format_args!("[ok]\n"));
    }
}

pub fn test_runner(tests: &[&dyn Testable]) {
    serial_log(format_args!("running {} tests\n", tests.len()));
    for test in tests {
        test.run();
    }
    serial_log(format_args!("{TEST_RESULT_OK_MARKER}\n"));
    exit_qemu(QemuExitCode::Success);
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    test_panic_handler(info)
}

pub fn test_panic_handler(info: &PanicInfo) -> ! {
    serial_log(format_args!("[failed]\n"));
    serial_log(format_args!("error: {info}\n"));
    serial_log(format_args!("{TEST_RESULT_FAIL_MARKER}\n"));
    exit_qemu(QemuExitCode::Failed);
}

fn serial_log(args: fmt::Arguments<'_>) {
    let mut writer = SerialPort::new_for_com1();
    writer.write_fmt(args).unwrap();
}
