use std::env;

fn main() {
    cbindgen::generate(env::var("CARGO_MANIFEST_DIR").unwrap())
        .unwrap()
        .write_to_file("target/libdpds_core.h");
}
