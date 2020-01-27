pub use std::os::unix::io::{RawFd, AsRawFd};
use crate::compositor::Compositor;
use crate::backend::MergedBackendError::InputBackendError;

pub mod drm;
pub mod headless;
pub mod libinput;
pub mod window;

struct InputDevice;
struct OutputDevice;

pub trait Backend {
	type Error;
	
	fn update_input_backend(&mut self) -> Result<(), Self::Error>;
	
	fn poll_for_events(&mut self) -> Option<BackendEvent>;
	
	type ShmPool;
	
	fn update_render_backend(&mut self) -> Result<(), Self::Error>;
	
	fn create_shm_pool(&mut self, fd: RawFd, size: usize) -> Result<Self::ShmPool, Self::Error>;
}

pub trait InputBackend {
	type Error;
	
	fn update(&mut self) -> Result<(), Self::Error>;
	
	fn poll_for_events(&mut self) -> Option<BackendEvent>;
}

pub trait RenderBackend {
	type Error;
	
	type ShmPool;
	
	fn update(&mut self) -> Result<(), Self::Error>;
	
	fn create_shm_pool(&mut self, fd: RawFd, size: usize) -> Result<Self::ShmPool, Self::Error>;
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
		self.input_backend.update().map_err(MergedBackendError::InputBackendError)
	}
	
	fn poll_for_events(&mut self) -> Option<BackendEvent> {
		self.input_backend.poll_for_events()
	}
	
	type ShmPool = R::ShmPool;
	
	fn update_render_backend(&mut self) -> Result<(), Self::Error> {
		self.render_backend.update().map_err(MergedBackendError::RenderBackendError)
	}
	
	fn create_shm_pool(&mut self, fd: RawFd, size: usize) -> Result<Self::ShmPool, Self::Error>  {
		self.render_backend.create_shm_pool(fd, size).map_err(MergedBackendError::RenderBackendError)
	}
}

pub fn create_backend<I: InputBackend, R: RenderBackend>(input_backend: I, render_backend: R) -> MergedBackend<I, R> {
	MergedBackend {
		input_backend,
		render_backend,
	}
}

#[derive(Debug, Clone)]
pub enum BackendEvent {
	KeyPress,
}