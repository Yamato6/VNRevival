//! The terminal, if there is one.
//!
//! On Windows the executable uses the GUI subsystem, so Windows gives it
//! no console: opened from Explorer, no console window appears next to
//! the game. Started from a terminal, [`attach`] connects the standard
//! output and error to that terminal's console, so `--help`, `detect` and
//! the engine's warnings are still printed there. (The terminal does not
//! wait for a GUI program, so its prompt may come back before the
//! output.)

/// Connects to the console of the process that started this one, if it
/// has one. Returns whether there is somewhere to print to; always `true`
/// outside Windows.
#[cfg(windows)]
pub fn attach() -> bool {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: AttachConsole takes a process id by value and only changes
    // which console this process is attached to; Rust's standard streams
    // look up their handles on every write, so they follow it.
    #[allow(unsafe_code)]
    let attached = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
    attached != 0
}

#[cfg(not(windows))]
pub fn attach() -> bool {
    true
}
