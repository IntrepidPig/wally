pub use std::os::unix::io::{AsRawFd, RawFd};

use calloop::channel::Channel;
use wayland_server::protocol::*;

use crate::compositor::SurfaceTree;

//pub mod drm;
pub mod headless;
pub mod libinput;
pub mod vulkan;
pub mod winit;

pub trait Backend {
	type Error;

	fn update_input_backend(&mut self) -> Result<(), Self::Error>;

	fn get_event_source(&mut self) -> Channel<BackendEvent>;

	type ShmPool;
	type SurfaceData: Send + 'static;

	fn update_render_backend(&mut self) -> Result<(), Self::Error>;

	fn create_surface(&mut self, surface: wl_surface::WlSurface) -> Result<Self::SurfaceData, Self::Error>;

	fn destroy_surface(&mut self, surface: wl_surface::WlSurface) -> Result<(), Self::Error>;

	fn render_tree(&mut self, tree: &SurfaceTree) -> Result<(), Self::Error>;
}

pub trait InputBackend {
	type Error;

	fn update(&mut self) -> Result<(), Self::Error>;

	fn get_event_source(&mut self) -> Channel<BackendEvent>;
}

pub trait RenderBackend {
	type Error;
	type ShmPool;
	type SurfaceData: Send + 'static;

	fn update(&mut self) -> Result<(), Self::Error>;

	fn create_surface(&mut self, surface: wl_surface::WlSurface) -> Result<Self::SurfaceData, Self::Error>;

	fn destroy_surface(&mut self, surface: wl_surface::WlSurface) -> Result<(), Self::Error>;

	fn render_tree(&mut self, tree: &SurfaceTree) -> Result<(), Self::Error>;
}

pub struct MergedBackend<I: InputBackend, R: RenderBackend> {
	input_backend: I,
	render_backend: R,
}

pub enum MergedBackendError<I: InputBackend, R: RenderBackend> {
	InputBackendError(I::Error),
	RenderBackendError(R::Error),
}

impl<I: InputBackend, R: RenderBackend> Backend for MergedBackend<I, R> {
	type Error = MergedBackendError<I, R>;

	fn update_input_backend(&mut self) -> Result<(), Self::Error> {
		self.input_backend
			.update()
			.map_err(MergedBackendError::InputBackendError)
	}

	fn get_event_source(&mut self) -> Channel<BackendEvent> {
		self.input_backend.get_event_source()
	}

	type ShmPool = R::ShmPool;
	type SurfaceData = R::SurfaceData;

	fn update_render_backend(&mut self) -> Result<(), Self::Error> {
		self.render_backend
			.update()
			.map_err(MergedBackendError::RenderBackendError)
	}

	fn create_surface(&mut self, surface: wl_surface::WlSurface) -> Result<Self::SurfaceData, Self::Error> {
		self.render_backend
			.create_surface(surface)
			.map_err(MergedBackendError::RenderBackendError)
	}

	fn destroy_surface(&mut self, surface: wl_surface::WlSurface) -> Result<(), Self::Error> {
		self.render_backend
			.destroy_surface(surface)
			.map_err(MergedBackendError::RenderBackendError)
	}

	fn render_tree(&mut self, tree: &SurfaceTree) -> Result<(), Self::Error> {
		self.render_backend
			.render_tree(tree)
			.map_err(MergedBackendError::RenderBackendError)
	}
}

pub fn create_backend<I: InputBackend, R: RenderBackend>(input_backend: I, render_backend: R) -> MergedBackend<I, R> {
	MergedBackend {
		input_backend,
		render_backend,
	}
}

#[derive(Debug, Clone, PartialEq)]
pub enum BackendEvent {
	KeyPress(KeyPress),
	StopRequested,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KeyPress {
	pub serial: u32,
	pub time: u32,
	pub key: u32,
	pub state: wl_keyboard::KeyState,
}
