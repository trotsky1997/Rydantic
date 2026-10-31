//! The published Context intentionally does not promise unwind safety.
//!
//! ```compile_fail
//! fn needs_unwind_safe<T: std::panic::UnwindSafe>() {}
//! needs_unwind_safe::<rydantic::Context>();
//! ```
//!
//! ```compile_fail
//! fn needs_ref_unwind_safe<T: std::panic::RefUnwindSafe>() {}
//! needs_ref_unwind_safe::<rydantic::Context>();
//! ```
//!
//! Context data must be thread safe and owned; these are API-bound failures.
//!
//! ```compile_fail
//! use rydantic::Model;
//! #[derive(Model)] struct Item { n: u32 }
//! Item::parse_json_with_context(r#"{"n":1}"#, std::sync::Arc::new(std::rc::Rc::new(1)));
//! ```
//!
//! ```compile_fail
//! use rydantic::Model;
//! #[derive(Model)] struct Item { n: u32 }
//! let local = String::from("borrowed");
//! Item::parse_json_with_context(r#"{"n":1}"#, std::sync::Arc::new(local.as_str()));
//! ```
