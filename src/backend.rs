pub use std::os::raw::c_void;
pub use std::os::unix::io::{AsRawFd, RawFd};

use std::{error::Error as StdError, fmt};

use calloop::channel::Channel;
use wayland_server::protocol::*;
// TODO remove this so Festus becomes an optional dependency
use festus::geometry::*;

pub(crate) mod easy_shm;
pub mod libinput;
pub mod vulkan;
pub mod winit;

pub trait InputBackend {
	type Error: fmt::Debug + fmt::Display;

	fn update(&mut self) -> Result<(), Self::Error>;

	fn get_event_source(&mut self) -> Channel<BackendEvent>;
}

pub trait ShmBuffer {
	fn offset(&self) -> usize;
	fn width(&self) -> u32;
	fn height(&self) -> u32;
	fn stride(&self) -> u32;
	fn format(&self) -> wl_shm::Format;
}

pub unsafe trait RenderTarget {
	fn size(&self) -> Size;
}

pub trait GraphicsBackend: fmt::Debug {
	type Error: StdError + fmt::Debug + fmt::Display;

	type ShmPool: Send + fmt::Debug;
	type ShmBuffer: ShmBuffer + Send + fmt::Debug + 'static;

	type VertexBufferHandle: Copy + Send + fmt::Debug;
	type TextureHandle: Copy + Send + fmt::Debug;
	type MvpBufferHandle: Copy + Send + fmt::Debug;

	type RenderTargetHandle: Copy + Send + fmt::Debug + 'static;

	fn update(&mut self) -> Result<(), Self::Error>;

	fn create_shm_pool(&mut self, fd: RawFd, size: usize) -> Result<Self::ShmPool, Self::Error>;

	fn resize_shm_pool(&mut self, shm_pool: &mut Self::ShmPool, new_size: usize) -> Result<(), Self::Error>;

	fn create_shm_buffer(
		&mut self,
		shm_pool: &mut Self::ShmPool,
		offset: usize,
		width: u32,
		height: u32,
		stride: u32,
		format: wl_shm::Format,
	) -> Result<Self::ShmBuffer, Self::Error>;
	
	fn create_texture_from_rgba(&mut self, rgba: RgbaInfo) -> Result<Self::TextureHandle, Self::Error>;

	fn create_texture_from_shm_buffer(
		&mut self,
		shm_buffer: &Self::ShmBuffer,
	) -> Result<Self::TextureHandle, Self::Error>;

	fn create_vertex_buffer(
		&mut self,
		vertices: &[Vertex],
		indices: &[u32],
	) -> Result<Self::VertexBufferHandle, Self::Error>;

	fn create_mvp_buffer(&mut self, mvp: [[[f32; 4]; 4]; 3]) -> Result<Self::MvpBufferHandle, Self::Error>;
	
	// TODO! this returns a mutable reference with the same lifetime as self, which is not correct.
	// This should instead be done with a closure that takes the mutable reference as an argument.
	fn map_mvp_buffer(&mut self, handle: Self::MvpBufferHandle) -> Option<&mut [[[f32; 4]; 4]; 3]>;

	fn create_texture(&mut self, size: Size) -> Result<Self::TextureHandle, Self::Error>;

	fn create_render_target(&mut self, size: Size) -> Result<Self::RenderTargetHandle, Self::Error>;

	unsafe fn begin_render_pass(&mut self, target: Self::RenderTargetHandle) -> Result<(), Self::Error>;

	unsafe fn draw(
		&mut self,
		vertex_buffer: Self::VertexBufferHandle,
		texture: Self::TextureHandle,
		mvp: Self::MvpBufferHandle,
	) -> Result<(), Self::Error>;

	unsafe fn end_render_pass(&mut self, target: Self::RenderTargetHandle) -> Result<(), Self::Error>;

	fn present_target(&mut self, handle: Self::RenderTargetHandle) -> Result<(), Self::Error>;
	
	fn destroy_texture(&mut self, handle: Self::TextureHandle) -> Result<(), Self::Error>;
	
	fn destroy_vertex_buffer(&mut self, handle: Self::VertexBufferHandle) -> Result<(), Self::Error>;
	
	fn destroy_mvp_buffer(&mut self, handle: Self::MvpBufferHandle) -> Result<(), Self::Error>;
	
	fn destroy_render_target(&mut self, handle: Self::RenderTargetHandle) -> Result<(), Self::Error>;

	fn get_size(&self) -> Size;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
	pub pos: [f32; 3],
	pub uv: [f32; 2],
}

pub struct RgbaInfo<'a> {
	pub width: u32,
	pub height: u32,
	pub data: &'a [u8],
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
