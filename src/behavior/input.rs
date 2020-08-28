use std::{
	io::Write,
	os::unix::io::{AsRawFd, RawFd},
	fmt,
};

use xkbcommon::xkb;

use wl_server::{
	protocol::*,
};

use crate::{backend::Button, compositor::{seat::{KeyboardData}, prelude::*}};

// TODO this probably should be both per-seat and handled by the input backend rather than in
// the compositor state.
pub struct KeyboardState {
	pub xkb: xkb::Context,
	pub keymap: xkb::Keymap,
	pub state: xkb::State,
	pub keymap_string: String,
	pub fd: RawFd,
	pub tmp: std::fs::File,
	pub xkb_modifiers_state: XkbModifiersState,
	pub mods_state_change: bool,
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
			mods_state_change: false,
		}
	}

	pub fn update_key(&mut self, key_press: KeyPress) -> bool {
		self.state.update_key(key_press.key + 8, key_press.state.into());
		let new_modifiers = self.get_modifier_state();
		if new_modifiers != self.xkb_modifiers_state {
			self.xkb_modifiers_state = new_modifiers;
			self.mods_state_change = true;
			true
		} else {
			self.mods_state_change = false;
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

	pub fn send_current_keyboard_modifiers(&self, keyboard: Resource<WlKeyboard, KeyboardData>, serial: Serial) {
		let mods = self.xkb_modifiers_state;
		keyboard.send_event(WlKeyboardEvent::Modifiers(wl_keyboard::ModifiersEvent {
			serial: serial.into(),
			mods_depressed: mods.mods_depressed,
			mods_latched: mods.mods_latched,
			mods_locked: mods.mods_locked,
			group: mods.group,
		}));
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
		self.inner.keyboard_state.update_key(key_press.clone());

		// Send the key event to the surface that currently has keyboard focus, and an updated modifiers event if modifiers changed.
		if let Some(focused) = self.inner.keyboard_focus.clone() {
			// TODO: check aliveness
			let client = focused.client();
			let client = client.get().unwrap();
			let client_state = client.state::<RefCell<ClientState<G>>>();
			let client_state = client_state.borrow();

			if let Some(ref seat) = client_state.seat {
				self.send_keyboard_key_event(seat.clone(), key_press);
			}
		}
	}

	pub fn handle_pointer_motion(&mut self, pointer_motion: PointerMotion) {
		let mut pointer_state = &mut self.inner.pointer;

		pointer_state.pos.0 += pointer_motion.dx_unaccelerated * pointer_state.sensitivity;
		pointer_state.pos.1 += pointer_motion.dy_unaccelerated * pointer_state.sensitivity;
		let pointer_pos = Point::new(pointer_state.pos.0.round() as i32, pointer_state.pos.1.round() as i32);

		if let Some(ref moving_surface) = self.inner.moving_surface {
			if let Some(node) = self.inner.window_manager.tree.find_surface(moving_surface) {
				let mut position = node.position.get();
				position.x = (position.x as f64 + pointer_motion.dx).round() as i32;
				position.y = (position.y as f64 + pointer_motion.dy).round() as i32;
				node.position.set(position);
			}
		}

		// Tdo this should check the surface input region, not the surface window geometry
		if let Some(node) = self.inner.window_manager.get_window_under_point(pointer_pos) {
			let surface = node.surface.borrow().clone();

			let surface_relative_coords =
				if let Some(geometry) = node.node_surface_geometry() {
					let node_surface_relative_coords = Point::new(pointer_pos.x - geometry.x, pointer_pos.y - geometry.y);
					// This unwrap is fine because Node::node_surface_geometry just returned Some, so this will definitely return Some
					let surface_relative_coords = node.node_surface_point_to_surface_point(node_surface_relative_coords).unwrap();
					surface_relative_coords
				} else {
					// This should probably not happen because the window manager just told us the pointer is
					// over this window, implying it has geometry
					Point::new(0, 0)
				};

			if let Some(old_pointer_focus) = self.inner.pointer_focus.clone() {
				if !old_pointer_focus.is(&surface) {
					// The pointer is over a different surface, unfocus the old one and focus the new one
					self.unfocus_surface_pointer(old_pointer_focus.clone());
					self.focus_surface_pointer(surface.clone(), surface_relative_coords);
				}
			} else {
				// The pointer has entered a surface while no other surface is focused, focus this surface
				self.focus_surface_pointer(surface.clone(), surface_relative_coords);
			}

			// Send the surface the actual motion event
			let client = surface.client();
			let client = client.get().unwrap();
			let client_state = client.state::<RefCell<ClientState<G>>>();
			let client_state = client_state.borrow();
			if let Some(ref seat) = client_state.seat {
				self.send_pointer_motion_event(seat.clone(), surface_relative_coords);
			}
		} else {
			// The pointer is not over any surface, remove pointer focus from the previous focused surface if any
			if let Some(old_pointer_focus) = self.inner.pointer_focus.take() {
				self.unfocus_surface_pointer(old_pointer_focus);
			}
		}
	}

	pub fn handle_pointer_button(&mut self, pointer_button: PointerButton) {
		let pointer_state = &mut self.inner.pointer;
		let pointer_pos = Point::new(pointer_state.pos.0.round() as i32, pointer_state.pos.1.round() as i32);

		if let Some(node) = self.inner.window_manager.get_window_under_point(pointer_pos) {
			let surface = node.surface.borrow().clone();
			if let Some(old_keyboard_focus) = self.inner.keyboard_focus.clone() {
				if !old_keyboard_focus.is(&surface) {
					self.unfocus_surface_keyboard(old_keyboard_focus.clone());
					self.focus_surface_keyboard(surface.clone());
					self.unset_surface_active(old_keyboard_focus.clone());
					self.set_surface_active(surface.clone());
					self.inner.window_manager.raise(surface);
				}
			} else {
				self.focus_surface_keyboard(surface.clone());
				self.set_surface_active(surface.clone());
				self.inner.window_manager.raise(surface);
			}
		} else {
			// Remove the keyboard focus from the current focus if empty space is clicked
			if let Some(old_keyboard_focus) = self.inner.keyboard_focus.take() {
				self.unfocus_surface_keyboard(old_keyboard_focus.clone());
				self.unset_surface_active(old_keyboard_focus);
			}
		}

		if pointer_button.button == Button::Left && pointer_button.state == PressState::Release {
			self.inner.moving_surface = None;
		}

		// Send event to the pointer of the seat of the focused window
		if let Some(focused) = self.inner.pointer_focus.clone() {
			let client = focused.client();
			let client = client.get().unwrap();
			let client_state = client.state::<RefCell<ClientState<G>>>();
			let client_state = client_state.borrow();
			if let Some(ref seat) = client_state.seat {
				self.send_pointer_button_event(seat.clone(), pointer_button);
			}
		}
	}
}