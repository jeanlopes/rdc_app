//! Windows Debug API bridge — zero external dependencies.
//!
//! Uses the Win32 Debug API (`CreateProcess` + `WaitForDebugEvent` loop) and
//! the `pdb` crate for symbol resolution. No LLDB, no Python, no external tools.
//!
//! # Architecture
//! ```text
//! async caller  →  WindowsDebugHandle (mpsc::Sender<DebugCommand>)
//!                          ↓
//!              Windows debug OS thread (std::thread — MUST be same thread as CreateProcess)
//!                          ↓
//!              WindowsDebugBackend (Win32 debug API calls)
//! ```
//!
//! # Usage
//! ```no_run
//! # async fn example() {
//! use win_debug_bridge::thread::WindowsDebugHandle;
//! use runtime_core::session::DebugTarget;
//!
//! let handle = WindowsDebugHandle::spawn().expect("debug init failed");
//! let target = DebugTarget {
//!     executable: r"C:\path\to\my_app.exe".into(),
//!     args: vec![],
//!     env: Default::default(),
//!     working_dir: None,
//! };
//! let (pid, state) = handle.launch_process(target).await.unwrap();
//! # }
//! ```

pub mod pdb_info;
pub mod thread;
pub mod windows_backend;
