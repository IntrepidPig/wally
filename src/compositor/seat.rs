use std::{
	os::unix::io::{AsRawFd, RawFd},
	sync::{Arc, Mutex},
};

use wayland_server::{protocol::*, Filter, Main};
use xkbcommon::xkb;

use crate::{
	backend::{InputBackend, RenderBackend},
	compositor::Compositor,
};

pub struct KeyboardData {
	_xkb: xkb::Context,
	_keymap: xkb::Keymap,
	keymap_string: String,
	fd: RawFd,
	_tmp: std::fs::File,
}

impl KeyboardData {
	pub fn new() -> Self {
		let xkb = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
		let keymap =
			xkb::Keymap::new_from_names(&xkb, "evdev", "pc105", "us", "", None, xkb::KEYMAP_COMPILE_NO_FLAGS).unwrap();
		let keymap_string = keymap.get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1);
		let mut tmp = tempfile::tempfile().unwrap();
		use std::io::Write;
		tmp.write_all(keymap_string.as_bytes()).unwrap();
		tmp.flush().unwrap();
		let fd = tmp.as_raw_fd();
		Self {
			_xkb: xkb,
			_keymap: keymap,
			keymap_string,
			fd,
			_tmp: tmp,
		}
	}
}

impl<I: InputBackend + 'static, R: RenderBackend + 'static> Compositor<I, R> {
	pub fn setup_seat_global(&mut self) {
		let inner = Arc::clone(&self.inner);
		let seat_filter = Filter::new(
			move |(main, version): (Main<wl_seat::WlSeat>, u32), _filter, _dispatch_data| {
				let inner = Arc::clone(&inner);
				let seat = &*main;
				if version >= 2 {
					seat.name(String::from("WallySeat"));
				}
				seat.capabilities(wl_seat::Capability::Pointer | wl_seat::Capability::Keyboard);
				main.quick_assign(move |_main, request: wl_seat::Request, _dispatch_data| {
					let inner = Arc::clone(&inner);
					let mut inner_lock = inner.lock().unwrap();
					match request {
						wl_seat::Request::GetPointer { id } => {
							log::debug!("Got get_pointer request for wl_seat");
							let pointer = (*id).clone();
							let resource = pointer.as_ref().clone();
							inner_lock.client_manager.get_client_info(resource.client().unwrap()).lock().unwrap().pointers.push(pointer);
							id.quick_assign(|_main, request, _dispatch_data| {
								match request {
									wl_pointer::Request::SetCursor { serial, surface, hotspot_x, hotspot_y } => {
										log::debug!("Got pointer request to set cursor: serial {} surface: {:?}, hotspot x {} hotspot y {}", serial, surface.as_ref().map(|s| s.as_ref().id()), hotspot_x, hotspot_y);
									},
									wl_pointer::Request::Release => {
										log::debug!("Got wl_pointer release request");
									},
									_ => {
										log::warn!("Got unknown request for wl_pointer");
									}
								}
							})
						},
						wl_seat::Request::GetKeyboard { id } => {
							log::debug!("Got get_keyboard request for wl_seat");
							let keyboard = (*id).clone();
							let resource = keyboard.as_ref().clone();
							let keyboard_data = KeyboardData::new();
							keyboard.keymap(wl_keyboard::KeymapFormat::XkbV1, keyboard_data.fd, keyboard_data.keymap_string.as_bytes().len() as u32);
							resource.user_data().set(move || Arc::new(Mutex::new(keyboard_data)));
							inner_lock.client_manager.get_client_info(resource.client().unwrap()).lock().unwrap().keyboards.push(keyboard);
							id.quick_assign(|_main, request, _dispatch_data| {
								match request {
									wl_keyboard::Request::Release => {
										log::debug!("Got keyboard release request");
									},
									_ => {
										log::warn!("Got unknown request for wl_keyboard");
									},
								}
							})
						},
						wl_seat::Request::GetTouch { .. } => {
							log::debug!("Got get_touch request for wl_seat");
						},
						wl_seat::Request::Release => {
							log::debug!("Got release request for wl_seat");
						},
						_ => {
							log::warn!("Got unknown request for wl_seat");
						},
					}
				});
			},
		);
		self.display.create_global::<wl_seat::WlSeat, _>(6, seat_filter);
	}
}
