use std::{
	os::unix::io::{RawFd},
	error::Error as StdError, fmt
};

use wl_server::{
	protocol::*,
};

use calloop::channel::Channel;
// TODO remove this so Festus becomes an optional dependency
use festus::geometry::*;

pub(crate) mod easy_shm;
pub mod libinput;
pub mod vulkan;
pub mod winit;

pub trait InputBackend: 'static {
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

pub struct OutputInfo {
	pub size: Size,
}

pub trait GraphicsBackend: Sized + fmt::Debug + 'static {
	type Error: StdError + fmt::Debug + fmt::Display;

	type ShmPool: Send + fmt::Debug;
	type ShmBuffer: ShmBuffer + Send + fmt::Debug + 'static;

	type VertexBufferHandle: Copy + Send + fmt::Debug;
	type TextureHandle: Copy + Send + fmt::Debug;
	type MvpBufferHandle: Copy + Send + fmt::Debug;

	type RenderTargetHandle: Copy + Send + Sync + fmt::Debug + 'static;

	type OutputHandle: Copy + Send + Sync + fmt::Debug;

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

	fn get_current_outputs(&self) -> Vec<Self::OutputHandle>;

	fn get_output_info(&self, output: Self::OutputHandle) -> Result<OutputInfo, Self::Error>;

	unsafe fn begin_render_pass(&mut self, target: Self::RenderTargetHandle) -> Result<(), Self::Error>;

	unsafe fn draw(
		&mut self,
		vertex_buffer: Self::VertexBufferHandle,
		texture: Self::TextureHandle,
		mvp: Self::MvpBufferHandle,
	) -> Result<(), Self::Error>;

	unsafe fn end_render_pass(&mut self, target: Self::RenderTargetHandle) -> Result<(), Self::Error>;

	fn present_target(
		&mut self,
		output: Self::OutputHandle,
		handle: Self::RenderTargetHandle,
	) -> Result<(), Self::Error>;

	fn destroy_texture(&mut self, handle: Self::TextureHandle) -> Result<(), Self::Error>;

	fn destroy_vertex_buffer(&mut self, handle: Self::VertexBufferHandle) -> Result<(), Self::Error>;

	fn destroy_mvp_buffer(&mut self, handle: Self::MvpBufferHandle) -> Result<(), Self::Error>;

	fn destroy_render_target(&mut self, handle: Self::RenderTargetHandle) -> Result<(), Self::Error>;
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

pub enum GraphicsBackendEvent<G: GraphicsBackend> {
	OutputAdded(G::OutputHandle),
	OutputRemoved(G::OutputHandle),
}

#[derive(Debug, Clone, PartialEq)]
pub enum BackendEvent {
	KeyPress(KeyPress),
	PointerMotion(PointerMotion),
	PointerButton(PointerButton),
	StopRequested,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PressState {
	Press,
	Release,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KeyPress {
	pub serial: u32,
	pub time: u32,
	pub key: u32,
	pub state: PressState,
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
	pub button: Button,
	pub state: PressState,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Button {
	Left,
	Right,
	Middle,
	Other(u8),
}

impl Button {
	pub fn to_wl(self) -> u32 {
		// According to smithay, this is how wayland sees mouse buttons
		match self {
			Button::Left => 0x110,
			Button::Right => 0x111,
			Button::Middle => 0x112,
			Button::Other(b) => b.into(),
		}
	}
}

// This is ridiculous
impl From<wl_keyboard::KeyState> for PressState {
	fn from(t: wl_keyboard::KeyState) -> Self {
		match t {
			wl_keyboard::KeyState::Pressed => Self::Press,
			wl_keyboard::KeyState::Released => Self::Release,
		}
	}
}

impl From<PressState> for wl_keyboard::KeyState {
	fn from(t: PressState) -> Self {
		match t {
			PressState::Press => wl_keyboard::KeyState::Pressed,
			PressState::Release => wl_keyboard::KeyState::Released,
		}
	}
}

impl From<wl_pointer::ButtonState> for PressState {
	fn from(t: wl_pointer::ButtonState) -> Self {
		match t {
			wl_pointer::ButtonState::Pressed => Self::Press,
			wl_pointer::ButtonState::Released => Self::Release,
		}
	}
}

impl From<PressState> for wl_pointer::ButtonState {
	fn from(t: PressState) -> Self {
		match t {
			PressState::Press => wl_pointer::ButtonState::Pressed,
			PressState::Release => wl_pointer::ButtonState::Released,
		}
	}
}

impl From<xkbcommon::xkb::KeyDirection> for PressState {
	fn from(t: xkbcommon::xkb::KeyDirection) -> Self {
		match t {
			xkbcommon::xkb::KeyDirection::Down => Self::Press,
			xkbcommon::xkb::KeyDirection::Up => Self::Release,
		}
	}
}

impl From<PressState> for xkbcommon::xkb::KeyDirection {
	fn from(t: PressState) -> Self {
		match t {
			PressState::Press => xkbcommon::xkb::KeyDirection::Down,
			PressState::Release => xkbcommon::xkb::KeyDirection::Up,
		}
	}
}

impl From<input::event::pointer::ButtonState> for PressState {
	fn from(t: input::event::pointer::ButtonState) -> Self {
		match t {
			input::event::pointer::ButtonState::Pressed => Self::Press,
			input::event::pointer::ButtonState::Released => Self::Release,
		}
	}
}

impl From<PressState> for input::event::pointer::ButtonState {
	fn from(t: PressState) -> Self {
		match t {
			PressState::Press => input::event::pointer::ButtonState::Pressed,
			PressState::Release => input::event::pointer::ButtonState::Released,
		}
	}
}

impl From<input::event::keyboard::KeyState> for PressState {
	fn from(t: input::event::keyboard::KeyState) -> Self {
		match t {
			input::event::keyboard::KeyState::Pressed => Self::Press,
			input::event::keyboard::KeyState::Released => Self::Release,
		}
	}
}

impl From<PressState> for input::event::keyboard::KeyState {
	fn from(t: PressState) -> Self {
		match t {
			PressState::Press => input::event::keyboard::KeyState::Pressed,
			PressState::Release => input::event::keyboard::KeyState::Released,
		}
	}
}

impl From<::winit::event::ElementState> for PressState {
	fn from(t: ::winit::event::ElementState) -> Self {
		match t {
			::winit::event::ElementState::Pressed => Self::Press,
			::winit::event::ElementState::Released => Self::Release,
		}
	}
}

impl From<PressState> for ::winit::event::ElementState {
	fn from(t: PressState) -> Self {
		match t {
			PressState::Press => ::winit::event::ElementState::Pressed,
			PressState::Release => ::winit::event::ElementState::Released,
		}
	}
}
