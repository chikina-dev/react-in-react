#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(clippy::unreadable_literal)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::upper_case_acronyms)]
#![allow(clippy::uninlined_format_args)]
#![allow(unsafe_op_in_unsafe_fn)]
#![no_std]

use ::core::ptr;

pub const SIZE_T_ERROR: &str =
    "conversion between C type 'size_t' and Rust type 'usize' overflowed.";

include!(concat!(env!("OUT_DIR"), "/bindings_target.rs"));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/rquickjs-sys/src/bindings/",
    bindings_env!("TARGET"),
    ".rs"
));

#[cfg(target_pointer_width = "64")]
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/rquickjs-sys/src/inlines/ptr_64.rs"
));

#[cfg(target_pointer_width = "32")]
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/rquickjs-sys/src/inlines/ptr_32_nan_boxing.rs"
));

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/rquickjs-sys/src/inlines/common.rs"
));
