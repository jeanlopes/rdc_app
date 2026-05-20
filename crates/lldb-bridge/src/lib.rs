//! LLDB bridge crate — wraps the LLDB Python API via PyO3 and exposes an
//! async-safe handle over a dedicated OS thread.
//!
//! # Architecture
//! ```text
//! async caller  →  LLDBHandle (mpsc::Sender<LLDBCommand>)
//!                      ↓
//!              LLDB OS thread (std::thread)
//!                      ↓
//!              PythonBackend (PyO3 + lldb Python module)
//! ```
//!
//! # Usage
//! ```no_run
//! use lldb_bridge::thread::LLDBHandle;
//! use runtime_core::session::DebugTarget;
//!
//! # tokio_test::block_on(async {
//! let handle = LLDBHandle::spawn().expect("LLDB init failed");
//! let target = DebugTarget {
//!     executable: "/path/to/binary".into(),
//!     args: vec![],
//!     env: Default::default(),
//!     working_dir: None,
//! };
//! let (pid, state) = handle.launch_process(target).await.unwrap();
//! # });
//! ```

pub mod thread;
pub mod python_backend;
