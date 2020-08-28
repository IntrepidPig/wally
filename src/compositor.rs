use std::{
	fs::{self},
	marker::PhantomData,
	sync::atomic::{AtomicBool, Ordering},
	time::{Duration, Instant},
	cmp,
};

use once_cell::{
	sync::{Lazy},
};
use calloop::{
	signals::{Signal, Signals},
	EventLoop, LoopHandle, Source,
};
use thiserror::Error;

use self::prelude::*;

pub mod prelude {
	pub use std::{
		marker::{PhantomData},
		cell::{RefCell},
	};

	pub use festus::geometry::*;
	pub use loaner::{Owner, Handle, Ref};
	pub use wl_server::{
		protocol::*,
		Client, Server, ServerError, ServerCreateError, Resource, NewResource, Global,
	};

	pub use crate::{
		backend::{BackendEvent, GraphicsBackend, InputBackend, KeyPress, PointerMotion, PointerButton, PressState},
		behavior::{
			CompositorState, InputBackendState, GraphicsBackendState, PointerState, CompositorInner,
			client::{ClientState},
			window::{WindowManager},
			input::{KeyboardState},
		},
		compositor::{
			get_time_ms,
			Compositor, Serial,
			surface::{Role, SurfaceData},
			output::{OutputData},
		},
		renderer::{Renderer},
	};
}

pub mod subcompositor;
pub mod compositor;
pub mod data_device;
pub mod shm;
pub mod surface;
pub mod output;
pub mod seat;
pub mod shell;
pub mod xdg;

pub(crate) static PROFILE_OUTPUT: AtomicBool = AtomicBool::new(false);
pub(crate) static START_TIME: Lazy<Instant> = Lazy::new(|| Instant::now());
pub(crate) static DEBUG_OUTPUT: AtomicBool = AtomicBool::new(false);

pub fn profile_output() -> bool {
	PROFILE_OUTPUT.load(Ordering::Relaxed)
}

pub fn debug_output() -> bool {
	DEBUG_OUTPUT.load(Ordering::Relaxed)
}

// TODO: this or the one stored in CompositorState
pub fn get_time_ms() -> u32 {
	let elapsed = START_TIME.elapsed();
	((elapsed.as_secs() % std::u32::MAX as u64) as u32).wrapping_add(elapsed.subsec_nanos())
}

#[derive(Debug, Clone, Copy, Eq)]
pub struct Serial(pub u64);

impl Serial {
	pub fn advance(&mut self) -> Serial {
		let old = *self;
		self.0 = self.0 + 1;
		if self.0 % std::u32::MAX as u64 == 0 {
			log::warn!("Input serial wrapped around, not sure what happens here");
		}
		old
	}
}

impl From<u32> for Serial {
	fn from(val: u32) -> Self {
		Self(val as u64)
	}
}

impl From<Serial> for u32 {
	fn from(val: Serial) -> Self {
		(val.0 % std::u32::MAX as u64) as u32
	}
}

impl PartialEq<Serial> for Serial {
    fn eq(&self, other: &Serial) -> bool {
        u32::from(*self) == u32::from(*other)
    }
}

impl PartialOrd for Serial {
	fn partial_cmp(&self, other: &Serial) -> Option<cmp::Ordering> {
		Some(u32::from(*self).cmp(&u32::from(*other)))
	}
}

pub struct Compositor<I: InputBackend, G: GraphicsBackend> {
	pub(crate) server: Server,
	_signal_event_source: Source<Signals>,
	_idle_event_source: calloop::Idle,
	_input_event_source: calloop::Source<calloop::channel::Channel<BackendEvent>>,
	_phantom: PhantomData<(I, G)>,
}

impl<I: InputBackend, G: GraphicsBackend> Compositor<I, G> {
	pub fn new(
		mut input_backend: I,
		graphics_backend: G,
		event_loop_handle: LoopHandle<Compositor<I, G>>,
	) -> Result<Self, CompositorError<G>> {
		let signals = Signals::new(&[Signal::SIGINT]).expect("Failed to setup signal handler");
		let signal_event_source = event_loop_handle
			.insert_source(
				signals,
				|_event: calloop::signals::Event, compositor: &mut Compositor<I, G>| {
					// TODO: this don't work (always)
					log::info!("Received sigint, exiting");
					compositor.state_mut().inner.running = false;
				},
			)
			.expect("Failed to insert signal handler in event loop");

		let idle_event_source = event_loop_handle.insert_idle(|_wally: &mut Compositor<I, G>| {});

		let input_events = input_backend.get_event_source();
		let input_event_source = event_loop_handle
			.insert_source(
				input_events,
				|e: calloop::channel::Event<BackendEvent>, compositor: &mut Compositor<I, G>| {
					if let calloop::channel::Event::Msg(event) = e {
						compositor.state_mut().handle_input_event(event);
					}
				},
			)
			.expect("Failed to insert input event source");

		let inner = CompositorInner::new();
		let input_state = InputBackendState { backend: input_backend };
		let renderer = Renderer::init(graphics_backend).unwrap(); // TODO no unwrap
		let graphics_state = GraphicsBackendState { renderer };
		let state = CompositorState {
			inner,
			input_state,
			graphics_state,
		};

		let server = Server::new(state)?;

		Ok(Self {
			server,
			_signal_event_source: signal_event_source,
			_idle_event_source: idle_event_source,
			//_display_event_source: display_event_source,
			_input_event_source: input_event_source,
			_phantom: PhantomData,
		})
	}

	pub fn state(&self) -> &CompositorState<I, G> {
		self.server.state.get::<CompositorState<I, G>>()
	}

	pub fn state_mut(&mut self) -> &mut CompositorState<I, G> {
		self.server.state.get_mut::<CompositorState<I, G>>()
	}

	pub fn print_debug_info(&self) {
		self.server.print_debug_info();
		self.state().print_debug_info();
	}

	pub fn start(&mut self, event_loop: &mut EventLoop<Compositor<I, G>>) {
		while self.state().inner.running {
			let main_start = Instant::now();
			{
				let start = Instant::now();

				let state = self.state_mut();
				state.input_state.update()
					.map_err(|_e| log::error!("Error updating the input backend"))
					.unwrap();
				if profile_output() {
					log::debug!("Input backend update: {} ms", start.elapsed().as_secs_f64() * 1000.0);
				}

				let start = Instant::now();
				state.graphics_state.update()
					.map_err(|e| log::error!("Error updating the render backend: {}", e))
					.unwrap();
				if profile_output() {
					log::debug!("Graphics backend update: {} ms", start.elapsed().as_secs_f64() * 1000.0);
				}
				
				let start = Instant::now();
				let inner = &state.inner;
				state.graphics_state
					.renderer
					.render_scene(|mut scene_render_state| {
						for node in inner.window_manager.tree.nodes_ascending() {
							scene_render_state.draw_node(&*node)?;
						}
						let pointer_state = &inner.pointer;
						let pointer_pos =
							Point::new(pointer_state.pos.0.round() as i32, pointer_state.pos.1.round() as i32);
						scene_render_state.draw_cursor(pointer_pos)?;
						Ok(())
					})
					.unwrap();
				state.graphics_state.renderer.present().unwrap();
				if profile_output() {
					log::debug!("Render time: {} ms", start.elapsed().as_secs_f64() * 1000.0);
				}
			}
			// TODO change timeout to something that syncs with rendering somehow. The timeout should be the time until
			// the next frame should start rendering.
			let start = Instant::now();
			match event_loop.dispatch(Some(Duration::from_millis(0)), self) {
				Ok(_) => {}
				Err(e) => {
					log::error!("An error occurred in the event loop: {}", e);
				}
			}
			match self.server.dispatch(|_handle| RefCell::new(ClientState::<G>::new())) {
				Ok(()) => {},
				Err(e) => {
					log::error!("Error dispatching requests: {}", e);
				},
			};
			if profile_output() {
				log::debug!("Event dispatch time: {} ms", start.elapsed().as_secs_f64() * 1000.0);
			}
			
			if debug_output() {
				self.print_debug_info();
			}
			let end = main_start.elapsed();
			if profile_output() {
				log::debug!("Ran frame in {} ms", end.as_secs_f64() * 1000.0);
			}
		}
	}

	pub fn init(&mut self) {
		self.setup_globals();
	}

	pub(crate) fn setup_globals(&mut self) {
		self.setup_compositor_global();
		self.setup_shm_global();
		self.setup_output_globals();
		self.setup_seat_global();
		self.setup_data_device_manager_global();
		self.setup_wl_shell_global();
		self.setup_xdg_wm_base_global();
	}
}

impl<I: InputBackend, G: GraphicsBackend> Drop for Compositor<I, G> {
	fn drop(&mut self) {
		log::info!("Closing wayland socket");
		fs::remove_file("/run/user/1000/wayland-0").unwrap();
	}
}

#[derive(Debug, Error)]
pub enum CompositorError<G: GraphicsBackend> {
	#[error("There was an error with the server")]
	ServerError(#[from] ServerError),
	#[error("There was an error creating the server")]
	CreateServerError(#[from] ServerCreateError),
	#[error("Failed to create a render target")]
	RenderTargetError(#[source] G::Error),
}
