#[cfg(windows)]
pub fn borderless(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    use windows_sys::Win32::Graphics::Dwm::{
        DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DwmSetWindowAttribute,
    };
    // Activation and frame changes can reset the compositor's border treatment.
    let no_border: u32 = 0xfffffffe;
    let rounded: u32 = 2;
    unsafe {
        DwmSetWindowAttribute(
            hwnd as _,
            DWMWA_BORDER_COLOR as _,
            (&no_border as *const u32).cast(),
            std::mem::size_of::<u32>() as _,
        );
        DwmSetWindowAttribute(
            hwnd as _,
            DWMWA_WINDOW_CORNER_PREFERENCE as _,
            (&rounded as *const u32).cast(),
            std::mem::size_of::<u32>() as _,
        );
    }
}
