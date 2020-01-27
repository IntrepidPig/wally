extern crate bindgen;

use std::env;
use std::path::PathBuf;

fn main() {
	println!("cargo:rustc-link-lib=drm_amdgpu");
	println!("cargo:rerun-if-changed=wrapper.h");
	
	let bindings = bindgen::Builder::default()
		.header("wrapper.h")
		.parse_callbacks(Box::new(bindgen::CargoCallbacks))
		.generate()
		.expect("Unable to generate bindings");
	
	let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
	bindings
		.write_to_file("src/ffi.rs")
		.expect("Couldn't write bindings!");
}