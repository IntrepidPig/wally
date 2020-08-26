use std::{
	marker::{PhantomData},
	time::{Instant},
};

use loaner::Handle;
use wl_server::{
	Resource, Global,
	protocol::*,
};

use crate::{
	backend::{InputBackend, GraphicsBackend},
	renderer::{Renderer, Output},
	behavior::{
		window::{WindowManager},
		input::{KeyboardState},
	},
	compositor::{
		surface::{SurfaceData},
	},
};

pub mod client;
pub mod window;
pub mod input;

pub struct CompositorState<I: InputBackend, G: GraphicsBackend> {
	pub input_state: InputBackendState<I>,
	pub graphics_state: GraphicsBackendState<G>,
	pub inner: CompositorInner<I, G>,
}

impl<I: InputBackend, G: GraphicsBackend> CompositorState<I, G> {
	pub fn print_debug_info(&self) {
		
	}
}

pub struct InputBackendState<I: InputBackend> {
	pub backend: I,
}

impl<I: InputBackend> InputBackendState<I> {
	pub fn update(&mut self) -> Result<(), I::Error> {
		self.backend.update()
	}
}

pub struct GraphicsBackendState<G: GraphicsBackend> {
	pub renderer: Renderer<G>,
}

impl<G: GraphicsBackend> GraphicsBackendState<G> {
	pub fn update(&mut self) -> Result<(), G::Error> {
		self.renderer.update()
	}
}

#[derive(Debug)]
pub struct CompositorInner<I: InputBackend, G: GraphicsBackend> {
	pub running: bool,
	pub start_time: Instant,
	pub window_manager: WindowManager<G>,
	pub pointer: PointerState,
	pub pointer_focus: Option<Resource<WlSurface, SurfaceData<G>>>,
	pub keyboard_state: KeyboardState,
	pub keyboard_focus: Option<Resource<WlSurface, SurfaceData<G>>>,
	pub output_globals: Vec<(Handle<Global>, Output<G>)>,
	_phantom: PhantomData<I>,
}

impl<I: InputBackend, G: GraphicsBackend> CompositorInner<I, G> {
	pub fn new() -> Self {
		Self {
			running: true,
			start_time: Instant::now(),
			window_manager: WindowManager::new(),
			pointer: PointerState::new(),
			pointer_focus: None,
			keyboard_state: KeyboardState::new(),
			keyboard_focus: None,
			output_globals: Vec::new(),
			_phantom: PhantomData,
		}
	}
}

#[derive(Debug)]
pub struct PointerState {
	pub pos: (f64, f64),
	pub sensitivity: f64,
}

impl PointerState {
	pub fn new() -> Self {
		Self {
			pos: (0.0, 0.0),
			sensitivity: 1.0,
		}
	}
}