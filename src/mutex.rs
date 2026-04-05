use crate::result::Result;
use core::cell::SyncUnsafeCell;
use core::fmt::Debug;
use core::ops::Deref;
use core::ops::DerefMut;
use core::panic;
use core::panic::Location;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering;

// Mutex のロック保持期間を表すガード。
// この値が生きている間だけ、Mutex 内部のデータへ可変アクセスできる。
// location は、ロック取得元を追跡してデバッグしやすくするために保持する。
pub struct MutexGuard<'a, T> {
    mutex: &'a Mutex<T>,
    data: &'a mut T,
    location: Location<'a>,
}

impl<'a, T> MutexGuard<'a, T> {
    #[track_caller]
    unsafe fn new(mutex: &'a Mutex<T>, data: &SyncUnsafeCell<T>) -> Self {
        Self {
            mutex,
            data: unsafe { &mut *data.get() },
            location: *Location::caller(),
        }
    }
}

unsafe impl<'a, T> Sync for MutexGuard<'a, T> {}
impl<'a, T> Deref for MutexGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.data
    }
}

impl<'a, T> DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.data
    }
}

impl<'a, T> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) {
        self.mutex.is_taken.store(false, Ordering::SeqCst);
    }
}

impl<'a, T> core::fmt::Debug for MutexGuard<'a, T>
where
    T: core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MutexGuard")
            .field("data", self.data)
            .field("location", &self.location)
            .finish()
    }
}

// 保護対象のデータと、その排他制御状態を保持する Mutex。
// data にはロック中だけアクセスし、残りのフィールドはロック状態の追跡やデバッグに使う。
pub struct Mutex<T> {
    data: SyncUnsafeCell<T>,       // ロックで保護する内部データ
    is_taken: AtomicBool,          // 現在ロック取得中かどうか
    taker_line_num: AtomicU32,     // ロック取得元の行番号（デバッグ用）
    created_at_file: &'static str, // Mutex を生成したファイル名
    created_at_line: u32,          // Mutex を生成した行番号
}

impl<T: Sized> Debug for Mutex<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Mutex")
            .field("taker_line_num", &self.taker_line_num)
            .field("created_at_file", &self.created_at_file)
            .field("created_at_line", &self.created_at_line)
            .finish()
    }
}

impl<T: Sized> Mutex<T> {
    #[track_caller]
    pub const fn new(data: T) -> Self {
        Self {
            data: SyncUnsafeCell::new(data),
            is_taken: AtomicBool::new(false),
            taker_line_num: AtomicU32::new(0),
            created_at_file: Location::caller().file(),
            created_at_line: Location::caller().line(),
        }
    }

    #[track_caller]
    fn try_lock(&self) -> Result<MutexGuard<T>> {
        if self
            .is_taken
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.taker_line_num
                .store(Location::caller().line(), Ordering::SeqCst);
            Ok(unsafe { MutexGuard::new(self, &self.data) })
        } else {
            Err("Lock failed")
        }
    }

    #[track_caller]
    pub fn lock(&self) -> MutexGuard<T> {
        for _ in 0..10000 {
            if let Ok(locked) = self.try_lock() {
                return locked;
            }
        }
        panic!(
            "Failed to acquire lock after 10000 attempts. Mutex created at {}:{}; last taker line number: {}",
            self.created_at_file,
            self.created_at_line,
            self.taker_line_num.load(Ordering::SeqCst)
        );
    }

    pub fn under_locked<R: Sized>(&self, f: &dyn Fn(&mut T) -> Result<R>) -> Result<R> {
        let mut locked = self.lock();
        f(&mut locked)
    }
}

unsafe impl<T> Sync for Mutex<T> {}
impl<T: Default> Default for Mutex<T> {
    #[track_caller]
    fn default() -> Self {
        Self::new(T::default())
    }
}
