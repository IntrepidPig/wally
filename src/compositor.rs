use std::{
	fmt,
	fs::{self},
	io::{self},
	marker::PhantomData,
	sync::atomic::{AtomicBool, AtomicU32, Ordering},
	time::{Duration, Instant},
};

use calloop::{
	mio,
	signals::{Signal, Signals},
	EventLoop, LoopHandle, Source,
};
use std::sync::{Arc, Mutex};
use thiserror::Error;
use wayland_server::{protocol::*, Interface, Resource, Client, Display, Filter, Main};

use crate::{
	backend::{BackendEvent, GraphicsBackend, InputBackend, ShmBuffer},
	behavior::{WindowManager},
	compositor::prelude::*,
	compositor::surface::SurfaceData,
	renderer::{Renderer},
};

pub mod client;
pub mod role;
pub mod seat;
pub mod shell;
pub mod shm;
pub mod surface;
pub mod xdg;

pub mod prelude {
	pub use std::{
		marker::PhantomData,
		sync::{Arc, Mutex},
	};

	pub use wayland_server::{protocol::*, Client, Display, Filter, Main};

	pub use festus::geometry::*;

	pub use crate::{
		backend::{BackendEvent, GraphicsBackend, InputBackend},
		compositor::{UserDataAccess, client::ClientInfo, role::Role, surface::SurfaceData, PointerState, Synced},
	};
}

pub type Synced<T> = Arc<Mutex<T>>;

/// Helper extension trait to clean up the access of UserData of a known type
pub trait UserDataAccess {
	fn get<T: 'static>(&self) -> &T;
	fn try_get<T: 'static>(&self) -> Option<&T>;
	fn try_get_synced<T: 'static>(&self) -> Option<Synced<T>>;
	fn get_synced<T: 'static>(&self) -> Synced<T>;
}

impl<I> UserDataAccess for I where I: Interface + AsRef<Resource<I>> + From<Resource<I>> {
	fn get<T: 'static>(&self) -> &T {
		self.try_get().unwrap()
	}

	fn try_get<T: 'static>(&self) -> Option<&T> {
		self.as_ref().user_data().get::<T>()
	}

	fn try_get_synced<T: 'static>(&self) -> Option<Synced<T>> {
		self.try_get::<Synced<T>>().map(Synced::clone)
	}

	fn get_synced<T: 'static>(&self) -> Synced<T> {
		self.try_get_synced().unwrap()
	}
}

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
	display: Display,
	inner: Arc<Mutex<CompositorInner<I, G>>>,
	pub(crate) input_backend_state: Arc<Mutex<InputBackendState<I>>>,
	pub(crate) graphics_backend_state: Arc<Mutex<GraphicsBackendState<G>>>,
	_signal_event_source: Source<Signals>,
	_idle_event_source: calloop::Idle,
	_display_event_source: calloop::Source<calloop::generic::Generic<calloop::generic::EventedRawFd>>,
	_input_event_source: calloop::Source<calloop::channel::Channel<BackendEvent>>,
}

pub struct InputBackendState<I: InputBackend> {
	pub input_backend: I,
}

pub struct GraphicsBackendState<G: GraphicsBackend> {
	pub renderer: Renderer<G>,
}

pub struct CompositorInner<I: InputBackend, G: GraphicsBackend> {
	running: bool,
	pub client_manager: ClientManager,
	pub window_manager: WindowManager<G>,
	pub pointer: Arc<Mutex<PointerState>>,
	pub pointer_focus: Option<wl_surface::WlSurface>,
	pub keyboard_focus: Option<wl_surface::WlSurface>,
	phantom: PhantomData<I>,
}

pub struct PointerState {
	pub pos: (f64, f64),
	pub sensitivity: f64,
	pub custom_cursor: Option<CustomCursor>,
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

impl<I: InputBackend, G: GraphicsBackend> CompositorInner<I, G> {
	fn trim_dead_clients(&mut self) {
		/* self.surface_tree.surfaces.retain(|surface| {
			log::debug!("Checking surface");
			if !surface.as_ref().is_alive() {
				log::debug!("Destroying surface");
				false
			} else {
				true
			}
		}) */
	}
}

pub struct ClientManager {
	pub clients: Vec<Arc<Mutex<ClientInfo>>>,
}

impl ClientManager {
	pub fn new() -> Self {
		Self { clients: Vec::new() }
	}

	pub fn get_client_info(&mut self, client: Client) -> Synced<ClientInfo> {
		// This is written weirdly to bypass borrow checker issues
		if self.clients.iter().any(|r| r.lock().unwrap().client.equals(&client)) {
			Arc::clone(
				&self
					.clients
					.iter()
					.find(|r| r.lock().unwrap().client.equals(&client))
					.unwrap(),
			)
		} else {
			self.clients.push(Arc::new(Mutex::new(ClientInfo {
				client,
				keyboards: Vec::new(),
				pointers: Vec::new(),
			})));
			Arc::clone(self.clients.last().unwrap())
		}
	}
}

pub struct ClientResources {
	pub client: Client,
	pub keyboard: Option<wl_keyboard::WlKeyboard>,
	pub pointer: Option<wl_pointer::WlPointer>,
}

impl<I: InputBackend + 'static, G: GraphicsBackend + 'static> Compositor<I, G> {
	pub fn new(
		mut input_backend: I,
		graphics_backend: G,
		event_loop_handle: LoopHandle<Compositor<I, G>>,
	) -> Result<Self, CompositorError<G>> {
		let mut display = Display::new();
		//let f = fs::File::create("/run/user/1000/wayland-0").unwrap();
		display
			.add_socket::<&str>(None)
			.map_err(|e| CompositorError::SocketError(e))?;

		let signals = Signals::new(&[Signal::SIGINT]).expect("Failed to setup signal handler");
		let signal_event_source = event_loop_handle
			.insert_source(
				signals,
				|_event: calloop::signals::Event, compositor: &mut Compositor<I, G>| {
					log::info!("Received sigint, exiting");
					let mut inner = compositor.inner.lock().unwrap();
					inner.running = false;
				},
			)
			.expect("Failed to insert signal handler in event loop");

		let idle_event_source = event_loop_handle.insert_idle(|_wally: &mut Compositor<I, G>| {
			log::trace!("Finished processing events");
		});

		let mut display_events = calloop::generic::Generic::from_raw_fd(display.get_poll_fd());
		display_events.set_interest(mio::Ready::readable());
		display_events.set_pollopts(mio::PollOpt::edge());
		let display_event_source = event_loop_handle
			.insert_source(
				display_events,
				|_event: calloop::generic::Event<calloop::generic::EventedRawFd>, compositor: &mut Compositor<I, G>| {
					log::trace!("Got display event");
					compositor
						.display
						.dispatch(Duration::from_millis(0), &mut ())
						.map_err(|e| {
							log::error!("Failed to dispatch display events: {}", e);
						})
						.unwrap();
					compositor.display.flush_clients(&mut ());
				},
			)
			.expect("Failed to insert epoll fd in the event loop");

		let input_events = input_backend.get_event_source();
		let input_event_source = event_loop_handle
			.insert_source(
				input_events,
				|e: calloop::channel::Event<BackendEvent>, compositor: &mut Compositor<I, G>| {
					if let calloop::channel::Event::Msg(event) = e {
						log::trace!("Got input event");
						compositor.handle_input_event(event);
					}
				},
			)
			.expect("Failed to insert input event source");

		let client_manager = ClientManager::new();

		let pointer_state = Arc::new(Mutex::new(PointerState {
			pos: (0.0, 0.0),
			sensitivity: 1.0,
			custom_cursor: None,
		}));

		let inner = CompositorInner {
			running: true,
			client_manager,
			window_manager: WindowManager::new(Box::new(crate::behavior::DumbWindowManagerBehavior::new(Arc::clone(
				&pointer_state,
			)))),
			//surface_tree: SurfaceTree::new(Arc::clone(&pointer_state)),
			pointer: pointer_state,
			pointer_focus: None,
			keyboard_focus: None,
			phantom: PhantomData,
		};

		let input_backend_state = Arc::new(Mutex::new(InputBackendState { input_backend }));

		let renderer = Renderer::init(graphics_backend).unwrap(); // TODO no unwrap

		let graphics_backend_state = Arc::new(Mutex::new(GraphicsBackendState { renderer }));

		Ok(Self {
			display,
			inner: Arc::new(Mutex::new(inner)),
			input_backend_state,
			graphics_backend_state,
			_signal_event_source: signal_event_source,
			_idle_event_source: idle_event_source,
			_display_event_source: display_event_source,
			_input_event_source: input_event_source,
		})
	}

	pub fn print_debug_info(&self) {
		let inner = self.inner.lock().unwrap();
		println!("Surfaces:");
		for (i, surface) in inner.window_manager.manager_impl.surfaces_ascending().enumerate() {
			println!("\tSurface@{} {}", surface.as_ref().id(), i);
			let surface_data = surface.get_synced::<SurfaceData<G>>();
			let surface_data_lock = surface_data.lock().unwrap();
			if let Some(role) = surface_data_lock.role.as_ref() {
				println!("\t\tRole: {:?}", role);
			} else {
				println!("\t\tRole: None");
			}
			println!("\t\tAlive: {}", surface.as_ref().is_alive());
			println!(
				"\t\tClient: {}",
				surface
					.as_ref()
					.client()
					.map(|client| if client.alive() { "Alive client" } else { "Dead client" })
					.unwrap_or("No client")
			);
		}
	}

	pub fn start(&mut self, event_loop: &mut EventLoop<Compositor<I, G>>) {
		while self.inner.lock().unwrap().running {
			let start = Instant::now();
			{
				let mut inner = self.inner.lock().unwrap();
				let input_update_start = Instant::now();
				let mut input_backend_state = self.input_backend_state.lock().unwrap();
				input_backend_state
					.input_backend
					.update()
					.map_err(|_e| log::error!("Error updating the input backend"))
					.unwrap();
				if profile_output() {
					log::debug!(
						"Updated input backend in {} ms",
						input_update_start.elapsed().as_secs_f64() * 1000.0
					);
				}
				
				let render_update_start = Instant::now();
				let mut graphics_backend_state = self.graphics_backend_state.lock().unwrap();
				graphics_backend_state
					.renderer
					.update()
					.map_err(|_e| log::error!("Error updating the render backend"))
					.unwrap();
				if profile_output() {
					log::debug!(
						"Updated render backend in {} ms",
						render_update_start.elapsed().as_secs_f64() * 1000.0
					);
				}
				let inner = &mut *inner;
				let render_tree_start = Instant::now();
				let surfaces_iter = inner.window_manager.manager_impl.surfaces_ascending();
				graphics_backend_state
					.renderer
					.render_scene(|mut scene_render_state| {
						for surface in surfaces_iter {
							scene_render_state.draw_surface(surface.clone())?;
						}
						let pointer_state = inner.pointer.lock().unwrap();
						let pointer_pos = Point::new(pointer_state.pos.0.round() as i32, pointer_state.pos.1.round() as i32);
						scene_render_state.draw_cursor(pointer_pos)?;
						Ok(())
					})
					.unwrap();
				graphics_backend_state.renderer.present().unwrap();
				if profile_output() {
					log::debug!(
						"Rendered surface tree in {} ms",
						render_tree_start.elapsed().as_secs_f64() * 1000.0
					);
				}
			}
			// TODO change timeout to something that syncs with rendering somehow. The timeout should be the time until
			// the next frame should start rendering.
			let dispatch_start = Instant::now();
			match event_loop.dispatch(Some(Duration::from_millis(0)), self) {
				Ok(_) => {}
				Err(e) => {
					log::error!("An error occurred in the event loop: {}", e);
				}
			}
			if profile_output() {
				log::debug!(
					"Dispatched events in {} ms",
					dispatch_start.elapsed().as_secs_f64() * 1000.0
				);
			}
			let flush_start = Instant::now();
			self.display.flush_clients(&mut ());
			if profile_output() {
				log::debug!("Flushed clients in {} ms", flush_start.elapsed().as_secs_f64() * 1000.0);
			}
			if debug_output() {
				self.print_debug_info();
			}
			let end = start.elapsed();
			if profile_output() {
				log::debug!("Ran frame in {} ms", end.as_secs_f64() * 1000.0);
			}
		}
	}

	pub fn handle_input_event(&mut self, event: BackendEvent) {
		log::trace!("Got input: {:?}", event);
		let mut inner = self.inner.lock().unwrap();
		match event {
			BackendEvent::StopRequested => {
				inner.running = false;
			}
			BackendEvent::KeyPress(key_press) => {
				let inner = &mut *inner;
				if let Some(focused) = inner.keyboard_focus.clone() {
					dbg!(focused.as_ref().id());
					let surface_data = focused.get_synced::<SurfaceData<G>>();
					let surface_data_lock = surface_data.lock().unwrap();
					let client_info_lock = surface_data_lock.client_info.lock().unwrap();
					for keyboard in &client_info_lock.keyboards {
						log::debug!("Sending key to focused keyboard");
						dbg!(&key_press);
						keyboard.key(key_press.serial, key_press.time, key_press.key, key_press.state);
					}
				}
			}
			BackendEvent::PointerMotion(pointer_motion) => {
				let mut pointer_state_lock = inner.pointer.lock().unwrap();
				pointer_state_lock.pos.0 += pointer_motion.dx_unaccelerated * pointer_state_lock.sensitivity;
				pointer_state_lock.pos.1 += pointer_motion.dy_unaccelerated * pointer_state_lock.sensitivity;

				let pointer_pos = pointer_state_lock.pos;
				drop(pointer_state_lock);
				let pointer_pos = Point::new(pointer_pos.0.round() as i32, pointer_pos.1.round() as i32);

				if let Some(surface) = inner.window_manager.get_window_under_point(pointer_pos) {
					let surface_data = surface.get_synced::<SurfaceData<G>>();
					let surface_data_lock = surface_data.lock().unwrap();
					let surface_relative_coords =
						if let Some(surface_position) = surface_data_lock.try_get_surface_position() {
							Point::new(pointer_pos.x - surface_position.x, pointer_pos.y - surface_position.y)
						} else {
							log::error!("Surface had no position set!");
							Point::new(0, 0)
						};

					if let Some(old_pointer_focus) = inner.pointer_focus.clone() {
						if *surface.as_ref() == *old_pointer_focus.as_ref() {
							// The pointer is over the same surface as it was previously, do not send any focus events
						} else {
							// The pointer is over a different surface, unfocus the old one and focus the new one
							let old_surface_data = old_pointer_focus
								.get_synced::<SurfaceData<G>>();
							let old_surface_data_lock = old_surface_data.lock().unwrap();
							let old_client_info_lock = old_surface_data_lock.client_info.lock().unwrap();
							for pointer in &old_client_info_lock.pointers {
								pointer.leave(get_input_serial(), &old_pointer_focus);
							}
							drop(old_client_info_lock);
							drop(old_surface_data_lock);
							let surface_client_info_lock = surface_data_lock.client_info.lock().unwrap();
							for pointer in &surface_client_info_lock.pointers {
								pointer.enter(
									get_input_serial(),
									&surface,
									surface_relative_coords.x as f64,
									surface_relative_coords.y as f64,
								);
							}
							inner.pointer_focus = Some(surface.clone());
						}
					} else {
						// The pointer has entered a surface while no other surface is focused, focus this surface
						let surface_client_info_lock = surface_data_lock.client_info.lock().unwrap();
						for pointer in &surface_client_info_lock.pointers {
							pointer.enter(
								get_input_serial(),
								&surface,
								surface_relative_coords.x as f64,
								surface_relative_coords.y as f64,
							);
						}
						inner.pointer_focus = Some(surface.clone());
					}

					// Send the surface the actual motion event
					let client_info_lock = surface_data_lock.client_info.lock().unwrap();
					for pointer in &client_info_lock.pointers {
						pointer.motion(
							get_input_serial(),
							surface_relative_coords.x as f64,
							surface_relative_coords.y as f64,
						);
					}
				} else {
					// The pointer is not over any surface, remove pointer focus from the previous focused surface if any
					if let Some(old_pointer_focus) = inner.pointer_focus.take() {
						let surface_data = old_pointer_focus
							.get_synced::<SurfaceData<G>>();
						let surface_data_lock = surface_data.lock().unwrap();
						let client_info_lock = surface_data_lock.client_info.lock().unwrap();
						for pointer in &client_info_lock.pointers {
							pointer.leave(get_input_serial(), &old_pointer_focus);
						}
					}
				}
			}
			BackendEvent::PointerButton(pointer_button) => {
				let pointer_state = inner.pointer.lock().unwrap();
				let pointer_pos = pointer_state.pos;
				drop(pointer_state);
				let pointer_pos = Point::new(pointer_pos.0.round() as i32, pointer_pos.1.round() as i32);

				if let Some(surface) = inner.window_manager.get_window_under_point(pointer_pos) {
					let surface_data = surface.get_synced::<SurfaceData<G>>();
					let surface_data_lock = surface_data.lock().unwrap();

					if pointer_button.state == wl_pointer::ButtonState::Pressed {
						if let Some(old_keyboard_focus) = inner.keyboard_focus.clone() {
							if surface.as_ref().equals(old_keyboard_focus.as_ref()) {
								// No focus change, this is the same surface
							} else {
								// Change the keyboard focus
								let old_surface_data = old_keyboard_focus
									.get_synced::<SurfaceData<G>>();
								let old_surface_data_lock = old_surface_data.lock().unwrap();
								let old_client_info_lock = old_surface_data_lock.client_info.lock().unwrap();
								for keyboard in &old_client_info_lock.keyboards {
									keyboard.leave(get_input_serial(), &old_keyboard_focus);
								}
								drop(old_client_info_lock);
								drop(old_surface_data_lock);
								let new_client_info_lock = surface_data_lock.client_info.lock().unwrap();
								for keyboard in &new_client_info_lock.keyboards {
									keyboard.modifiers(get_input_serial(), 0, 0, 0, 0);
									keyboard.enter(get_input_serial(), &surface, Vec::new());
								}
								inner.keyboard_focus = Some(surface.clone());
							}
						} else {
							// Focus the keyboard on a window when there was no previously focused window
							let new_client_info_lock = surface_data_lock.client_info.lock().unwrap();
							for keyboard in &new_client_info_lock.keyboards {
								keyboard.modifiers(get_input_serial(), 0, 0, 0, 0);
								keyboard.enter(get_input_serial(), &surface, Vec::new());
							}
							inner.keyboard_focus = Some(surface.clone());
						}
					}
				} else {
					// Remove the keyboard focus from the current focus if empty space is clicked
					if let Some(old_keyboard_focus) = inner.keyboard_focus.take() {
						let old_surface_data = old_keyboard_focus
							.get_synced::<SurfaceData<G>>();
						let old_surface_data_lock = old_surface_data.lock().unwrap();
						let old_client_info_lock = old_surface_data_lock.client_info.lock().unwrap();
						for keyboard in &old_client_info_lock.keyboards {
							keyboard.leave(get_input_serial(), &old_keyboard_focus);
						}
					}
				}

				// Send event to focused window
				if let Some(focused) = inner.keyboard_focus.clone() {
					let surface_data = focused.get_synced::<SurfaceData<G>>();
					let surface_data_lock = surface_data.lock().unwrap();
					let client_info_lock = surface_data_lock.client_info.lock().unwrap();
					for pointer in &client_info_lock.pointers {
						pointer.button(
							pointer_button.serial,
							pointer_button.time,
							pointer_button.button,
							pointer_button.state,
						);
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
		self.setup_seat_global();
		self.setup_data_device_manager_global();
		self.setup_wl_shell_global();
		self.setup_xdg_wm_base_global();
	}

	fn setup_compositor_global(&mut self) {
		let inner = Arc::clone(&self.inner);
		let graphics_backend_state = Arc::clone(&self.graphics_backend_state);
		let compositor_filter = Filter::new(
			move |(main, _num): (Main<wl_compositor::WlCompositor>, u32), _filter, _dispatch_data| {
				let inner = Arc::clone(&inner);
				let graphics_backend_state = Arc::clone(&graphics_backend_state);
				main.quick_assign(move |_main, request, _dispatch_data| {
					let inner = Arc::clone(&inner);
					let graphics_backend_state = Arc::clone(&graphics_backend_state);
					match request {
						wl_compositor::Request::CreateRegion { id } => {
							log::debug!("Got request to create region");
							id.quick_assign(move |_main, request, _| {
								match request {
									wl_region::Request::Destroy => {
										// TODO handle in destructor
									}
									wl_region::Request::Add { x, y, width, height } => {
										log::debug!("Got request to add ({}, {}) {}x{} to region", x, y, width, height);
									}
									wl_region::Request::Subtract { x, y, width, height } => {
										log::debug!(
											"Got request to subtract ({}, {}) {}x{} from region",
											x,
											y,
											width,
											height
										);
									}
									_ => log::warn!("Unknown request for wl_region"),
								}
							});
						}
						wl_compositor::Request::CreateSurface { id } => {
							let graphics_backend_destructor = Arc::clone(&graphics_backend_state);
							let inner_destructor = Arc::clone(&inner);
							let surface = id.clone();
							let surface_resource = surface.as_ref();
							let client_info = inner.lock().unwrap()
								.client_manager
								.get_client_info(surface_resource.client().unwrap());
							let mut graphics_backend_state_lock = graphics_backend_state.lock().unwrap();
							let surface_renderer_data = graphics_backend_state_lock.renderer.create_surface_renderer_data().unwrap();
							let surface_data: Arc<Mutex<SurfaceData<G>>> =
								Arc::new(Mutex::new(SurfaceData::new(client_info, surface_renderer_data)));
							let surface_data_clone = Arc::clone(&surface_data);
							surface_resource
								.user_data()
								.set_threadsafe(move || Arc::clone(&surface_data_clone));
							id.quick_assign(move |_main, request: wl_surface::Request, _| {
								let inner = Arc::clone(&inner);
								let surface_data = Arc::clone(&surface_data);
								match request {
									wl_surface::Request::Destroy => {
										// Handled by destructor
									}
									wl_surface::Request::Attach { buffer, x, y } => {
										log::debug!("Got wl_surface attach request");
										let mut surface_data_lock = surface_data.lock().unwrap();
										// Release the previously attached buffer if it hasn't been committed yet
										if let Some(old_buffer) = surface_data_lock.pending_state.attached_buffer.take()
										{
											if let Some(old_buffer) = old_buffer {
												old_buffer.0.release()
											}
										};
										// Attach the new buffer to the surface
										if let Some(buffer) = buffer {
											surface_data_lock.pending_state.attached_buffer =
												Some(Some((buffer, Point::new(x, y))));
										} else {
											// Attaching a null buffer to a surface is equivalent to unmapping it.
											surface_data_lock.pending_state.attached_buffer = Some(None);
										}
									}
									wl_surface::Request::Damage { .. } => {
										log::debug!("Got wl_surface damage request");
									}
									wl_surface::Request::Frame { callback } => {
										let mut surface_data_lock = surface_data.lock().unwrap();
										if let Some(_old_callback) =
											surface_data_lock.callback.replace((*callback).clone())
										{
											log::warn!("Replacing surface callback with a newly requested one, unclear if this is intended behavior");
										}
										log::debug!("Got wl_surface frame request");
									}
									wl_surface::Request::SetOpaqueRegion { .. } => {
										log::debug!("Got wl_surface set_opaque_region request");
									}
									wl_surface::Request::SetInputRegion { .. } => {
										log::debug!("Got wl_surface set_input_region request");
									}
									wl_surface::Request::Commit => {
										// TODO: relying on the impl of ShmBuffer to ascertain the size of the buffer is probably unsound if the ShmBuffer impl lies.
										// So that trait should either be unsafe, or Shm should be moved out of the Rendering backend and EasyShm should be made canonical
										log::debug!("Got wl_surface commit request");
										let mut surface_data_lock = surface_data.lock().unwrap();
										surface_data_lock.commit_pending_state();
										if let Some(ref committed_buffer) = surface_data_lock.committed_buffer {
											let buffer_data = committed_buffer.0.get_synced::<G::ShmBuffer>();
											let buffer_data_lock = buffer_data.lock().unwrap();
											let new_size =
												Size::new(buffer_data_lock.width(), buffer_data_lock.height());
											drop(buffer_data_lock);
											drop(surface_data_lock);
											let mut inner_lock = inner.lock().unwrap();
											inner_lock
												.window_manager
												.manager_impl
												.handle_surface_resize((*surface).clone(), new_size);
										}
									}
									wl_surface::Request::SetBufferTransform { .. } => {
										log::debug!("Got wl_surface set_buffer_transform request");
									}
									wl_surface::Request::SetBufferScale { .. } => {
										log::debug!("Got wl_surface set_buffer_scale request");
									}
									wl_surface::Request::DamageBuffer { .. } => {
										log::debug!("Got wl_surface damage_buffer request");
									}
									_ => {
										log::warn!("Got unknown request for wl_surface");
									}
								}
							});
							id.assign_destructor(Filter::new(
								move |surface: wl_surface::WlSurface, _filter, _dispatch_data| {
									log::debug!("Got wl_surface destroy request");
									let mut graphics_backend_state_lock = graphics_backend_destructor.lock().unwrap();
									let surface_data = surface.get_synced::<SurfaceData<G>>();
									graphics_backend_state_lock
										.renderer
										.destroy_surface_renderer_data(
											surface_data.lock().unwrap().renderer_data.take().unwrap(),
										)
										.map_err(|e| log::error!("Failed to destroy surface: {}", e))
										.unwrap();
									let mut inner = inner_destructor.lock().unwrap();
									inner.trim_dead_clients();
								},
							));
							log::debug!("Got request to create surface");
						}
						_ => {
							log::warn!("Got unknown request for wl_compositor");
						}
					}
				});
			},
		);
		self.display
			.create_global::<wl_compositor::WlCompositor, _>(4, compositor_filter);
	}

	fn setup_data_device_manager_global(&mut self) {
		let data_device_manager_filter = Filter::new(
			|(main, _num): (Main<wl_data_device_manager::WlDataDeviceManager>, u32), _filter, _dispatch_data| {
				main.quick_assign(
					|_main, request: wl_data_device_manager::Request, _dispatch_data| match request {
						wl_data_device_manager::Request::CreateDataSource { id: _ } => {
							log::debug!("Got create_data_source request for wl_data_device_manager");
						}
						wl_data_device_manager::Request::GetDataDevice { id: _, seat: _ } => {
							log::debug!("Got get_data_device request for wl_data_device_manager");
						}
						_ => {
							log::warn!("Got unknown request for wl_data_device_manager");
						}
					},
				)
			},
		);
		self.display
			.create_global::<wl_data_device_manager::WlDataDeviceManager, _>(3, data_device_manager_filter);
	}
}

impl<I: InputBackend, G: GraphicsBackend> Drop for Compositor<I, G> {
	fn drop(&mut self) {
		log::trace!("Closing wayland socket");
		fs::remove_file("/run/user/1000/wayland-0").unwrap();
	}
}

#[derive(Debug, Error)]
pub enum CompositorError<G: GraphicsBackend + 'static> {
	#[error("There was an error creating a wayland socket")]
	SocketError(#[source] io::Error),
	#[error("Failed to create a render target")]
	RenderTargetError(#[source] G::Error),
}
