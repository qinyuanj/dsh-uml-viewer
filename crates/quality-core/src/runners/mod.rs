//! Per-language coverage runners; each follows the pinned original's command,
//! artifact lifecycle and failure contract without sharing implementation details.
pub mod lua;
pub mod python;
pub mod typescript;
