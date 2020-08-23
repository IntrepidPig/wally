use std::{
	fmt,
	fs::{self},
	marker::PhantomData,
	sync::atomic::{AtomicBool, AtomicU32, Ordering},
	time::{Duration, Instant},
};

use calloop::{
	signals::{Signal, Signals},
	EventLoop, LoopHandle, Source,
};
use std::sync::{Arc, Mutex};
use thiserror::Error;

use crate::{
	backend::{BackendEvent, GraphicsBackend, InputBackend},
	behavior::WindowManager,
	compositor::prelude::*,
	compositor::surface::SurfaceData,
	input::KeyboardState,
	renderer::{Output, Renderer},
};
use wl_server::server::{ServerCreateError, ServerError};

pub mod client;
pub mod compositor;
pub mod data_device;
pub mod shm;
pub mod surface;
pub mod output;
pub mod role;
pub mod seat;
pub mod shell;
pub mod xdg;

pub mod prelude {
	pub use std::{
		marker::PhantomData,
		sync::{Arc, Mutex},
		cell::{RefCell},
	};

	pub use loaner::{Owner, Handle, Ref};

	//pub use wayland_server::{protocol::*, Client, Display, Filter, Main};
	pub use wl_server::{
		protocol::*,
		Client, Server, Resource, NewResource, Global,
	};

	pub use festus::geometry::*;

	pub use crate::{
		impl_user_data, impl_user_data_graphics,
		backend::{BackendEvent, GraphicsBackend, InputBackend, KeyPress, PointerButton, PointerMotion, PressState},
		compositor::{
			CompositorState, client::ClientState, role::Role, surface::SurfaceData, PointerState, Synced, UserDataAccess,
			shm::{BufferData},
		},
	};
}

pub type Synced<T> = Arc<Mutex<T>>;

pub(crate) static INPUT_SERIAL: AtomicU32 = AtomicU32::new(1);
pub(crate) static PROFILE_OUTPUT: AtomicBool = AtomicBool::new(false);
pub(crate) static DEBUG_OUTPUT: AtomicBool = AtomicBool::new(false);

pub fn get_input_serial() -> u32 {
	INPUT_SERIAL.fetch_add(1, Ordering::Relaxed)
}

pub fn profile_output() -> bool {
	PROFILE_OUTPUT.load(Ordering::Relaxed)
}

pub fn debug_output() -> bool {
	DEBUG_OUTPUT.load(Ordering::Relaxed)
}

pub struct Compositor<I: InputBackend, G: GraphicsBackend> {
	pub(crate) server: Server,
	_signal_event_source: Source<Signals>,
	_idle_event_source: calloop::Idle,
	//_display_event_source: calloop::Source<calloop::generic::Generic<calloop::generic::EventedRawFd>>,
	_input_event_source: calloop::Source<calloop::channel::Channel<BackendEvent>>,
	_phantom: PhantomData<(I, G)>,
}

pub struct CompositorState<I: InputBackend, G: GraphicsBackend> {
	pub input_state: InputBackendState<I>,
	pub graphics_state: GraphicsBackendState<G>,
	pub inner: CompositorInner<I, G>,
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

pub struct CompositorInner<I: InputBackend, G: GraphicsBackend> {
	running: bool,
	pub window_manager: WindowManager<G>,
	pub pointer: PointerState,
	pub pointer_focus: Option<Resource<WlSurface>>,
	pub keyboard_state: KeyboardState,
	pub keyboard_focus: Option<Resource<WlSurface>>,
	pub output_globals: Vec<(Handle<Global>, Output<G>)>,
	phantom: PhantomData<I>,
}

pub struct PointerState {
	pub pos: (f64, f64),
	pub sensitivity: f64,
	pub custom_cursor: Option<CustomCursor>,
}

impl PointerState {
	pub fn new() -> Self {
		Self {
			pos: (0.0, 0.0),
			sensitivity: 1.0,
			custom_cursor: None
		}
	}
}

impl fmt::Debug for PointerState {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct("PointerState")
			.field("pos", &self.pos)
			.field("default", &"<default>")
			.field("custom_cursor", &self.custom_cursor)
			.finish()
	}
}

pub struct CustomCursor {
	pub surface: wl_surface::WlSurface,
	pub hotspot: Point,
}

impl fmt::Debug for CustomCursor {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct("CustomCursor")
			.field("surface", &"<WlSurface>")
			.field("hotspot", &self.hotspot)
			.finish()
	}
}

pub struct ClientResources {
	pub client: Client,
	pub keyboard: Option<wl_keyboard::WlKeyboard>,
	pub pointer: Option<wl_pointer::WlPointer>,
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
						compositor.handle_input_event(event);
					}
				},
			)
			.expect("Failed to insert input event source");

		let pointer_state = PointerState::new();
		let keyboard_state = KeyboardState::new();

		let window_manager = WindowManager::new(Box::new(crate::behavior::DumbWindowManagerBehavior::new()));

		let inner = CompositorInner {
			running: true,
			window_manager,
			pointer: pointer_state,
			pointer_focus: None,
			keyboard_state,
			keyboard_focus: None,
			output_globals: Vec::new(),
			phantom: PhantomData,
		};

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
		log::debug!("Debug info goes here");
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
						for surface in inner.window_manager.manager_impl.surfaces_ascending() {
							scene_render_state.draw_surface(surface)?;
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
			match self.server.dispatch(|_handle| RefCell::new(ClientState::new())) {
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

	pub fn handle_input_event(&mut self, event: BackendEvent) {
		let state = self.state_mut();
		match event {
			BackendEvent::StopRequested => {
				state.inner.running = false;
			}
			BackendEvent::KeyPress(key_press) => {
				let state_change = state.inner.keyboard_state.update_key(key_press.clone());

				// Send the key event to the surface that currently has keyboard focus, and an updated modifiers event if modifiers changed.
				if let Some(focused) = state.inner.keyboard_focus.clone() {
					// TODO: check aliveness
					let client = focused.client();
					let client = client.get().unwrap();
					let client_state = client.state::<RefCell<ClientState>>();
					let client_state = client_state.borrow();

					for keyboard in &client_state.keyboards {
						if state_change {
							let mods = state.inner.keyboard_state.xkb_modifiers_state;
							let modifiers_event = wl_keyboard::ModifiersEvent {
								serial: key_press.serial,
								mods_depressed: mods.mods_depressed,
								mods_latched: mods.mods_latched,
								mods_locked: mods.mods_locked,
								group: mods.group,
							};
							keyboard.send_event(WlKeyboardEvent::Modifiers(modifiers_event));
						}
						let key_event = wl_keyboard::KeyEvent {
							serial: key_press.serial,
							time: key_press.time,
							key: key_press.key,
							state: key_press.state.into(),
						};
						keyboard.send_event(WlKeyboardEvent::Key(key_event));
					}
				}
			}
			BackendEvent::PointerMotion(pointer_motion) => {
				let mut pointer_state = &mut state.inner.pointer;

				pointer_state.pos.0 += pointer_motion.dx_unaccelerated * pointer_state.sensitivity;
				pointer_state.pos.1 += pointer_motion.dy_unaccelerated * pointer_state.sensitivity;
				let pointer_pos = Point::new(pointer_state.pos.0.round() as i32, pointer_state.pos.1.round() as i32);

				if let Some(surface) = state.inner.window_manager.get_window_under_point(pointer_pos) {
					let surface_data: Ref<RefCell<SurfaceData<G>>> = surface.get_user_data();
					let client = surface.client();
					let client = client.get().unwrap();
					let client_state = client.state::<RefCell<ClientState>>();

					let surface_relative_coords =
						if let Some(geometry) = surface_data.borrow().try_get_surface_geometry() {
							Point::new(pointer_pos.x - geometry.x, pointer_pos.y - geometry.y)
						} else {
							// This should probably not happen because the window manager just told us the pointer is
							// over this window, implying it has geometry
							Point::new(0, 0)
						};

					if let Some(old_pointer_focus) = state.inner.pointer_focus.clone() {
						let old_surface_client = old_pointer_focus.client();
						let old_surface_client = old_surface_client.get().unwrap();
						let old_surface_client_state = old_surface_client.state::<RefCell<ClientState>>();

						if old_pointer_focus.is(&surface) {
							// The pointer is over the same surface as it was previously, do not send any focus events
						} else {
							// The pointer is over a different surface, unfocus the old one and focus the new one
							for pointer in &old_surface_client_state.borrow().pointers {
								pointer.send_event(WlPointerEvent::Leave(wl_pointer::LeaveEvent {
									serial: get_input_serial(),
									surface: old_pointer_focus.clone(),
								}));
							}
							for pointer in &client_state.borrow().pointers {
								pointer.send_event(WlPointerEvent::Enter(wl_pointer::EnterEvent {
									serial: get_input_serial(),
									surface: surface.clone(),
									surface_x: (surface_relative_coords.x as f64).into(),
									surface_y: (surface_relative_coords.y as f64).into(),
								}));
							}
							state.inner.pointer_focus = Some(surface.clone())
						}
					} else {
						// The pointer has entered a surface while no other surface is focused, focus this surface
						for pointer in &client_state.borrow().pointers {
							pointer.send_event(WlPointerEvent::Enter(wl_pointer::EnterEvent {
								serial: get_input_serial(),
								surface: surface.clone(),
								surface_x: (surface_relative_coords.x as f64).into(),
								surface_y: (surface_relative_coords.y as f64).into(),
							}));
						}
						state.inner.pointer_focus = Some(surface.clone());
					}

					// Send the surface the actual motion event
					for pointer in &client_state.borrow().pointers {
						pointer.send_event(WlPointerEvent::Motion(wl_pointer::MotionEvent {
							time: get_input_serial(),
							surface_x: (surface_relative_coords.x as f64).into(),
							surface_y: (surface_relative_coords.y as f64).into(),
						}));
					}
				} else {
					// The pointer is not over any surface, remove pointer focus from the previous focused surface if any
					if let Some(old_pointer_focus) = state.inner.pointer_focus.take() {
						let client = old_pointer_focus.client();
						let client = client.get().unwrap();
						let client_state = client.state::<RefCell<ClientState>>();

						for pointer in &client_state.borrow().pointers {
							pointer.send_event(WlPointerEvent::Leave(wl_pointer::LeaveEvent {
								serial: get_input_serial(),
								surface: old_pointer_focus.clone(),
							}));
						}
					}
				}
			}
			BackendEvent::PointerButton(pointer_button) => {
				let pointer_state = &mut state.inner.pointer;
				let pointer_pos = Point::new(pointer_state.pos.0.round() as i32, pointer_state.pos.1.round() as i32);

				if let Some(surface) = state.inner.window_manager.get_window_under_point(pointer_pos) {
					let client = surface.client();
					let client = client.get().unwrap();
					let client_state = client.state::<RefCell<ClientState>>();

					if pointer_button.state == PressState::Press {
						if let Some(old_keyboard_focus) = state.inner.keyboard_focus.clone() {
							if old_keyboard_focus.is(&surface) {
								// No focus change, this is the same surface
							} else {
								// Change the keyboard focus
								let old_surface_client = old_keyboard_focus.client();
								let old_surface_client = old_surface_client.get().unwrap();
								let old_surface_client_state = old_surface_client.state::<RefCell<ClientState>>();

								for keyboard in &old_surface_client_state.borrow().keyboards {
									keyboard.send_event(WlKeyboardEvent::Leave(wl_keyboard::LeaveEvent {
										serial: get_input_serial(),
										surface: old_keyboard_focus.clone(),
									}));
								}
								for keyboard in &client_state.borrow().keyboards {
									let mods = state.inner.keyboard_state.xkb_modifiers_state;
									let modifiers_event = wl_keyboard::ModifiersEvent {
										serial: get_input_serial(),
										mods_depressed: mods.mods_depressed,
										mods_latched: mods.mods_latched,
										mods_locked: mods.mods_locked,
										group: mods.group,
									};
									let enter_event = wl_keyboard::EnterEvent {
										serial: get_input_serial(),
										surface: surface.clone(),
										keys: Vec::new(), // TODO: actual value
									};
									keyboard.send_event(WlKeyboardEvent::Modifiers(modifiers_event));
									keyboard.send_event(WlKeyboardEvent::Enter(enter_event));
								}
								state.inner.keyboard_focus = Some(surface.clone());
							}
						} else {
							// Focus the keyboard on a window when there was no previously focused window
							for keyboard in &client_state.borrow().keyboards {
								let mods = state.inner.keyboard_state.xkb_modifiers_state;
								let modifiers_event = wl_keyboard::ModifiersEvent {
									serial: get_input_serial(),
									mods_depressed: mods.mods_depressed,
									mods_latched: mods.mods_latched,
									mods_locked: mods.mods_locked,
									group: mods.group,
								};
								let enter_event = wl_keyboard::EnterEvent {
									serial: get_input_serial(),
									surface: surface.clone(),
									keys: Vec::new(), // TODO: actual value
								};
								keyboard.send_event(WlKeyboardEvent::Modifiers(modifiers_event));
								keyboard.send_event(WlKeyboardEvent::Enter(enter_event));
							}
							state.inner.keyboard_focus = Some(surface.clone());
						}
					}
				} else {
					// Remove the keyboard focus from the current focus if empty space is clicked
					if let Some(old_keyboard_focus) = state.inner.keyboard_focus.take() {
						let old_surface_client = old_keyboard_focus.client();
						let old_surface_client = old_surface_client.get().unwrap();
						let old_surface_client_state = old_surface_client.state::<RefCell<ClientState>>();

						for keyboard in &old_surface_client_state.borrow().keyboards {
							keyboard.send_event(WlKeyboardEvent::Leave(wl_keyboard::LeaveEvent {
								serial: get_input_serial(),
								surface: old_keyboard_focus.clone(),
							}));
						}
					}
				}

				// Send event to focused window
				if let Some(focused) = state.inner.keyboard_focus.clone() {
					let client = focused.client();
					let client = client.get().unwrap();
					let client_state = client.state::<RefCell<ClientState>>();

					for pointer in &client_state.borrow().pointers {
						pointer.send_event(WlPointerEvent::Button(wl_pointer::ButtonEvent {
							serial: pointer_button.serial,
							time: pointer_button.time,
							button: pointer_button.button.to_wl(),
							state: pointer_button.state.into(),
						}));
					}
				}
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

pub trait UserDataAccess<T> {
	fn try_get_user_data(&self) -> Option<Ref<T>>;

	fn get_user_data(&self) -> Ref<T> {
		self.try_get_user_data().expect("Object was destroyed")
	}
}

#[macro_export]
macro_rules! impl_user_data_graphics {
	($i:ty, $t:ty) => {
		impl<G: GraphicsBackend> UserDataAccess<$t> for Resource<$i> {
			fn try_get_user_data(&self) -> Option<Ref<$t>> {
				self.object().get().map(|object| {
					let data = object.get_data::<$t>().unwrap().upgrade();
					data.custom_ref()
				})
			}
		}
	};
}

#[macro_export]
macro_rules! impl_user_data {
	($i:ty, $t:ty) => {
		impl UserDataAccess<$t> for Resource<$i> {
			fn try_get_user_data(&self) -> Option<Ref<$t>> {
				self.object().get().map(|object| {
					let data = object.get_data::<$t>().unwrap().upgrade();
					data.custom_ref()
				})
			}
		}
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
