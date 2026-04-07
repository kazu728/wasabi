use crate::acpi::AcpiRsdpStruct;
use crate::graphics::Bitmap;
use crate::result::Result;
use core::mem::offset_of;
use core::mem::size_of;
use core::ptr::null_mut;

type EfiVoid = u8;
pub type EfiHandle = u64;

// UEFIのプロトコルを識別するためのGUID構造体と、Graphics Output ProtocolのGUID定数
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct EfiGuid {
    pub data0: u32,
    pub data1: u16,
    pub data2: u16,
    pub data3: [u8; 8],
}

const EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID: EfiGuid = EfiGuid {
    data0: 0x9042a9de,
    data1: 0x23dc,
    data2: 0x4a38,
    data3: [0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a],
};

const EFI_LOADED_IMAGE_PROTOCOL_GUID: EfiGuid = EfiGuid {
    data0: 0x5B1B31A1,
    data1: 0x9562,
    data2: 0x11d2,
    data3: [0x8E, 0x3F, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
};

const EFI_ACPI_TABLE_GUID: EfiGuid = EfiGuid {
    data0: 0x8868e871,
    data1: 0xe4f1,
    data2: 0x11d3,
    data3: [0xbc, 0x22, 0x00, 0x80, 0xc7, 0x3c, 0x88, 0x81],
};

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
#[must_use]
#[repr(u64)]
pub enum EfiStatus {
    Success = 0,
}

#[repr(C)]
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
#[allow(non_camel_case_types)]
pub enum EfiMemoryType {
    RESERVED = 0,
    LOADER_CODE,
    LOADER_DATA,
    BOOT_SERVICES_CODE,
    BOOT_SERVICES_DATA,
    RUNTIME_SERVICES_CODE,
    RUNTIME_SERVICES_DATA,
    CONVENTIONAL_MEMORY,
    UNUSABLE_MEMORY,
    ACPI_RECLAIM_MEMORY,
    ACPI_MEMORY_NVS,
    MEMORY_MAPPED_IO,
    MEMORY_MAPPED_IO_PORT_SPACE,
    PAL_CODE,
    PERSISTENT_MEMORY,
}

#[repr(C)]
#[derive(Debug)]
pub struct EfiMemoryDescriptor {
    pub memory_type: EfiMemoryType,
    pub physical_start: u64,
    virtual_start: u64,
    pub number_of_pages: u64,
    attribute: u64,
}

const MEMORY_MAP_BUFFER_SIZE: usize = 0x8000;

// UEFIのGetMemoryMap関数の呼び出しに必要なバッファと関連情報を保持する
pub struct MemoryMapHolder {
    memory_map_buffer: [u8; MEMORY_MAP_BUFFER_SIZE], // GetMemoryMap関数がメモリマップを格納するためのバッファ
    memory_map_size: usize, // バッファのサイズ、GetMemoryMap関数の呼び出し前に設定する必要がある
    map_key: usize, // GetMemoryMap関数が返すマップキー、ExitBootServices関数の呼び出しに必要
    descriptor_size: usize, // 各ディスクリプタのサイズ
    descriptor_version: u32, // ディスクリプタのバージョン
}

pub struct MemoryMapIterator<'a> {
    map: &'a MemoryMapHolder,
    offset: usize,
}

impl<'a> Iterator for MemoryMapIterator<'a> {
    type Item = &'a EfiMemoryDescriptor;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset >= self.map.memory_map_size {
            return None;
        } else {
            let e: &EfiMemoryDescriptor = unsafe {
                &*(self.map.memory_map_buffer.as_ptr().add(self.offset)
                    as *const EfiMemoryDescriptor)
            };
            self.offset += self.map.descriptor_size;
            Some(e)
        }
    }
}

impl MemoryMapHolder {
    pub const fn new() -> Self {
        Self {
            memory_map_buffer: [0; MEMORY_MAP_BUFFER_SIZE],
            memory_map_size: MEMORY_MAP_BUFFER_SIZE,
            map_key: 0,
            descriptor_size: 0,
            descriptor_version: 0,
        }
    }

    pub fn iter(&self) -> MemoryMapIterator {
        MemoryMapIterator {
            map: self,
            offset: 0,
        }
    }
}

// ブート中に使えるサービス関数のテーブル
// UEFIは既存のポインタ値で関数を呼び出すため、ファームウェアが期待するメモリレイアウトと一致させる
#[repr(C)]
pub struct EfiBootServicesTable {
    _reserved0: [u64; 7],
    get_memory_map: extern "win64" fn(
        memory_map_size: *mut usize,
        memory_map: *mut EfiVoid,
        map_key: *mut usize,
        descriptor_size: *mut usize,
        descriptor_version: *mut u32,
    ) -> EfiStatus,
    _reserved2: [u64; 11],
    handle_protocol: extern "win64" fn(
        handle: EfiHandle,
        protocol: *const EfiGuid,
        interface: *mut *mut EfiVoid,
    ) -> EfiStatus,
    _reserved1: [u64; 9],
    exit_boot_services: extern "win64" fn(image_handle: EfiHandle, map_key: usize) -> EfiStatus,
    _reserved4: [u64; 10],
    locate_protocol: extern "win64" fn(
        protocol: *const EfiGuid,
        registration: *const EfiVoid,
        interface: *mut *mut EfiVoid,
    ) -> EfiStatus,
}

impl EfiBootServicesTable {
    pub fn get_memory_map(&self, map: &mut MemoryMapHolder) -> EfiStatus {
        (self.get_memory_map)(
            &mut map.memory_map_size,
            map.memory_map_buffer.as_mut_ptr(),
            &mut map.map_key,
            &mut map.descriptor_size,
            &mut map.descriptor_version,
        )
    }
}

// UEFI仕様の外で定義されているテーブルや構造体を定義するモジュール
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiConfigurationTable {
    vendor_guid: EfiGuid,
    pub vendor_table: *const EfiVoid,
}

// UEFIシステムテーブルの定義、UEFIがエントリポイントに渡す引数の構造体
#[repr(C)]
pub struct EfiSystemTable {
    _reserved0: [u64; 12],
    pub boot_services: &'static EfiBootServicesTable,
    number_of_table_entries: usize,
    configuration_table: *const EfiConfigurationTable,
}

impl EfiSystemTable {
    pub fn acpi_table(&self) -> Option<&'static AcpiRsdpStruct> {
        self.lookup_config_table(&EFI_ACPI_TABLE_GUID)
            .map(|t| unsafe { &*(t.vendor_table as *const AcpiRsdpStruct) })
    }

    fn lookup_config_table(&self, guid: &EfiGuid) -> Option<EfiConfigurationTable> {
        for i in 0..self.number_of_table_entries {
            let ct = unsafe { &*self.configuration_table.add(i) };
            if ct.vendor_guid == *guid {
                return Some(*ct);
            }
        }
        None
    }
}

#[repr(C)]
#[derive(Debug)]
pub(crate) struct EfiGraphicsOutputProtocolPixelInfo {
    pub version: u32,
    pub horizontal_resolution: u32,
    pub vertical_resolution: u32,
    _padding: [u32; 5],
    pub pixels_per_scan_line: u32,
}

#[repr(C)]
#[derive(Debug)]
pub(crate) struct EfiGraphicsOutputProtocolMode<'a> {
    pub max_mode: u32,
    pub mode: u32,
    pub info: &'a EfiGraphicsOutputProtocolPixelInfo,
    pub size_of_info: u64,
    pub frame_buffer_base: usize,
    pub frame_buffer_size: usize,
}

#[repr(C)]
#[derive(Debug)]
pub(crate) struct EfiGraphicsOutputProtocol<'a> {
    reserved: [u64; 3],
    pub mode: &'a EfiGraphicsOutputProtocolMode<'a>,
}
// Return a reference to the Graphics Output Protocol for controlling the frame buffer
pub(crate) fn locate_graphic_protocol<'a>(
    efi_system_table: &'a EfiSystemTable,
) -> Result<&'a EfiGraphicsOutputProtocol<'a>> {
    let mut graphic_output_protocol = null_mut::<EfiGraphicsOutputProtocol>();
    let status = (efi_system_table.boot_services.locate_protocol)(
        &EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID,
        null_mut::<EfiVoid>(),
        &mut graphic_output_protocol as *mut *mut EfiGraphicsOutputProtocol as *mut *mut EfiVoid,
    );

    if status != EfiStatus::Success {
        return Err("Failed to locate Graphics Output Protocol");
    }

    Ok(unsafe { &*graphic_output_protocol })
}

pub struct EfiLoadedImageProtocol {
    _reserved: [u64; 8],
    pub image_base: u64,
    pub image_size: u64,
}

pub fn locate_loaded_image_protocol<'a>(
    image_handle: EfiHandle,
    efi_system_table: &'a EfiSystemTable,
) -> Result<&'a EfiLoadedImageProtocol> {
    let mut graphic_output_protocol = null_mut::<EfiLoadedImageProtocol>();
    let status = (efi_system_table.boot_services.handle_protocol)(
        image_handle,
        &EFI_LOADED_IMAGE_PROTOCOL_GUID,
        &mut graphic_output_protocol as *mut *mut EfiLoadedImageProtocol as *mut *mut EfiVoid,
    );

    if status != EfiStatus::Success {
        return Err("Failed to locate Loaded Image Protocol");
    }

    Ok(unsafe { &*graphic_output_protocol })
}

pub struct VramBufferInfo {
    pub(crate) buf: *mut u8,
    pub width: i64,
    pub height: i64,
    pub(crate) pixels_per_line: i64,
}

impl Bitmap for VramBufferInfo {
    fn bytes_per_pixel(&self) -> i64 {
        4
    }
    fn pixels_per_line(&self) -> i64 {
        self.pixels_per_line
    }
    fn width(&self) -> i64 {
        self.width
    }
    fn height(&self) -> i64 {
        self.height
    }
    fn buf_mut(&mut self) -> *mut u8 {
        self.buf
    }
}

pub fn init_vram(efi_system_table: &EfiSystemTable) -> Result<VramBufferInfo> {
    let gp = locate_graphic_protocol(efi_system_table)?;
    Ok(VramBufferInfo {
        buf: gp.mode.frame_buffer_base as *mut u8,
        width: gp.mode.info.horizontal_resolution as i64,
        height: gp.mode.info.vertical_resolution as i64,
        pixels_per_line: gp.mode.info.pixels_per_scan_line as i64,
    })
}

// UEFIからOSに制御を移交する
pub fn exit_from_efi_boot_services(
    image_handle: EfiHandle,
    efi_system_table: &EfiSystemTable,
    memory_map: &mut MemoryMapHolder,
) {
    loop {
        let status = efi_system_table.boot_services.get_memory_map(memory_map);
        assert_eq!(status, EfiStatus::Success);

        let status =
            (efi_system_table.boot_services.exit_boot_services)(image_handle, memory_map.map_key);
        if status == EfiStatus::Success {
            break;
        }
    }
}

const _: () = assert!(offset_of!(EfiBootServicesTable, get_memory_map) == 56);
const _: () = assert!(offset_of!(EfiBootServicesTable, exit_boot_services) == 232);
const _: () = assert!(offset_of!(EfiBootServicesTable, locate_protocol) == 320);
const _: () = assert!(offset_of!(EfiSystemTable, boot_services) == 96);
const _: () = assert!(size_of::<EfiGraphicsOutputProtocolPixelInfo>() == 36);
