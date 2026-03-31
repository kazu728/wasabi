extern crate alloc;

use alloc::boxed::Box;
use core::arch::asm;
use core::arch::global_asm;
use core::fmt;
use core::marker::PhantomData;
use core::mem::offset_of;
use core::mem::size_of;
use core::mem::size_of_val;
use core::mem::MaybeUninit;
use core::pin::Pin;

use crate::info;
use crate::{error, result::Result};

pub fn hlt() {
    unsafe {
        asm!("hlt", options(nomem, nostack, preserves_flags));
    }
}

pub fn hlt_loop() -> ! {
    loop {
        hlt();
    }
}

pub fn busy_loop_hint() {
    unsafe { asm!("pause") }
}

pub fn read_io_port_u8(port: u16) -> u8 {
    let mut data: u8;
    unsafe {
        asm!(
            "in al, dx",
            in("dx") port,
            out("al") data,
        )
    }
    data
}

pub fn write_io_port_u8(port: u16, data: u8) {
    unsafe {
        asm!(
            "out dx, al",
            in("dx") port,
            in("al") data,
        )
    }
}

pub fn read_cr3() -> *mut PML4 {
    let mut cr3: *mut PML4;
    unsafe {
        asm!(
            "mov rax, cr3",
            out("rax") cr3,
        )
    }
    cr3
}

pub unsafe fn outb(port: u16, value: u8) {
    unsafe {
        asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

pub unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe {
        asm!(
            "in al, dx",
            in("dx") port,
            out("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

pub const PAGE_SIZE: usize = 4096;
const ATTR_MASK: u64 = 0xFFF;
const ATTR_PRESENT: u64 = 1 << 0;
const ATTR_WRITABLE: u64 = 1 << 1;
const ATTR_WRITE_THROUGH: u64 = 1 << 3;
const ATTR_CACHE_DISABLE: u64 = 1 << 4;

#[derive(Debug, Clone, Copy)]
#[repr(u64)]
pub enum PageAttr {
    NotPresent = 0,
    ReadWriteKernel = ATTR_PRESENT | ATTR_WRITABLE,
    ReadWriteIo = ATTR_PRESENT | ATTR_WRITABLE | ATTR_WRITE_THROUGH | ATTR_CACHE_DISABLE,
}

#[derive(Debug, Eq, PartialEq)]
pub enum TranslationResult {
    PageMapped4K { phys: u64 },
    PageMapped2M { phys: u64 },
    PageMapped1G { phys: u64 },
}

#[repr(transparent)]
pub struct Entry<const LEVEL: usize, const SHIFT: usize, NEXT> {
    value: u64,
    next_type: PhantomData<NEXT>,
}

impl<const LEVEL: usize, const SHIFT: usize, NEXT> Entry<LEVEL, SHIFT, NEXT> {
    fn read_value(&self) -> u64 {
        self.value
    }

    fn is_present(&self) -> bool {
        (self.read_value() & 1 << 0) != 0
    }

    fn is_writable(&self) -> bool {
        (self.read_value() & 1 << 1) != 0
    }

    fn is_user(&self) -> bool {
        (self.read_value() & 1 << 2) != 0
    }

    fn format(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "L{}Entry @ {:#p} {{ {:#018X} {}{}{} ",
            LEVEL,
            self,
            self.read_value(),
            if self.is_present() { "P" } else { "N" },
            if self.is_writable() { "W" } else { "R" },
            if self.is_user() { "U" } else { "S" }
        )?;
        write!(f, " }}")
    }

    // 次のレベルのテーブルへのポインタを返す
    fn table(&self) -> Result<&NEXT> {
        if self.is_present() {
            Ok(unsafe { &*((self.value & !ATTR_MASK) as *const NEXT) })
        } else {
            Err("Page Not Found")
        }
    }

    // 次のレベルのテーブルへの可変なポインタを返す
    fn table_mut(&mut self) -> Result<&mut NEXT> {
        if self.is_present() {
            Ok(unsafe { &mut *((self.value & !ATTR_MASK) as *mut NEXT) })
        } else {
            Err("Page Not Found")
        }
    }

    fn set_page(&mut self, phys: u64, attr: PageAttr) -> Result<()> {
        if phys & ATTR_MASK != 0 {
            return Err("Physical address must be aligned to page size");
        } else {
            self.value = phys | attr as u64;
            Ok(())
        }
    }

    fn populate(&mut self) -> Result<&mut Self> {
        if self.is_present() {
            Err("Page is already populated")
        } else {
            let next: Box<NEXT> = Box::new(unsafe { MaybeUninit::zeroed().assume_init() });
            self.value = (Box::into_raw(next) as u64) | PageAttr::ReadWriteKernel as u64;
            Ok(self)
        }
    }

    fn ensure_populated(&mut self) -> Result<&mut Self> {
        if self.is_present() {
            Ok(self)
        } else {
            self.populate()
        }
    }
}

impl<const LEVEL: usize, const SHIFT: usize, NEXT> fmt::Display for Entry<LEVEL, SHIFT, NEXT> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}

impl<const LEVEL: usize, const SHIFT: usize, NEXT> fmt::Debug for Entry<LEVEL, SHIFT, NEXT> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}

#[repr(align(4096))]
pub struct Table<const LEVEL: usize, const SHIFT: usize, NEXT> {
    entry: [Entry<LEVEL, SHIFT, NEXT>; 512],
}

impl<const LEVEL: usize, const SHIFT: usize, NEXT: core::fmt::Debug> Table<LEVEL, SHIFT, NEXT> {
    fn format(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "L{}Table @ {:#p} {{\n", LEVEL, self)?;
        for i in 0..512 {
            let e = &self.entry[i];
            if !e.is_present() {
                continue;
            }
            writeln!(f, "  [{i:03}] {e}\n")?;
        }
        write!(f, "}}")
    }

    pub fn next_level(&self, index: usize) -> Option<&NEXT> {
        self.entry.get(index).and_then(|e| e.table().ok())
    }

    fn calc_index(&self, addr: u64) -> usize {
        ((addr >> SHIFT) & 0b1_1111_1111) as usize
    }
}

impl<const LEVEL: usize, const SHIFT: usize, NEXT: core::fmt::Debug> fmt::Debug
    for Table<LEVEL, SHIFT, NEXT>
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}

pub type PT = Table<1, 12, [u8; PAGE_SIZE]>;
pub type PD = Table<2, 21, PT>;
pub type PDPT = Table<3, 30, PD>;
pub type PML4 = Table<4, 39, PDPT>;

impl PML4 {
    pub fn new() -> Box<Self> {
        Box::new(Self::default())
    }
    fn default() -> Self {
        unsafe { MaybeUninit::zeroed().assume_init() }
    }

    pub fn create_mapping(
        &mut self,
        virt_start: u64,
        virt_end: u64,
        phys: u64,
        attr: PageAttr,
    ) -> Result<()> {
        if virt_start & ATTR_MASK != 0 {
            return Err("Invalid virt star");
        }
        if virt_end & ATTR_MASK != 0 {
            return Err("Invalid virt end");
        }
        if phys & ATTR_MASK != 0 {
            return Err("Invalid phys");
        }

        for addr in (virt_start..virt_end).step_by(PAGE_SIZE) {
            let index = self.calc_index(addr);
            let table = self.entry[index].ensure_populated()?.table_mut()?;
            let index = table.calc_index(addr);
            let table = table.entry[index].ensure_populated()?.table_mut()?;
            let index = table.calc_index(addr);
            let table = table.entry[index].ensure_populated()?.table_mut()?;
            let index = table.calc_index(addr);
            let pte = &mut table.entry[index];
            pte.set_page(phys + (addr - virt_start), attr)?;
        }
        Ok(())
    }
}

pub unsafe fn write_es(selector: u16) {
    unsafe {
        asm!(
            "mov es, ax",
            in("ax") selector
        );
    }
}

pub fn write_cs(cs: u16) {
    unsafe {
        asm!(
            "lea rax, [rip + 2f]", // 続き先となるラベル 2 のアドレスを rax にロード
            "push cx", // 新しい CS セレクタをスタックに積む
            "push rax", // far jump 先の RIP をスタックに積む
            "ljmp [rsp]",  // スタック上の far pointer を使って CS と RIP を同時に更新する
            "2:", // 新しい CS で実行を再開する位置
            "add rsp, 8 + 2", // スタックに積んだ RIP 8 バイトと CS 2 バイトを片付ける
             in("cx") cs); // cx に新しいコードセグメントセレクタを渡す
    }
}

pub fn wirte_ss(selector: u16) {
    unsafe {
        asm!(
            "mov ss, ax",
            in("ax") selector
        );
    }
}

pub fn write_ds(selector: u16) {
    unsafe {
        asm!(
            "mov ds, ax",
            in("ax") selector
        );
    }
}

pub fn write_fs(selector: u16) {
    unsafe {
        asm!(
            "mov fs, ax",
            in("ax") selector
        );
    }
}

pub fn write_gs(selector: u16) {
    unsafe {
        asm!(
            "mov gs, ax",
            in("ax") selector
        );
    }
}

#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy)]
// タスク切り替えや例外処理のために、CPU の FPU/SIMD 状態を丸ごと退避するための構造体
struct FPUContext {
    data: [u8; 512],
}

#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy)]
// 汎用レジスタの保存用構造体
struct GeneralRegisterContext {
    rax: u64,
    rdx: u64,
    rbx: u64,
    rbp: u64,
    rsi: u64,
    rdi: u64,
    r8: u64,
    r9: u64,
    r10: u64,
    r11: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
    rcx: u64,
}

const _: () = assert!(size_of::<GeneralRegisterContext>() == (16 - 1) * 8);

#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy)]
// 割り込みや例外から復帰するための情報を表す構造体(割り込みが終わったあと、元の実行場所へ戻るための情報)
struct InterruptContext {
    rip: u64,
    cs: u64,
    rflags: u64,
    rsp: u64,
    ss: u64,
}

const _: () = assert!(size_of::<InterruptContext>() == 5 * 8);

#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy)]
struct InterruptInfo {
    fpu_context: FPUContext,
    _dummy: u64,
    greg: GeneralRegisterContext,
    error_code: u64,
    ctx: InterruptContext,
}

const _: () = assert!(size_of::<InterruptInfo>() == (16 + 4 + 1) * 8 + 8 + 512);

impl fmt::Debug for InterruptInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "
        {{
            rip: {:#018X}, CS: {:#06X},
            rsp: {:#018X}, SS: {:#06X},
            rbp: {:#018X},

            rflags: {:#018X},
            error_code: {:#018X},

            rax: {:#018X}, rcx: {:#018X},
            rdx: {:#018X}, rbx: {:#018X},
            rsi: {:#018X}, rdi: {:#018X},
            r8: {:#018X}, r9: {:#018X},
            r10: {:#018X}, r11: {:#018X},
            r12: {:#018X}, r13: {:#018X},
            r14: {:#018X}, r15: {:#018X},
        }}",
            self.ctx.rip,
            self.ctx.cs,
            self.ctx.rsp,
            self.ctx.ss,
            self.greg.rbp,
            self.ctx.rflags,
            self.error_code,
            self.greg.rax,
            self.greg.rcx,
            self.greg.rdx,
            self.greg.rbx,
            self.greg.rsi,
            self.greg.rdi,
            self.greg.r8,
            self.greg.r9,
            self.greg.r10,
            self.greg.r11,
            self.greg.r12,
            self.greg.r13,
            self.greg.r14,
            self.greg.r15,
        )
    }
}

// 割り込みハンドラ(エラーコードを自動で積まないもの)のエントリポイント
macro_rules! interrupt_entrypoint {
    ($index: literal) => {
        global_asm!(concat!(
            ".global interrupt_entrypoint",
            stringify!($index),
            "\n",
            "interrupt_entrypoint",
            stringify!($index),
            ":\n",
            // エラーコードを自動で積まない割り込みでも、共通ハンドラのレイアウトを揃える。
            "push 0\n",
            // rcx を割り込み番号の受け渡しに使う前に退避する。
            "push rcx\n",
            "mov rcx, ",
            stringify!($index),
            "\n",
            "jmp inthandler_common\n",
        ));
    };
}

// エラーコードを自動で積む割り込みのエントリポイント
macro_rules! interrupt_entrypoint_wiht_ecode {
    ($index: literal) => {
        global_asm!(concat!(
            ".global interrupt_entrypoint",
            stringify!($index),
            "\n",
            "interrupt_entrypoint",
            stringify!($index),
            ":\n",
            // rcx を割り込み番号の受け渡しに使う前に退避する。
            "push rcx\n",
            "mov rcx, ",
            stringify!($index),
            "\n",
            "jmp inthandler_common\n",
        ));
    };
}

interrupt_entrypoint!(3);
interrupt_entrypoint!(6);
interrupt_entrypoint_wiht_ecode!(8);
interrupt_entrypoint_wiht_ecode!(13);
interrupt_entrypoint_wiht_ecode!(14);
interrupt_entrypoint!(32);

extern "sysv64" {
    fn interrupt_entrypoint3();
    fn interrupt_entrypoint6();
    fn interrupt_entrypoint8();
    fn interrupt_entrypoint13();
    fn interrupt_entrypoint14();
    fn interrupt_entrypoint32();
}

global_asm!(
    r#"
.global inthandler_common
inthandler_common:
    push r15
    push r14
    push r13
    push r12
    push r11
    push r10
    push r9
    push r8
    push rdi
    push rsi
    push rbp
    push rbx
    push rdx
    push rax

    // FPU/SIMD の状態を保存するための領域を確保して、fxsave 命令で保存する。
    sub rsp, 512 + 8 // 512 バイトの FPUContext と、スタックアラインメントのための余分なスペースを確保する。
    fxsave [rsp] // FPU/SIMD 状態をスタックに保存する。

    // 割り込みハンドラに割り込み情報へのポインタを渡すために、rsp を引数として渡す。
    // 割り込みハンドラはこのポインタから割り込み発生時の CPU 状態を読み取ることができる。
    mov rdi, rsp

    mov rbp , rsp // 元の rsp を rbp に退避する。
    and rsp, -16 // interrupt_handler 呼び出し前に rsp を 16 バイト境界へ揃える。

    // 割り込み番号は rcx に入っているので、rsi にコピーして渡す。
    mov rsi, rcx

    // Rust の割り込みハンドラ関数を呼び出す。割り込み番号と割り込み情報へのポインタが引数として渡される。
    call interrupt_handler

    mov rsp, rbp // 割り込みハンドラから戻ったあと、退避していた元の rsp を復元する。
    fxrstor [rsp] // 割り込み前の FPU/SIMD 状態を復元するために、fxrstor 命令でスタックから復元する。

    // FPUContext の領域をスタックから解放する。
    add rsp, 512 + 8

    pop rax
    pop rdx
    pop rbx
    pop rbp
    pop rsi
    pop rdi
    pop r8
    pop r9
    pop r10
    pop r11
    pop r12
    pop r13
    pop r14
    pop r15

    pop rcx // 割り込み番号をスタックから戻す
    add rsp , 8 // 割り込み番号をスタックから戻すために、スタックポインタを調整する

    iretq
    "#
);

pub fn read_cr2() -> u64 {
    let mut cr2: u64;
    unsafe {
        asm!(
            "mov rax, cr2",
            out("rax") cr2,
        )
    }
    cr2
}

#[no_mangle]
// rdi に割り込み情報へのポインタ(rsp)、rsiに割り込み番号のポインタ（rcx）が渡される。
extern "sysv64" fn interrupt_handler(info: &InterruptInfo, index: usize) {
    error!("Interrupt Info: {:#?}", info);
    error!("Exception {index:#04X}: ");

    match index {
        3 => {
            error!("Breakpoint");
            return;
        }
        6 => error!("Invalid Opcode"),
        8 => error!("Double Fault"),
        13 => {
            error!("General Protection Fault");
            let rip = info.ctx.rip;
            error!("Bytes @ RIP({rip:#018X}): ");
            let rip = rip as *const u8;
            let bytes = unsafe { core::slice::from_raw_parts(rip, 16) };
            error!(" = {bytes:02X?}");
        }
        14 => {
            error!("Page Fault");
            error!("CR2={:#018X}", read_cr2());
            error!(
                "Cause by A {} mode {} on a {} page, page structures are {}",
                if info.error_code & 0b0000_0100 != 0 {
                    "user"
                } else {
                    "supervisor"
                },
                if info.error_code & 0b0001_0000 != 0 {
                    "instruction fetch"
                } else if info.error_code & 0b0000_0010 != 0 {
                    "data write"
                } else {
                    "data read"
                },
                if info.error_code & 0b0001 != 0 {
                    "present"
                } else {
                    "non present"
                },
                if info.error_code & 0b1000 != 0 {
                    "invalid"
                } else {
                    "valid"
                }
            );
        }
        _ => {
            error!("Not handled");
        }
    }
}

#[no_mangle]
extern "sysv64" fn int_handler_unimplemented() {
    panic!("Unexpected interrupt",);
}

// Interrupt Gate は、IDT に入れる「割り込みハンドラの入口の種類」を表す
// CPU に「この割り込みが来たら、どのルールでこのハンドラへ入るか」を伝える
// DPL は Descriptor Privilege Level の略で、「どの特権レベルからこのゲートを使ってよいか」を表す
// PDDRTTTT (TTTT: type, R: reserved, D: DP:, P: present)
pub const BIT_FLAGS_INGATE: u8 = 0b0000_1110u8;
pub const BIT_FLAGS_PRESENT: u8 = 0b1000_0000u8;
pub const BIT_FLAGS_DPL0: u8 = 0 << 5;
pub const BIT_FLAGS_DPL3: u8 = 3 << 5;

#[repr(u8)]
#[derive(Copy, Clone)]
enum IdtAttr {
    _NotPresent = 0,
    IntGateDPL0 = BIT_FLAGS_INGATE | BIT_FLAGS_DPL0 | BIT_FLAGS_PRESENT,
    IntGateDPL3 = BIT_FLAGS_INGATE | BIT_FLAGS_DPL3 | BIT_FLAGS_PRESENT,
}

#[repr(C, packed)]
#[allow(dead_code)]
#[derive(Copy, Clone)]
pub struct IdtDescriptor {
    offset_low: u16,
    segment_selector: u16,
    ist_index: u8,
    attr: IdtAttr,
    offset_mid: u16,
    offset_high: u32,
    _reserved: u32,
}

const _: () = assert!(size_of::<IdtDescriptor>() == 16);

impl IdtDescriptor {
    fn new(
        segment_selector: u16,
        ist_index: u8,
        attr: IdtAttr,
        f: unsafe extern "sysv64" fn(),
    ) -> Self {
        let handler_addr = f as *const unsafe extern "sysv64" fn() as usize;
        Self {
            offset_low: handler_addr as u16,
            segment_selector,
            ist_index,
            attr,
            offset_mid: (handler_addr >> 16) as u16,
            offset_high: (handler_addr >> 32) as u32,
            _reserved: 0,
        }
    }
}

#[allow(dead_code)]
#[repr(C, packed)]
#[derive(Debug)]
struct IdtParameters {
    limit: u16, // IDT のサイズ
    base: *const IdtDescriptor,
}

const _: () = assert!(size_of::<IdtParameters>() == 10);
const _: () = assert!(offset_of!(IdtParameters, base) == 2);

// Interrupt Descriptor Table
// CPU に「割り込みが来たとき、どのアドレスにあるハンドラ関数へ飛べばいいか」を伝えるためのテーブル
pub struct Idt {
    #[allow(dead_code)]
    entries: Pin<Box<[IdtDescriptor; 0x100]>>,
}

impl Idt {
    // 割り込み番号ごとの飛び先表」を作って、その表の場所を lidt で CPU に教える
    pub fn new(segment_selector: u16) -> Self {
        let mut entries = [IdtDescriptor::new(
            segment_selector,
            1,
            IdtAttr::IntGateDPL0,
            int_handler_unimplemented,
        ); 0x100];

        entries[3] = IdtDescriptor::new(
            segment_selector,
            1,
            IdtAttr::IntGateDPL3, // 他はカーネルモードからの割り込みを想定して DPL0 にしているが、INT3 (Breakpoint) はユーザープログラムからも発行される可能性があるので DPL3 にする。
            interrupt_entrypoint3,
        );

        entries[6] = IdtDescriptor::new(
            segment_selector,
            1,
            IdtAttr::IntGateDPL0,
            interrupt_entrypoint6,
        );
        entries[8] = IdtDescriptor::new(
            segment_selector,
            1,
            IdtAttr::IntGateDPL0,
            interrupt_entrypoint8,
        );
        entries[13] = IdtDescriptor::new(
            segment_selector,
            1,
            IdtAttr::IntGateDPL0,
            interrupt_entrypoint13,
        );
        entries[14] = IdtDescriptor::new(
            segment_selector,
            1,
            IdtAttr::IntGateDPL0,
            interrupt_entrypoint14,
        );
        entries[32] = IdtDescriptor::new(
            segment_selector,
            1,
            IdtAttr::IntGateDPL0,
            interrupt_entrypoint32,
        );

        let limit = size_of_val(&entries) as u16;
        let entries = Box::pin(entries);

        let params = IdtParameters {
            limit,
            base: entries.as_ptr(),
        };

        info!("Loading IDT: {:#?}", params);
        unsafe {
            asm!("lidt [rcx]", in("rcx") &params);
        };

        Self { entries }
    }
}

// TSS 割り込みや特権遷移のとき、CPU がどのスタックを使うかを教える設定領域
#[repr(C, packed)]
struct TaskStateSegment64Inner {
    _reserved0: u32,
    _rsp: [u64; 3], // ring0 ~ ring2 までのスタックポインタを指定するための領域
    _ist: [u64; 8], // Interrupt Stack Table の領域。特定の割り込みで、通常のスタックポインタではなく、ここで指定したスタックを使うように設定できる。
    _reserved1: [u16; 5],
    _io_map_base_addr: u16, //
}

const _: () = assert!(size_of::<TaskStateSegment64Inner>() == 104);

pub struct TaskStateSegment64 {
    inner: Pin<Box<TaskStateSegment64Inner>>,
}

impl TaskStateSegment64 {
    //  CPU にロードする TSS ディスクリプタ用に、TSS 本体の物理的な配置アドレスを返す。
    pub fn phys_addr(&self) -> u64 {
        self.inner.as_ref().get_ref() as *const TaskStateSegment64Inner as u64
    }

    // 割り込み用に 64 KiB のメモリを確保し、その領域の上端アドレスをスタックポインタとして返す
    // カーネルが管理するStackなので、メモリの生存期間はカーネルの生存期間と同じであるため、leak して解放しないようにする。
    unsafe fn alloc_interrupt_stack() -> u64 {
        const HANDLER_STACK_SIZE: usize = 64 * 1024;
        let stack = Box::new([0u8; HANDLER_STACK_SIZE]);
        let rsp = unsafe { stack.as_ptr().add(HANDLER_STACK_SIZE) as u64 };
        core::mem::forget(stack);

        rsp
    }

    // CPU が割り込み時に使うスタック情報を全部入れた TSS を作って、固定アドレスに置いて返す
    pub fn new() -> Self {
        let rsp0 = unsafe { Self::alloc_interrupt_stack() };
        let mut ist = [0u64; 8];

        for ist in ist[1..=7].iter_mut() {
            *ist = unsafe { Self::alloc_interrupt_stack() };
        }

        let tss64 = TaskStateSegment64Inner {
            _reserved0: 0,
            _rsp: [rsp0, 0, 0],
            _ist: ist,
            _reserved1: [0; 5],
            _io_map_base_addr: 0,
        };

        let this = Self {
            inner: Box::pin(tss64),
        };

        info!("TSS64 created @ {:#X}", this.phys_addr());
        this
    }
}

impl Drop for TaskStateSegment64 {
    fn drop(&mut self) {
        panic!("TSS64 being dropped");
    }
}

pub fn init_exceptions() -> (GtdWarpper, Idt) {
    let gdt = GtdWarpper::default();
    gdt.load();

    unsafe {
        write_cs(KERNEL_CS);
        wirte_ss(KERNEL_DS);
        write_es(KERNEL_DS);
        write_ds(KERNEL_DS);
        write_fs(KERNEL_DS);
        write_gs(KERNEL_DS);
    }

    let idt = Idt::new(KERNEL_CS);

    (gdt, idt)
}

pub const BIT_TYPE_DATA: u64 = 0b10u64 << 43;
pub const BIT_TYPE_CODE: u64 = 0b11u64 << 43;

pub const BIT_PRESENT: u64 = 1u64 << 47;
pub const BIT_CS_LONG_MODE: u64 = 1u64 << 53;
pub const BIT_CS_READABLE: u64 = 1u64 << 41;
pub const BIT_CS_WRITABLE: u64 = 1u64 << 41;
pub const BIT_DPL0: u64 = 0u64 << 45;
pub const BIT_DPL3: u64 = 3u64 << 45;

#[repr(u64)]
enum GdtAttr {
    KernelCode = BIT_TYPE_CODE | BIT_PRESENT | BIT_CS_LONG_MODE | BIT_CS_READABLE,
    KernelData = BIT_TYPE_DATA | BIT_PRESENT | BIT_CS_WRITABLE,
}

#[allow(dead_code)]
#[repr(C, packed)]
struct GdtrParameters {
    limit: u16,
    base: *const u64,
}
pub const KERNEL_CS: u16 = 1 << 3;
pub const KERNEL_DS: u16 = 2 << 3;
pub const TSS64_SEL: u16 = 3 << 3;

#[allow(dead_code)]
#[repr(C, packed)]
pub struct Gdt {
    null_segment: GdtSegmentDescriptor,
    kernel_code_segment: GdtSegmentDescriptor,
    kernel_data_segment: GdtSegmentDescriptor,
    task_state_segment: TaskStateSegment64Descriptor,
}

const _: () = assert!(size_of::<Gdt>() == 40);

#[allow(dead_code)]
pub struct GtdWarpper {
    inner: Pin<Box<Gdt>>,
    tss64: TaskStateSegment64,
}

impl GtdWarpper {
    pub fn load(&self) {
        let params = GdtrParameters {
            limit: (size_of::<Gdt>() - 1) as u16,
            base: self.inner.as_ref().get_ref() as *const Gdt as *const u64,
        };

        info!("Loading GDT: {:#018X}", params.base as u64);
        unsafe { asm!("lgdt [rcx]", in("rcx") &params) };
        info!("Loading TSS (selector = {:#X})", TSS64_SEL);

        unsafe {
            asm!(
                "ltr cx",
                in("cx") TSS64_SEL,
            );
        }
    }
}

impl Default for GtdWarpper {
    fn default() -> Self {
        let tss64 = TaskStateSegment64::new();

        let gdt = Gdt {
            null_segment: GdtSegmentDescriptor::null(),
            kernel_code_segment: GdtSegmentDescriptor::new(GdtAttr::KernelCode),
            kernel_data_segment: GdtSegmentDescriptor::new(GdtAttr::KernelData),
            task_state_segment: TaskStateSegment64Descriptor::new(tss64.phys_addr()),
        };

        let gdt = Box::pin(gdt);

        Self { inner: gdt, tss64 }
    }
}

pub struct GdtSegmentDescriptor {
    value: u64,
}

impl GdtSegmentDescriptor {
    fn null() -> Self {
        Self { value: 0 }
    }

    fn new(attr: GdtAttr) -> Self {
        Self { value: attr as u64 }
    }
}
impl fmt::Display for GdtSegmentDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "({:#018X})", self.value)
    }
}

#[repr(C, packed)]
#[allow(dead_code)]
struct TaskStateSegment64Descriptor {
    limit_low: u16,
    base_low: u16,
    base_mid_low: u8,
    attr: u16,
    base_mid_high: u8,
    base_high: u32,
    _reserved: u32,
}

impl TaskStateSegment64Descriptor {
    const fn new(base_addr: u64) -> Self {
        Self {
            limit_low: (size_of::<TaskStateSegment64Inner>() - 1) as u16,
            base_low: (base_addr & 0xffff) as u16,
            base_mid_low: ((base_addr >> 16) & 0xff) as u8,
            attr: 0b1000_0000_1000_1001,
            base_mid_high: ((base_addr >> 24) & 0xff) as u8,
            base_high: ((base_addr >> 32) & 0xffffffff) as u32,
            _reserved: 0,
        }
    }
}

const _: () = assert!(size_of::<TaskStateSegment64Descriptor>() == 16);

pub fn trigger_debug_interrupt() {
    unsafe {
        asm!("int3");
    }
}

pub unsafe fn write_cr3(table: *const PML4) {
    unsafe {
        asm!(
            "mov cr3, rax",
            in("rax") table,
        )
    }
}

// TLB: Translation Lookaside Buffer ページング機構のためのキャッシュメモリ
pub fn flush_tlb() {
    unsafe {
        write_cr3(read_cr3());
    }
}
