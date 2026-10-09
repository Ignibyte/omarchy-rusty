//! The `rusty` command: `rusty <noun> <verb>` and store scripts, with no dependencies, so
//! the store skills that call `rusty <script>` work anywhere.
//!
//! - [`session`]: the command line and `rusty session start|status`, for the back end's
//!   systemd user service.

pub mod session;
