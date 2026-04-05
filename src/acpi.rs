use crate::{hpet::HpetRegisters, result::Result};
use core::mem::size_of;

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct SystemDescriptionTableHeader {
    signature: [u8; 4],
    length: u32,
    _unused: [u8; 28],
}

const _: () = assert!(core::mem::size_of::<SystemDescriptionTableHeader>() == 36);

impl SystemDescriptionTableHeader {
    fn expect_signature(&self, sig: &'static [u8; 4]) {
        assert_eq!(self.signature, *sig);
    }

    fn signature(&self) -> &[u8; 4] {
        &self.signature
    }
}

struct XsdtIterator<'a> {
    table: &'a Xsdt,
    index: usize,
}

impl<'a> XsdtIterator<'a> {
    pub fn new(table: &'a Xsdt) -> Self {
        Self { table, index: 0 }
    }
}

impl<'a> Iterator for XsdtIterator<'a> {
    type Item = &'static SystemDescriptionTableHeader;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.table.num_of_entiries() {
            None
        } else {
            self.index += 1;
            Some(unsafe {
                &*(self.table.entry(self.index - 1) as *const SystemDescriptionTableHeader)
            })
        }
    }
}

#[repr(packed)]
struct Xsdt {
    header: SystemDescriptionTableHeader,
}

const _: () = assert!(core::mem::size_of::<Xsdt>() == 36);

impl Xsdt {
    fn find_table(&self, sig: &'static [u8; 4]) -> Option<&'static SystemDescriptionTableHeader> {
        self.iter().find(|t| t.signature() == sig)
    }

    fn header_size(&self) -> usize {
        size_of::<SystemDescriptionTableHeader>()
    }

    fn num_of_entiries(&self) -> usize {
        (self.header.length as usize - self.header_size()) / size_of::<*const u8>()
    }

    unsafe fn entry(&self, index: usize) -> *const u8 {
        unsafe {
            ((self as *const Self as *const u8).add(self.header_size()) as *const *const u8)
                .add(index)
                .read_unaligned()
        }
    }

    fn iter(&self) -> XsdtIterator {
        XsdtIterator::new(self)
    }
}

// ACPI: Advanced Configuration and Power Interface
trait AcpiTable {
    const SIGNATURE: &'static [u8; 4];
    type Table;

    fn new(header: &SystemDescriptionTableHeader) -> &Self::Table {
        header.expect_signature(Self::SIGNATURE);

        let mcfg: &Self::Table =
            unsafe { &*(header as *const SystemDescriptionTableHeader as *const Self::Table) };
        mcfg
    }
}

// メモリだけでなく、さまざまなアドレス空間を表現できる汎用的なアドレス構造体
#[repr(packed)]
pub struct GenericAddress {
    address_space_id: u8, // どのアドレス空間を表すか
    _unused: [u8; 3],
    address: u64, // アドレス空間内のアドレス
}

const _: () = assert!(core::mem::size_of::<GenericAddress>() == 12);

impl GenericAddress {
    pub fn address_in_memory_space(&self) -> Result<usize> {
        if self.address_space_id == 0 {
            Ok(self.address as usize)
        } else {
            Err("ACPI Generic Address is not in system memory space")
        }
    }
}

#[repr(packed)]
pub struct AcpiHpetDescriptor {
    _header: SystemDescriptionTableHeader,
    _reserved0: u32,
    address: GenericAddress, // HPETを操作するためのメモリ上の場所を示す
    _reserved1: u32,
}

impl AcpiTable for AcpiHpetDescriptor {
    const SIGNATURE: &'static [u8; 4] = b"HPET";
    type Table = Self;
}

impl AcpiHpetDescriptor {
    pub fn base_address(&self) -> Result<&'static mut HpetRegisters> {
        unsafe {
            self.address
                .address_in_memory_space()
                .map(|addr| &mut *(addr as *mut HpetRegisters))
        }
    }
}

const _: () = assert!(core::mem::size_of::<AcpiHpetDescriptor>() == 56);

// ACPIのルートテーブルであるRSDP (Root System Description Pointer)を表す構造体
#[repr(C)]
#[derive(Debug)]
pub struct AcpiRsdpStruct {
    signature: [u8; 8],
    checksum: u8,
    oem_id: [u8; 6],
    revision: u8,
    rsdt_address: u32, // RSDT (Root System Description Table)のアドレス
    length: u32,
    xsdt: u64, // XSDT (Extended System Description Table)のアドレス
}

impl AcpiRsdpStruct {
    fn xsdt(&self) -> &Xsdt {
        unsafe { &*(self.xsdt as *const Xsdt) }
    }

    // High Prescision Event Timer
    // タイマーデバイス。Memory Mapped I/Oでアクセスできる。
    pub fn hpet(&self) -> Option<&AcpiHpetDescriptor> {
        let xsdt = self.xsdt();
        xsdt.find_table(b"HPET").map(AcpiHpetDescriptor::new)
    }
}
