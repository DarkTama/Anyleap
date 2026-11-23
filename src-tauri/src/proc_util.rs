//! Shared helpers for spawning child processes.

/// Win32 `CREATE_NO_WINDOW`: stops a console window flashing for child processes.
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Make a synchronous child spawn window-less on Windows (no-op elsewhere).
pub fn no_window(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
