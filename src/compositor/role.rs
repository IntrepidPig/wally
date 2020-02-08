use std::{
	sync::{Arc, Mutex},
	fmt,
};

use wayland_protocols::{
	xdg_shell::server::*,
};

use crate::{
	compositor::{
		xdg::{XdgToplevelData},
	},
};

#[derive(Clone)]
pub enum Role {
	XdgToplevel(xdg_toplevel::XdgToplevel),
}

impl Role {
	pub fn destroy(&mut self) {
		match *self {
			Role::XdgToplevel(ref mut xdg_toplevel) => {
			
			},
		}
	}
}

impl fmt::Debug for Role {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match *self {
			Role::XdgToplevel(ref xdg_toplevel) => {
				let xdg_toplevel_data_ref: &Arc<Mutex<XdgToplevelData>> = xdg_toplevel.as_ref().user_data().get::<Arc<Mutex<XdgToplevelData>>>().unwrap();
				let xdg_toplevel_data_lock = xdg_toplevel_data_ref.lock().unwrap();
				fmt::Debug::fmt(&*xdg_toplevel_data_lock, f)
			}
		}
	}
}