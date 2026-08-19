//! One module per family of subcommand; one function per subcommand.
//!
//! They share nothing but their signature: each takes what it needs and
//! returns an `Exit`. The match that names them is `crate::run`, and it stays
//! there so a command added without a dispatch arm fails to build.

pub(crate) mod agent;
pub(crate) mod check;
pub(crate) mod hook;
pub(crate) mod query;
pub(crate) mod write;
