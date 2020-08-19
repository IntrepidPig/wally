use std::{
	io::Write,
	os::unix::io::{AsRawFd, RawFd},
};

use xkbcommon::xkb;

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
