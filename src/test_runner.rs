use crate::qemu::exit_qemu;
use crate::qemu::QemuExitCode;
use crate::x86_64::inb;
use crate::x86_64::outb;
use core::any::type_name;
use core::fmt;
use core::fmt::Write;
use core::hint::spin_loop;
use core::panic::PanicInfo;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;

const COM1_PORT: u16 = 0x3f8;
const SERIAL_LINE_STATUS_PORT: u16 = COM1_PORT + 5;
const TEST_RESULT_OK_MARKER: &str = "WASABI_TEST_RESULT:OK";
const TEST_RESULT_FAIL_MARKER: &str = "WASABI_TEST_RESULT:FAIL";

static SERIAL_INITIALIZED: AtomicBool = AtomicBool::new(false);

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
    init_serial_once();
    let mut writer = SerialWriter;
    writer.write_fmt(args).unwrap();
}

fn init_serial_once() {
    if SERIAL_INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    unsafe {
        outb(COM1_PORT + 1, 0x00);
        outb(COM1_PORT + 3, 0x80);
        outb(COM1_PORT, 0x03);
        outb(COM1_PORT + 1, 0x00);
        outb(COM1_PORT + 3, 0x03);
        outb(COM1_PORT + 2, 0xc7);
        outb(COM1_PORT + 4, 0x0b);
    }

    SERIAL_INITIALIZED.store(true, Ordering::Release);
}

struct SerialWriter;

impl Write for SerialWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            write_serial_byte(byte);
        }
        Ok(())
    }
}

fn write_serial_byte(byte: u8) {
    unsafe {
        if byte == b'\n' {
            write_serial_byte_raw(b'\r');
        }
        write_serial_byte_raw(byte);
    }
}

unsafe fn write_serial_byte_raw(byte: u8) {
    while unsafe { inb(SERIAL_LINE_STATUS_PORT) & 0x20 } == 0 {
        spin_loop();
    }
    unsafe {
        outb(COM1_PORT, byte);
    }
}
