use crate::x86_64::outb;
use core::hint::spin_loop;

const QEMU_EXIT_PORT: u16 = 0xf4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum QemuExitCode {
    Success = 0x10,
    Failed = 0x11,
}

pub fn exit_qemu(code: QemuExitCode) -> ! {
    unsafe {
        outb(QEMU_EXIT_PORT, code as u8);
    }
    loop {
        spin_loop();
    }
}
