fn main() {
	//generate_wayland_protocol_api()
}

/* extern crate bindgen;

use std::env;
use std::path::PathBuf;

fn generate_amdgpu_bindings() {
	println!("cargo:rustc-link-lib=drm_amdgpu");
	println!("cargo:rerun-if-changed=wrapper.h");

	let bindings = bindgen::Builder::default()
		.header("wrapper.h")
		.parse_callbacks(Box::new(bindgen::CargoCallbacks))
		.generate()
		.expect("Unable to generate bindings");

	let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
	bindings.write_to_file("src/ffi.rs").expect("Couldn't write bindings!");
} */

/* static PROTOCOL: &str = include_str!("wayland.xml");

use std::{
	env,
	fs,
	path,
};

fn generate_wayland_protocol_api() {
	let api = wl_scanner::generate_api(PROTOCOL).expect("Failed to generate Rust API");
	let formatted_api = wl_scanner::format_rustfmt_external(&api).expect("Failed to format Rust API");
	let out_dir = env::var("OUT_DIR").expect("OUT_DIR not specified");
	let mut out_path = path::PathBuf::from(out_dir);
	out_path.push("wayland_api.rs");
	fs::write(&out_path, &formatted_api).expect("Failed to write API to file");
} */