#![no_std]
#![no_main]

use core::arch::asm;
use core::cmp::min;
use core::mem::offset_of;
use core::mem::size_of;
use core::panic::PanicInfo;
use core::ptr::null_mut;

type EfiVoid = u8;
type EfiHandle = u64;
type Result<T> = core::result::Result<T, &'static str>;

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
#[must_use]
#[repr(u64)]
enum EfiStatus {
    Success = 0,
}

pub fn hlt() {
    unsafe {
        asm!("hlt");
    }
}

#[no_mangle]
fn efi_main(_image_handle: EfiHandle, efi_system_table: &EfiSystemTable) {
    let mut vram = init_vram(efi_system_table).expect("Failed to initialize VRAM");
    let vw = vram.width;
    let vh = vram.height;

    fill_rect(&mut vram, 0x00000, 0, 0, vw, vh).unwrap();
    fill_rect(&mut vram, 0xff0000, 32, 32, 32, 32).unwrap();
    fill_rect(&mut vram, 0x00ff00, 64, 64, 64, 64).unwrap();
    fill_rect(&mut vram, 0x0000ff, 128, 128, 128, 128).unwrap();

    for i in 0..256 {
        draw_point(&mut vram, 0x010101 * i as u32, i, i).unwrap();
    }

    let grid_size: i64 = 32;
    let rect_size: i64 = grid_size * 8;

    for i in (0..=rect_size).step_by(grid_size as usize) {
        draw_line(&mut vram, 0xff0000, 0, i, rect_size, i).unwrap();
        draw_line(&mut vram, 0xff0000, i, 0, i, rect_size).unwrap();
    }

    let cx = rect_size / 2;
    let cy = rect_size / 2;

    for i in (0..=rect_size).step_by(grid_size as usize) {
        draw_line(&mut vram, 0xffff00, cx, cy, 0, i).unwrap();
        draw_line(&mut vram, 0x00ffff, cx, cy, i, 0).unwrap();
        draw_line(&mut vram, 0xff00ff, cx, cy, rect_size, i).unwrap();
        draw_line(&mut vram, 0xffffff, cx, cy, i, rect_size).unwrap();
    }

    for (i, c) in "ABCDEF".chars().enumerate() {
        draw_font_fg(&mut vram, i as i64 * 16 + 256, i as i64 * 16, 0xffffff, c);
    }
    draw_str_fg(&mut vram, 256, 256, 0xffffff, "Hello, world!");

    loop {
        hlt();
    }
}

// 文字cに対応するフォントデータを取得する
fn lookup_font(c: char) -> Option<[[char; 8]; 16]> {
    const FONT_SOURCE: &str = include_str!("font.txt");

    if let Ok(c) = u8::try_from(c) {
        let mut fi = FONT_SOURCE.split('\n');

        while let Some(line) = fi.next() {
            if let Some(line) = line.strip_prefix("0x") {
                if let Ok(idx) = u8::from_str_radix(line, 16) {
                    if idx != c {
                        continue;
                    }
                    let mut font = [[' '; 8]; 16];
                    for (y, line) in fi.by_ref().take(16).enumerate() {
                        for (x, c) in line.chars().enumerate() {
                            if let Some(e) = font[y].get_mut(x) {
                                *e = c;
                            }
                        }
                    }
                    return Some(font);
                }
            }
        }
    }

    None
}

fn draw_font_fg<T: Bitmap>(buf: &mut T, x: i64, y: i64, color: u32, c: char) {
    if let Some(font) = lookup_font(c) {
        for (dy, line) in font.iter().enumerate() {
            for (dx, pixel) in line.iter().enumerate() {
                let color = match pixel {
                    '*' => color,
                    _ => continue,
                };
                draw_point(buf, color, x + dx as i64, y + dy as i64).unwrap();
            }
        }
    }
}

fn draw_str_fg<T: Bitmap>(buf: &mut T, x: i64, y: i64, color: u32, s: &str) {
    for (i, c) in s.chars().enumerate() {
        draw_font_fg(buf, x + i as i64 * 8, y, color, c);
    }
}

// 傾斜のエンドポイントを計算する
fn calc_slope_endpoint(
    da: i64, // 主軸方向のデルタ
    db: i64, // 従軸方向のデルタ
    ia: i64, // 主軸方向の現在位置
) -> Option<i64> {
    if da < db {
        None
    } else if da == 0 {
        Some(0)
    } else if (0..=da).contains(&ia) {
        Some((2 * db * ia + da) / da / 2)
    } else {
        None
    }
}

// 線分を描画する
fn draw_line<T: Bitmap>(buf: &mut T, color: u32, x0: i64, y0: i64, x1: i64, y1: i64) -> Result<()> {
    if !buf.is_in_x_range(x0)
        || !buf.is_in_y_range(y0)
        || !buf.is_in_x_range(x1)
        || !buf.is_in_y_range(y1)
    {
        return Err("Out of range");
    }

    let dx = (x1 - x0).abs();
    let sx = (x1 - x0).signum();

    let dy = (y1 - y0).abs();
    let sy = (y1 - y0).signum();

    if sx >= dy {
        for (rx, ry) in (0..dx).flat_map(|rx| calc_slope_endpoint(dx, dy, rx).map(|ry| (rx, ry))) {
            draw_point(buf, color, x0 + rx * sx, y0 + ry * sy)?;
        }
    } else {
        for (rx, ry) in (0..dy).flat_map(|ry| calc_slope_endpoint(dy, dx, ry).map(|rx| (rx, ry))) {
            draw_point(buf, color, x0 + rx * sx, y0 + ry * sy)?;
        }
    }
    Ok(())
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        hlt();
    }
}

trait Bitmap {
    fn bytes_per_pixel(&self) -> i64;
    fn pixels_per_line(&self) -> i64;
    fn width(&self) -> i64;
    fn height(&self) -> i64;
    fn buf_mut(&mut self) -> *mut u8;

    // Returned pointer is invalid as long as the given coordinates are out of bounds.
    unsafe fn unchecked_pixel_at_mut(&mut self, x: i64, y: i64) -> *mut u32 {
        unsafe {
            self.buf_mut()
                .add(((y * self.pixels_per_line() + x) * self.bytes_per_pixel()) as usize)
                as *mut u32
        }
    }

    // 2次元座標(x, y)から、フレームバッファ内の対応するピクセルへのポインタを計算する
    fn pixel_at_mut(&mut self, x: i64, y: i64) -> Option<*mut u32> {
        if self.is_in_x_range(x) && self.is_in_y_range(y) {
            Some(unsafe { self.unchecked_pixel_at_mut(x, y) })
        } else {
            None
        }
    }

    fn is_in_x_range(&self, px: i64) -> bool {
        0 <= px && px < min(self.width(), self.pixels_per_line())
    }
    fn is_in_y_range(&self, py: i64) -> bool {
        0 <= py && py < self.height()
    }
}

struct VramBufferInfo {
    buf: *mut u8,
    width: i64,
    height: i64,
    pixels_per_line: i64,
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

fn init_vram(efi_system_table: &EfiSystemTable) -> Result<VramBufferInfo> {
    let gp = locate_graphic_protocol(efi_system_table)?;
    Ok(VramBufferInfo {
        buf: gp.mode.frame_buffer_base as *mut u8,
        width: gp.mode.info.horizontal_resolution as i64,
        height: gp.mode.info.vertical_resolution as i64,
        pixels_per_line: gp.mode.info.pixels_per_scan_line as i64,
    })
}

// ピクセル(x, y)に色colorを描画する（範囲チェックなし）
fn unchecked_draw_point<T: Bitmap>(buf: &mut T, color: u32, x: i64, y: i64) {
    unsafe {
        *buf.unchecked_pixel_at_mut(x, y) = color;
    }
}

// ピクセル(x, y)に色colorを描画する（範囲チェックあり）
fn draw_point<T: Bitmap>(buf: &mut T, color: u32, x: i64, y: i64) -> Result<()> {
    unsafe {
        *(buf.pixel_at_mut(x, y).ok_or("Out of range")?) = color;
    }
    Ok(())
}

// 矩形領域を色colorで塗りつぶす
fn fill_rect<T: Bitmap>(
    buf: &mut T,
    color: u32,
    px: i64, //矩形の左上角のX座標（開始位置）
    py: i64, //矩形の左上角のY座標（開始位置）
    w: i64,  //矩形の幅
    h: i64,  //矩形の高さ
) -> Result<()> {
    if !buf.is_in_x_range(px)
        || !buf.is_in_y_range(py)
        || !buf.is_in_x_range(px + w - 1)
        || !buf.is_in_y_range(py + h - 1)
    {
        return Err("Out of range");
    }

    for y in py..(py + h) {
        for x in px..(px + w) {
            unchecked_draw_point(buf, color, x, y);
        }
    }

    Ok(())
}

#[repr(C)]
struct EfiBootServicesTable {
    _reserved0: [u64; 40],
    locate_protocol: extern "win64" fn(
        protocol: *const EfiGuid,
        registration: *const EfiVoid,
        interface: *mut *mut EfiVoid,
    ) -> EfiStatus,
}
const _: () = assert!(offset_of!(EfiBootServicesTable, locate_protocol) == 320);

#[repr(C)]
struct EfiSystemTable {
    _reserved0: [u64; 12],
    pub boot_services: &'static EfiBootServicesTable,
}
const _: () = assert!(offset_of!(EfiSystemTable, boot_services) == 96);

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

#[repr(C)]
#[derive(Debug)]
struct EfiGraphicsOutputProtocol<'a> {
    reserved: [u64; 3],
    pub mode: &'a EfiGraphicsOutputProtocolMode<'a>,
}

#[repr(C)]
#[derive(Debug)]
struct EfiGraphicsOutputProtocolMode<'a> {
    pub max_mode: u32,
    pub mode: u32,
    pub info: &'a EfiGraphicsOutputProtocolPixelInfo,
    pub size_of_info: u64,
    pub frame_buffer_base: usize,
    pub frame_buffer_size: usize,
}

#[repr(C)]
#[derive(Debug)]
struct EfiGraphicsOutputProtocolPixelInfo {
    pub version: u32,
    pub horizontal_resolution: u32,
    pub vertical_resolution: u32,
    _padding: [u32; 5],
    pub pixels_per_scan_line: u32,
}
const _: () = assert!(size_of::<EfiGraphicsOutputProtocolPixelInfo>() == 36);

// Return a reference to the Graphics Output Protocol for controlling the frame buffer
fn locate_graphic_protocol<'a>(
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
