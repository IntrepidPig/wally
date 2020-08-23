use std::{
	io::Write,
	os::unix::io::{AsRawFd, RawFd},
	fmt,
};

use xkbcommon::xkb;

use wl_server::{
	protocol::*,
};

use crate::compositor::prelude::*;

pub struct KeyboardState {
	pub xkb: xkb::Context,
	pub keymap: xkb::Keymap,
	pub state: xkb::State,
	pub keymap_string: String,
	pub fd: RawFd,
	pub tmp: std::fs::File,
	pub xkb_modifiers_state: XkbModifiersState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XkbModifiersState {
	pub mods_depressed: u32,
	pub mods_latched: u32,
	pub mods_locked: u32,
	pub group: u32,
}

impl KeyboardState {
	pub fn new() -> Self {
		let xkb = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
		let keymap =
			xkb::Keymap::new_from_names(&xkb, "evdev", "pc105", "us", "", None, xkb::KEYMAP_COMPILE_NO_FLAGS).unwrap();
		let state = xkb::State::new(&keymap);
		let keymap_string = keymap.get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1);
		let mut tmp = tempfile::tempfile().unwrap();
		tmp.write_all(keymap_string.as_bytes()).unwrap();
		tmp.flush().unwrap();
		let fd = tmp.as_raw_fd();
		Self {
			xkb: xkb,
			keymap: keymap,
			state,
			keymap_string,
			fd,
			tmp: tmp,
			xkb_modifiers_state: XkbModifiersState {
				mods_depressed: 0,
				mods_latched: 0,
				mods_locked: 0,
				group: 0,
			},
		}
	}

	pub fn update_key(&mut self, key_press: KeyPress) -> bool {
		self.state.update_key(key_press.key + 8, key_press.state.into());
		let new_modifiers = self.get_modifier_state();
		if new_modifiers != self.xkb_modifiers_state {
			self.xkb_modifiers_state = new_modifiers;
			true
		} else {
			false
		}
	}

	fn get_modifier_state(&mut self) -> XkbModifiersState {
		let mods_depressed = self.state.serialize_mods(xkb::STATE_MODS_DEPRESSED);
		let mods_latched = self.state.serialize_mods(xkb::STATE_MODS_LATCHED);
		let mods_locked = self.state.serialize_mods(xkb::STATE_MODS_LOCKED);
		let group = self.state.serialize_layout(xkb::STATE_LAYOUT_EFFECTIVE);
		XkbModifiersState {
			mods_depressed,
			mods_latched,
			mods_locked,
			group,
		}
	}
}

impl fmt::Debug for KeyboardState {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct("KeyboardState")
			.field("xkb", &"<xkb context>")
			.field("keymap", &"<xkb kemap>")
			.field("state", &"<xkb state>")
			.field("keymap_string", &self.keymap_string)
			.field("fd", &self.fd)
			.field("tmp", &self.tmp)
			.field("xkb_modifiers_state", &self.xkb_modifiers_state)
			.finish()
	}
}

impl<I: InputBackend, G: GraphicsBackend> CompositorState<I, G> {
	pub fn handle_input_event(&mut self, event: BackendEvent) {
		match event {
			BackendEvent::StopRequested => self.handle_stop_request(),
			BackendEvent::KeyPress(key_press) => self.handle_key_press(key_press),
			BackendEvent::PointerMotion(pointer_motion) => self.handle_pointer_motion(pointer_motion),
			BackendEvent::PointerButton(pointer_button) => self.handle_pointer_button(pointer_button),
		}
	}

	pub fn handle_stop_request(&mut self) {
		self.inner.running = false;
	}

	pub fn handle_key_press(&mut self, key_press: KeyPress) {
		let state_change = self.inner.keyboard_state.update_key(key_press.clone());

		// Send the key event to the surface that currently has keyboard focus, and an updated modifiers event if modifiers changed.
		if let Some(focused) = self.inner.keyboard_focus.clone() {
			// TODO: check aliveness
			let client = focused.client();
			let client = client.get().unwrap();
			let client_state = client.state::<RefCell<ClientState>>();
			let client_state = client_state.borrow();

			for keyboard in &client_state.keyboards {
				if state_change {
					let mods = self.inner.keyboard_state.xkb_modifiers_state;
					let modifiers_event = wl_keyboard::ModifiersEvent {
						serial: key_press.serial,
						mods_depressed: mods.mods_depressed,
						mods_latched: mods.mods_latched,
						mods_locked: mods.mods_locked,
						group: mods.group,
					};
					keyboard.send_event(WlKeyboardEvent::Modifiers(modifiers_event));
				}
				let key_event = wl_keyboard::KeyEvent {
					serial: key_press.serial,
					time: key_press.time,
					key: key_press.key,
					state: key_press.state.into(),
				};
				keyboard.send_event(WlKeyboardEvent::Key(key_event));
			}
		}
	}

	pub fn handle_pointer_motion(&mut self, pointer_motion: PointerMotion) {
		let mut pointer_state = &mut self.inner.pointer;

		pointer_state.pos.0 += pointer_motion.dx_unaccelerated * pointer_state.sensitivity;
		pointer_state.pos.1 += pointer_motion.dy_unaccelerated * pointer_state.sensitivity;
		let pointer_pos = Point::new(pointer_state.pos.0.round() as i32, pointer_state.pos.1.round() as i32);

		if let Some(node) = self.inner.window_manager.get_window_under_point(pointer_pos) {
			let surface = node.surface.borrow().clone();
			let client = surface.client();
			let client = client.get().unwrap();
			let client_state = client.state::<RefCell<ClientState>>();

			let surface_relative_coords =
				if let Some(geometry) = node.geometry() {
					Point::new(pointer_pos.x - geometry.x, pointer_pos.y - geometry.y)
				} else {
					// This should probably not happen because the window manager just told us the pointer is
					// over this window, implying it has geometry
					Point::new(0, 0)
				};

			if let Some(old_pointer_focus) = self.inner.pointer_focus.clone() {
				let old_surface_client = old_pointer_focus.client();
				let old_surface_client = old_surface_client.get().unwrap();
				let old_surface_client_state = old_surface_client.state::<RefCell<ClientState>>();

				if old_pointer_focus.is(&surface) {
					// The pointer is over the same surface as it was previously, do not send any focus events
				} else {
					// The pointer is over a different surface, unfocus the old one and focus the new one
					for pointer in &old_surface_client_state.borrow().pointers {
						pointer.send_event(WlPointerEvent::Leave(wl_pointer::LeaveEvent {
							serial: get_input_serial(),
							surface: old_pointer_focus.clone(),
						}));
					}
					for pointer in &client_state.borrow().pointers {
						pointer.send_event(WlPointerEvent::Enter(wl_pointer::EnterEvent {
							serial: get_input_serial(),
							surface: surface.clone(),
							surface_x: (surface_relative_coords.x as f64).into(),
							surface_y: (surface_relative_coords.y as f64).into(),
						}));
					}
					self.inner.pointer_focus = Some(surface.clone())
				}
			} else {
				// The pointer has entered a surface while no other surface is focused, focus this surface
				for pointer in &client_state.borrow().pointers {
					pointer.send_event(WlPointerEvent::Enter(wl_pointer::EnterEvent {
						serial: get_input_serial(),
						surface: surface.clone(),
						surface_x: (surface_relative_coords.x as f64).into(),
						surface_y: (surface_relative_coords.y as f64).into(),
					}));
				}
				self.inner.pointer_focus = Some(surface.clone());
			}

			// Send the surface the actual motion event
			for pointer in &client_state.borrow().pointers {
				pointer.send_event(WlPointerEvent::Motion(wl_pointer::MotionEvent {
					time: get_input_serial(),
					surface_x: (surface_relative_coords.x as f64).into(),
					surface_y: (surface_relative_coords.y as f64).into(),
				}));
			}
		} else {
			// The pointer is not over any surface, remove pointer focus from the previous focused surface if any
			if let Some(old_pointer_focus) = self.inner.pointer_focus.take() {
				let client = old_pointer_focus.client();
				let client = client.get().unwrap();
				let client_state = client.state::<RefCell<ClientState>>();

				for pointer in &client_state.borrow().pointers {
					pointer.send_event(WlPointerEvent::Leave(wl_pointer::LeaveEvent {
						serial: get_input_serial(),
						surface: old_pointer_focus.clone(),
					}));
				}
			}
		}
	}

	pub fn handle_pointer_button(&mut self, pointer_button: PointerButton) {
		let pointer_state = &mut self.inner.pointer;
		let pointer_pos = Point::new(pointer_state.pos.0.round() as i32, pointer_state.pos.1.round() as i32);

		if let Some(node) = self.inner.window_manager.get_window_under_point(pointer_pos) {
			let surface = node.surface.borrow().clone();
			let client = surface.client();
			let client = client.get().unwrap();
			let client_state = client.state::<RefCell<ClientState>>();

			if pointer_button.state == PressState::Press {
				if let Some(old_keyboard_focus) = self.inner.keyboard_focus.clone() {
					if old_keyboard_focus.is(&surface) {
						// No focus change, this is the same surface
					} else {
						// Change the keyboard focus
						let old_surface_client = old_keyboard_focus.client();
						let old_surface_client = old_surface_client.get().unwrap();
						let old_surface_client_state = old_surface_client.state::<RefCell<ClientState>>();

						for keyboard in &old_surface_client_state.borrow().keyboards {
							keyboard.send_event(WlKeyboardEvent::Leave(wl_keyboard::LeaveEvent {
								serial: get_input_serial(),
								surface: old_keyboard_focus.clone(),
							}));
						}
						for keyboard in &client_state.borrow().keyboards {
							let mods = self.inner.keyboard_state.xkb_modifiers_state;
							let modifiers_event = wl_keyboard::ModifiersEvent {
								serial: get_input_serial(),
								mods_depressed: mods.mods_depressed,
								mods_latched: mods.mods_latched,
								mods_locked: mods.mods_locked,
								group: mods.group,
							};
							let enter_event = wl_keyboard::EnterEvent {
								serial: get_input_serial(),
								surface: surface.clone(),
								keys: Vec::new(), // TODO: actual value
							};
							keyboard.send_event(WlKeyboardEvent::Modifiers(modifiers_event));
							keyboard.send_event(WlKeyboardEvent::Enter(enter_event));
						}
						self.inner.keyboard_focus = Some(surface.clone());
					}
				} else {
					// Focus the keyboard on a window when there was no previously focused window
					for keyboard in &client_state.borrow().keyboards {
						let mods = self.inner.keyboard_state.xkb_modifiers_state;
						let modifiers_event = wl_keyboard::ModifiersEvent {
							serial: get_input_serial(),
							mods_depressed: mods.mods_depressed,
							mods_latched: mods.mods_latched,
							mods_locked: mods.mods_locked,
							group: mods.group,
						};
						let enter_event = wl_keyboard::EnterEvent {
							serial: get_input_serial(),
							surface: surface.clone(),
							keys: Vec::new(), // TODO: actual value
						};
						keyboard.send_event(WlKeyboardEvent::Modifiers(modifiers_event));
						keyboard.send_event(WlKeyboardEvent::Enter(enter_event));
					}
					self.inner.keyboard_focus = Some(surface.clone());
				}
			}
		} else {
			// Remove the keyboard focus from the current focus if empty space is clicked
			if let Some(old_keyboard_focus) = self.inner.keyboard_focus.take() {
				let old_surface_client = old_keyboard_focus.client();
				let old_surface_client = old_surface_client.get().unwrap();
				let old_surface_client_state = old_surface_client.state::<RefCell<ClientState>>();

				for keyboard in &old_surface_client_state.borrow().keyboards {
					keyboard.send_event(WlKeyboardEvent::Leave(wl_keyboard::LeaveEvent {
						serial: get_input_serial(),
						surface: old_keyboard_focus.clone(),
					}));
				}
			}
		}

		// Send event to focused window
		if let Some(focused) = self.inner.keyboard_focus.clone() {
			let client = focused.client();
			let client = client.get().unwrap();
			let client_state = client.state::<RefCell<ClientState>>();

			for pointer in &client_state.borrow().pointers {
				pointer.send_event(WlPointerEvent::Button(wl_pointer::ButtonEvent {
					serial: pointer_button.serial,
					time: pointer_button.time,
					button: pointer_button.button.to_wl(),
					state: pointer_button.state.into(),
				}));
			}
		}
	}
}