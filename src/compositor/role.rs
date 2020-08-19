use std::fmt;

use wayland_protocols::xdg_shell::server::*;

use crate::compositor::{prelude::*, xdg::XdgSurfaceData};

#[derive(Clone)]
pub enum Role {
	XdgSurface(xdg_surface::XdgSurface),
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
				let xdg_surface_data = xdg_surface.get_synced::<XdgSurfaceData>();
				let mut xdg_surface_data_lock = xdg_surface_data.lock().unwrap();
				xdg_surface_data_lock.commit_pending_state();
			}
		}
	}

	pub fn resize_window(&mut self, size: Size) {
		match self {
			Role::XdgSurface(ref xdg_surface) => {
				let xdg_surface_data = xdg_surface.get_synced::<XdgSurfaceData>();
				let mut xdg_surface_data_lock = xdg_surface_data.lock().unwrap();
				xdg_surface_data_lock.resize_window(size);
				xdg_surface.configure(42);
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
				let xdg_surface_data = xdg_surface.get_synced::<XdgSurfaceData>();
				let xdg_surface_data_lock = xdg_surface_data.lock().unwrap();
				xdg_surface_data_lock.solid_window_geometry
			}
		}
	}
}

impl fmt::Debug for Role {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match *self {
			Role::XdgSurface(ref xdg_surface) => {
				let xdg_surface_data = xdg_surface.get_synced::<XdgSurfaceData>();
				let xdg_surface_data_lock = xdg_surface_data.lock().unwrap();
				fmt::Debug::fmt(&*xdg_surface_data_lock, f)
			}
		}
	}
}
