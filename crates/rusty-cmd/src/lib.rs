//! The `rusty` command (TICKET-038): `rusty <noun> <verb>` and store scripts, answered
//! with no Qt, so the store skills that call `rusty <script>` work anywhere. The Qt window
//! retired with TICKET-053 and the agent session host with TICKET-056.
//!
//! - [`session`]: the command line and `rusty session start|status`, the back end's
//!   unit.

pub mod session;
