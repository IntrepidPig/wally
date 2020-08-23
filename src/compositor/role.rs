use std::fmt;

use wl_protocols::xdg_shell::*;

use crate::compositor::{prelude::*};

#[derive(Clone)]
pub enum Role {
	XdgSurface(Resource<XdgSurface>),
}

impl Role {
	pub fn destroy(&mut self) {
		match *self {
			Role::XdgSurface(ref _xdg_surface) => {}
		}
	}

	pub fn commit_pending_state(&mut self) {
		match self {
			Role::XdgSurface(ref xdg_surface) => {
				xdg_surface.get_user_data().borrow_mut().commit_pending_state()
			}
		}
	}

	pub fn request_resize(&self, size: Size) {
		match self {
			Role::XdgSurface(ref xdg_surface) => {
				let xdg_surface_data = xdg_surface.get_user_data();
				xdg_surface_data.borrow().request_resize(size);
				let configure_event = xdg_surface::ConfigureEvent {
					serial: 42, // TODO: what should this actually be
				};
				xdg_surface.send_event(XdgSurfaceEvent::Configure(configure_event));
			}
		}
	}

	pub fn set_surface_size(&mut self, _size: Size) {
		match self {
			Role::XdgSurface(ref _xdg_surface) => log::warn!("Set surface size not fully implemented"),
		}
	}

	pub fn get_solid_window_geometry(&self) -> Option<Rect> {
		match self {
			Role::XdgSurface(ref xdg_surface) => {
				let xdg_surface_data = xdg_surface.get_user_data();
				let geometry = xdg_surface_data.borrow().solid_window_geometry;
				geometry
			}
		}
	}
}

impl fmt::Debug for Role {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match *self {
			Role::XdgSurface(ref xdg_surface) => {
				let xdg_surface_data = xdg_surface.get_user_data();
				let res = write!(f, "Role: {:?}", xdg_surface_data.borrow());
				res
			}
		}
	}
}
