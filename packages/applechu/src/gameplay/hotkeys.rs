use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

pub struct EdgeHotkey {
    key: AtomicI32,
    down: AtomicBool,
}

impl EdgeHotkey {
    pub const fn new() -> Self {
        Self {
            key: AtomicI32::new(0),
            down: AtomicBool::new(false),
        }
    }

    pub fn configure(&self, key: i32) {
        self.key.store(key, Ordering::Relaxed);
        self.reset();
    }

    pub fn reset(&self) {
        self.down.store(false, Ordering::Relaxed);
    }

    pub fn poll(&self) -> bool {
        let key = self.key.load(Ordering::Relaxed);
        if key <= 0 {
            self.reset();
            return false;
        }
        let is_down = unsafe { GetAsyncKeyState(key) } < 0;
        let was_down = self.down.swap(is_down, Ordering::AcqRel);
        is_down && !was_down
    }
}
