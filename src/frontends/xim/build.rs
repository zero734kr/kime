use std::env;
use std::path::Path;

use bindgen::{
    callbacks::{IntKind, ParseCallbacks},
    CargoCallbacks,
};

#[derive(Debug)]
struct Callbacks;

impl ParseCallbacks for Callbacks {
    fn int_macro(&self, name: &str, _value: i64) -> Option<IntKind> {
        if name.starts_with("XCB_XIM") && !name.contains("SIZE") {
            Some(IntKind::U8)
        } else {
            None
        }
    }

    fn include_file(&self, filename: &str) {
        CargoCallbacks.include_file(filename)
    }
}

fn main() {
    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .default_macro_constant_type(bindgen::MacroTypeVariation::Unsigned)
        .whitelist_var("XCB_.+")
        .whitelist_function("xcb_(im|compound|utf).+")
        .whitelist_type("xcb_x?im.+")
        .blacklist_function(".+fr_(read|write|size|free)$")
        .generate_block(true)
        .prepend_enum_name(false)
        .parse_callbacks(Box::new(Callbacks))
        .generate()
        .unwrap();

    bindings
        .write_to_file(Path::new(&env::var("OUT_DIR").unwrap()).join("bindings.rs"))
        .unwrap();
}
