use std::sync::Arc;

use wayland_server::{protocol::*, Filter, Main};

use crate::{
	backend::{GraphicsBackend, InputBackend},
	compositor::Compositor,
};

impl<I: InputBackend + 'static, G: GraphicsBackend + 'static> Compositor<I, G> {
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
							let pointer = (*id).clone();
							let resource = pointer.as_ref().clone();
							inner_lock
								.client_manager
								.get_client_info(resource.client().unwrap())
								.lock()
								.unwrap()
								.pointers
								.push(pointer);
							id.quick_assign(|_main, request, _dispatch_data| match request {
								wl_pointer::Request::SetCursor { .. } => {}
								wl_pointer::Request::Release => {}
								_ => {
									log::warn!("Got unknown request for wl_pointer");
								}
							})
						}
						wl_seat::Request::GetKeyboard { id } => {
							let keyboard = (*id).clone();
							let resource = keyboard.as_ref().clone();
							let keyboard_state = Arc::clone(&inner_lock.keyboard_state);
							let keyboard_state_lock = keyboard_state.lock().unwrap();
							keyboard.keymap(
								wl_keyboard::KeymapFormat::XkbV1,
								keyboard_state_lock.fd,
								keyboard_state_lock.keymap_string.as_bytes().len() as u32,
							);
							drop(keyboard_state_lock);
							resource.user_data().set(move || keyboard_state);
							inner_lock
								.client_manager
								.get_client_info(resource.client().unwrap())
								.lock()
								.unwrap()
								.keyboards
								.push(keyboard);
							id.quick_assign(|_main, request, _dispatch_data| {
								match request {
									wl_keyboard::Request::Release => {
										// TODO: probably should remove this keyboard from the client at this point
									}
									_ => {
										log::warn!("Got unknown request for wl_keyboard");
									}
								}
							})
						}
						wl_seat::Request::GetTouch { .. } => {}
						wl_seat::Request::Release => {}
						_ => {
							log::warn!("Got unknown request for wl_seat");
						}
					}
				});
			},
		);
		self.display.create_global::<wl_seat::WlSeat, _>(6, seat_filter);
	}
}
