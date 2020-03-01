use std::{
	fmt,
	sync::{Arc, Mutex},
};

use wayland_protocols::xdg_shell::server::*;

use crate::compositor::{xdg::XdgToplevelData, PointerState};

#[derive(Clone)]
pub enum Role {
	XdgToplevel(xdg_toplevel::XdgToplevel),
	Cursor(Arc<Mutex<PointerState>>),
}

impl Role {
	pub fn destroy(&mut self) {
		match *self {
			Role::XdgToplevel(ref _xdg_toplevel) => {}
			Role::Cursor(ref _pointer_state) => {}
		}
	}

	pub fn get_geometry(&self) -> Option<(i32, i32, u32, u32)> {
		match self {
			Role::XdgToplevel(ref xdg_toplevel) => {
				let xdg_toplevel_data = xdg_toplevel
					.as_ref()
					.user_data()
					.get::<Arc<Mutex<XdgToplevelData>>>()
					.unwrap();
				let xdg_toplevel_data_lock = xdg_toplevel_data.lock().unwrap();
				Some((
					xdg_toplevel_data_lock.pos.0,
					xdg_toplevel_data_lock.pos.1,
					xdg_toplevel_data_lock.size.0,
					xdg_toplevel_data_lock.size.1,
				))
			}
			Role::Cursor(ref _pointer_state) => {
				log::warn!("Tried to get geometry of cursor");
				None
			}
		}
	}
}

impl fmt::Debug for Role {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match *self {
			Role::XdgToplevel(ref xdg_toplevel) => {
				let xdg_toplevel_data_ref: &Arc<Mutex<XdgToplevelData>> = xdg_toplevel
					.as_ref()
					.user_data()
					.get::<Arc<Mutex<XdgToplevelData>>>()
					.unwrap();
				let xdg_toplevel_data_lock = xdg_toplevel_data_ref.lock().unwrap();
				fmt::Debug::fmt(&*xdg_toplevel_data_lock, f)
			}
			Role::Cursor(ref pointer_state) => {
				let pointer_state = pointer_state.lock().unwrap();
				fmt::Debug::fmt(&*pointer_state, f)
			}
		}
	}
}
