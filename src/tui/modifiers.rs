//! Live modifier keys, for terminals that deliver Shift+Enter as a plain Enter.
//!
//! iTerm and Apple Terminal often send Enter with no Shift bit while the Shift
//! key is held. The composer reads the keyboard directly on macOS.

pub(crate) fn physical_shift_held() -> bool {
    #[cfg(test)]
    if let Some(forced) = FORCED_SHIFT.with(std::cell::Cell::get) {
        return forced;
    }
    live_shift_held()
}

#[cfg(test)]
thread_local! {
    static FORCED_SHIFT: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn force_physical_shift(held: Option<bool>) {
    FORCED_SHIFT.with(|cell| cell.set(held));
}

#[cfg(target_os = "macos")]
fn live_shift_held() -> bool {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceFlagsState(state_id: i32) -> u64;
    }
    // HID system state, and the Shift bit from CGEventFlags.
    const HID_SYSTEM_STATE: i32 = 1;
    const SHIFT: u64 = 0x0002_0000;
    unsafe { CGEventSourceFlagsState(HID_SYSTEM_STATE) & SHIFT != 0 }
}

#[cfg(not(target_os = "macos"))]
fn live_shift_held() -> bool {
    false
}
