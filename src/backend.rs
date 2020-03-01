pub use std::os::unix::io::{AsRawFd, RawFd};

use std::fmt;

use calloop::channel::Channel;
use wayland_server::protocol::*;

use crate::compositor::surface::SurfaceTree;

pub mod libinput;
pub mod vulkan;
pub mod winit;

pub trait InputBackend {
	type Error: fmt::Debug + fmt::Display;

	fn update(&mut self) -> Result<(), Self::Error>;

	fn get_event_source(&mut self) -> Channel<BackendEvent>;
}

pub trait RenderBackend {
	type Error: fmt::Debug + fmt::Display;
	type ShmPool;
	type ObjectHandle: Send + fmt::Debug + 'static;

	fn update(&mut self) -> Result<(), Self::Error>;

	fn create_object(&mut self) -> Result<Self::ObjectHandle, Self::Error>;

	fn destroy_object(&mut self, object_handle: Self::ObjectHandle) -> Result<(), Self::Error>;

	fn render_tree(&mut self, tree: &SurfaceTree<Self>) -> Result<(), Self::Error>;

	fn get_size(&self) -> (u32, u32);
}

pub struct MergedBackend<I: InputBackend, R: RenderBackend> {
	pub input_backend: I,
	pub render_backend: R,
}

pub(crate) fn create_backend<I: InputBackend, R: RenderBackend>(input_backend: I, render_backend: R) -> MergedBackend<I, R> {
	MergedBackend {
		input_backend,
		render_backend,
	}
}

#[derive(Debug, Clone, PartialEq)]
pub enum BackendEvent {
	KeyPress(KeyPress),
	PointerMotion(PointerMotion),
	PointerButton(PointerButton),
	StopRequested,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KeyPress {
	pub serial: u32,
	pub time: u32,
	pub key: u32,
	pub state: wl_keyboard::KeyState,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PointerMotion {
	pub serial: u32,
	pub time: u32,
	pub dx: f64,
	pub dx_unaccelerated: f64,
	pub dy: f64,
	pub dy_unaccelerated: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PointerButton {
	pub serial: u32,
	pub time: u32,
	pub button: u32,
	pub state: wl_pointer::ButtonState,
}
