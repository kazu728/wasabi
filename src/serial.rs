use crate::x86_64::busy_loop_hint;
use crate::x86_64::read_io_port_u8;
use crate::x86_64::write_io_port_u8;
use core::fmt;
use core::sync::atomic::AtomicU8;
use core::sync::atomic::Ordering;

const COM1_BASE_PORT: u16 = 0x3f8;
const DATA_REGISTER_OFFSET: u16 = 0;
const INTERRUPT_ENABLE_REGISTER_OFFSET: u16 = 1;
const FIFO_CONTROL_REGISTER_OFFSET: u16 = 2;
const LINE_CONTROL_REGISTER_OFFSET: u16 = 3;
const MODEM_CONTROL_REGISTER_OFFSET: u16 = 4;
const LINE_STATUS_REGISTER_OFFSET: u16 = 5;
const DLAB_ENABLE: u8 = 0x80;
const WORD_LENGTH_8_BITS: u8 = 0x03;
const FIFO_ENABLE_CLEAR_AND_TRIGGER_14_BYTES: u8 = 0xc7;
const MODEM_CONTROL_IRQS_ENABLED_RTS_DSR: u8 = 0x0b;
const TRANSMIT_HOLDING_REGISTER_EMPTY: u8 = 0x20;
const BAUD_DIVISOR_115200: u16 = 0x0001;

const COM1_UNINITIALIZED: u8 = 0;
const COM1_INITIALIZING: u8 = 1;
const COM1_INITIALIZED: u8 = 2;

static COM1_INIT_STATE: AtomicU8 = AtomicU8::new(COM1_UNINITIALIZED);

pub struct SerialPort {
    base: u16,
}

impl SerialPort {
    pub fn new(base: u16) -> Self {
        Self { base }
    }

    pub fn new_for_com1() -> Self {
        Self::new(COM1_BASE_PORT)
    }

    pub fn init(&mut self) {
        // Disable all interrupts.
        write_io_port_u8(self.base + INTERRUPT_ENABLE_REGISTER_OFFSET, 0x00);
        // Enable DLAB to configure the baud divisor.
        write_io_port_u8(self.base + LINE_CONTROL_REGISTER_OFFSET, DLAB_ENABLE);
        write_io_port_u8(
            self.base + DATA_REGISTER_OFFSET,
            (BAUD_DIVISOR_115200 & 0xff) as u8,
        );
        write_io_port_u8(
            self.base + INTERRUPT_ENABLE_REGISTER_OFFSET,
            (BAUD_DIVISOR_115200 >> 8) as u8,
        );
        // 8 data bits, no parity, one stop bit.
        write_io_port_u8(self.base + LINE_CONTROL_REGISTER_OFFSET, WORD_LENGTH_8_BITS);
        // Enable FIFO, clear both queues, set 14-byte threshold.
        write_io_port_u8(
            self.base + FIFO_CONTROL_REGISTER_OFFSET,
            FIFO_ENABLE_CLEAR_AND_TRIGGER_14_BYTES,
        );
        // Mark terminal ready and enable IRQs.
        write_io_port_u8(
            self.base + MODEM_CONTROL_REGISTER_OFFSET,
            MODEM_CONTROL_IRQS_ENABLED_RTS_DSR,
        );
    }

    pub fn write_byte(&mut self, byte: u8) {
        self.ensure_initialized_for_write();
        self.write_byte_unchecked(byte);
    }

    fn write_byte_unchecked(&mut self, byte: u8) {
        if byte == b'\n' {
            self.write_byte_raw(b'\r');
        }
        self.write_byte_raw(byte);
    }

    fn write_byte_raw(&mut self, byte: u8) {
        while !self.is_transmit_holding_register_empty() {
            busy_loop_hint();
        }
        write_io_port_u8(self.base + DATA_REGISTER_OFFSET, byte);
    }

    fn is_transmit_holding_register_empty(&self) -> bool {
        read_io_port_u8(self.base + LINE_STATUS_REGISTER_OFFSET) & TRANSMIT_HOLDING_REGISTER_EMPTY
            != 0
    }

    fn ensure_initialized_for_write(&self) {
        if self.base == COM1_BASE_PORT {
            ensure_com1_initialized();
        }
    }
}

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.ensure_initialized_for_write();
        for byte in s.bytes() {
            self.write_byte_unchecked(byte);
        }
        Ok(())
    }
}

impl Default for SerialPort {
    fn default() -> Self {
        Self::new_for_com1()
    }
}

pub fn ensure_com1_initialized() {
    loop {
        match COM1_INIT_STATE.compare_exchange(
            COM1_UNINITIALIZED,
            COM1_INITIALIZING,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                let mut port = SerialPort::new_for_com1();
                port.init();
                COM1_INIT_STATE.store(COM1_INITIALIZED, Ordering::Release);
                return;
            }
            Err(COM1_INITIALIZING) => busy_loop_hint(),
            Err(COM1_INITIALIZED) => return,
            Err(_) => unreachable!(),
        }
    }
}
