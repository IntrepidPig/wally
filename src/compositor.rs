use std::{
	fmt,
	fs::{self},
	io::{self},
	marker::PhantomData,
	time::{Duration, Instant},
};

use calloop::{
	mio,
	signals::{Signal, Signals},
	EventLoop, LoopHandle, Source,
};
use std::sync::{Arc, Mutex};
use thiserror::Error;
use wayland_server::{protocol::*, Client, Display, Filter, Main};

use crate::{
	backend::{BackendEvent, InputBackend, MergedBackend, RenderBackend},
	compositor::prelude::*,
	compositor::{
		surface::{SurfaceData, SurfaceTree},
	},
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

	pub use crate::{
		backend::{BackendEvent, InputBackend, RenderBackend},
		compositor::{
			client::ClientInfo,
			role::Role,
			surface::{SurfaceData, SurfaceTree},
			PointerState, Synced,
		},
	};
}

pub type Synced<T> = Arc<Mutex<T>>;

static INPUT_SERIAL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

pub fn get_input_serial() -> u32 {
	INPUT_SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

pub struct Compositor<I: InputBackend, R: RenderBackend> {
	display: Display,
	inner: Arc<Mutex<CompositorInner<I, R>>>,
	pub(crate) backend: Arc<Mutex<MergedBackend<I, R>>>,
	_signal_event_source: Source<Signals>,
	_idle_event_source: calloop::Idle,
	_display_event_source: calloop::Source<calloop::generic::Generic<calloop::generic::EventedRawFd>>,
	_input_event_source: calloop::Source<calloop::channel::Channel<BackendEvent>>,
}

pub struct CompositorInner<I: InputBackend, R: RenderBackend> {
	running: bool,
	pub client_manager: ClientManager,
	pub surface_tree: SurfaceTree<R>,
	pub pointer: Arc<Mutex<PointerState>>,
	pub focused: Option<wl_surface::WlSurface>,
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
	pub hotspot_x: i32,
	pub hotspot_y: i32,
}

impl fmt::Debug for CustomCursor {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct("CustomCursor")
			.field("surface", &"<WlSurface>")
			.field("hotspot_x", &self.hotspot_x)
			.field("hotspot_y", &self.hotspot_y)
			.finish()
	}
}

impl<I: InputBackend, R: RenderBackend> CompositorInner<I, R> {
	fn trim_dead_clients(&mut self) {
		self.surface_tree.surfaces.retain(|surface| {
			log::debug!("Checking surface");
			if !surface.as_ref().is_alive() {
				log::debug!("Destroying surface");
				false
			} else {
				true
			}
		})
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

impl<I: InputBackend + 'static, R: RenderBackend + 'static> Compositor<I, R> {
	pub fn new(
		input_backend: I,
		render_backend: R,
		event_loop_handle: LoopHandle<Compositor<I, R>>,
	) -> Result<Self, CompositorError> {
		let mut backend = crate::backend::create_backend(input_backend, render_backend);
		let mut display = Display::new();
		//let f = fs::File::create("/run/user/1000/wayland-0").unwrap();
		display
			.add_socket::<&str>(None)
			.map_err(|e| CompositorError::SocketError(e))?;

		let signals = Signals::new(&[Signal::SIGINT]).expect("Failed to setup signal handler");
		let signal_event_source = event_loop_handle
			.insert_source(
				signals,
				|_event: calloop::signals::Event, compositor: &mut Compositor<I, R>| {
					log::info!("Received sigint, exiting");
					let mut inner = compositor.inner.lock().unwrap();
					inner.running = false;
				},
			)
			.expect("Failed to insert signal handler in event loop");

		let idle_event_source = event_loop_handle.insert_idle(|_wally: &mut Compositor<I, R>| {
			log::trace!("Finished processing events");
		});

		let mut display_events = calloop::generic::Generic::from_raw_fd(display.get_poll_fd());
		display_events.set_interest(mio::Ready::readable());
		display_events.set_pollopts(mio::PollOpt::edge());
		let display_event_source = event_loop_handle
			.insert_source(
				display_events,
				|_event: calloop::generic::Event<calloop::generic::EventedRawFd>, compositor: &mut Compositor<I, R>| {
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

		let input_events = backend.input_backend.get_event_source();
		let input_event_source = event_loop_handle
			.insert_source(
				input_events,
				|e: calloop::channel::Event<BackendEvent>, compositor: &mut Compositor<I, R>| {
					if let calloop::channel::Event::Msg(event) = e {
						log::trace!("Got input event");
						compositor.handle_input_event(event);
					}
				},
			)
			.expect("Failed to insert input event source");

		let client_manager = ClientManager::new();

		let pointer_state = Arc::new(Mutex::new(PointerState {
			pos: (10.0, 10.0),
			sensitivity: 0.5,
			custom_cursor: None,
		}));

		let inner = CompositorInner {
			running: true,
			client_manager,
			surface_tree: SurfaceTree::new(Arc::clone(&pointer_state)),
			pointer: pointer_state,
			focused: None,
			phantom: PhantomData,
		};

		Ok(Self {
			display,
			inner: Arc::new(Mutex::new(inner)),
			backend: Arc::new(Mutex::new(backend)),
			_signal_event_source: signal_event_source,
			_idle_event_source: idle_event_source,
			_display_event_source: display_event_source,
			_input_event_source: input_event_source,
		})
	}

	pub fn print_debug_info(&self) {
		let inner = self.inner.lock().unwrap();
		println!("Surfaces:");
		for (i, surface) in inner.surface_tree.surfaces.iter().enumerate() {
			println!("\tSurface@{} {}", surface.as_ref().id(), i);
			if let Some(surface_data_ref) = surface.as_ref().user_data().get::<Arc<Mutex<SurfaceData<R>>>>() {
				let surface_data_lock = surface_data_ref.lock().unwrap();
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
	}

	pub fn start(&mut self, event_loop: &mut EventLoop<Compositor<I, R>>) {
		while self.inner.lock().unwrap().running {
			let start = Instant::now();
			{
				let mut inner = self.inner.lock().unwrap();
				let input_update_start = Instant::now();
				let mut backend = self.backend.lock().unwrap();
				backend
					.input_backend
					.update()
					.map_err(|_e| log::error!("Error updating the input backend"))
					.unwrap();
				println!(
					"Updated input backend in {} ms",
					input_update_start.elapsed().as_secs_f64() * 1000.0
				);
				let render_update_start = Instant::now();
				backend
					.render_backend
					.update()
					.map_err(|_e| log::error!("Error updating the render backend"))
					.unwrap();
				println!(
					"Updated render backend in {} ms",
					render_update_start.elapsed().as_secs_f64() * 1000.0
				);
				let inner = &mut *inner;
				let render_tree_start = Instant::now();
				let surface_tree = &inner.surface_tree;
				backend
					.render_backend
					.render_tree(surface_tree)
					.map_err(|_e| log::error!("Error rendering surface tree"))
					.unwrap();
				println!(
					"Rendered surface tree in {} ms",
					render_tree_start.elapsed().as_secs_f64() * 1000.0
				);
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
			println!(
				"Dispatched events in {} ms",
				dispatch_start.elapsed().as_secs_f64() * 1000.0
			);
			let flush_start = Instant::now();
			self.display.flush_clients(&mut ());
			println!("Flushed clients in {} ms", flush_start.elapsed().as_secs_f64() * 1000.0);
			//self.print_debug_info();
			let end = start.elapsed();
			println!("Ran frame in {} ms", end.as_secs_f64() * 1000.0);
		}
	}

	pub fn handle_input_event(&mut self, event: BackendEvent) {
		println!("Got input: {:?}", event);
		let mut inner = self.inner.lock().unwrap();
		let backend = self.backend.lock().unwrap();
		match event {
			BackendEvent::StopRequested => {
				inner.running = false;
			}
			BackendEvent::KeyPress(key_press) => {
				let inner = &mut *inner;
				if let Some(focused) = inner.focused.clone() {
					let surface_data_lock = focused
						.as_ref()
						.user_data()
						.get::<Synced<SurfaceData<R::ObjectHandle>>>()
						.unwrap()
						.lock()
						.unwrap();
					let client_info_lock = surface_data_lock.client_info.lock().unwrap();
					for keyboard in &client_info_lock.keyboards {
						keyboard.key(key_press.serial, key_press.time, key_press.key, key_press.state);
					}
				}
			}
			BackendEvent::PointerMotion(pointer_motion) => {
				let mut pointer_state = inner.pointer.lock().unwrap();
				pointer_state.pos.0 += pointer_motion.dx_unaccelerated * pointer_state.sensitivity;
				pointer_state.pos.1 += pointer_motion.dy_unaccelerated * pointer_state.sensitivity;
				let size = backend.render_backend.get_size();
				if pointer_state.pos.0 < 0.0 {
					pointer_state.pos.0 = 0.0;
				}
				if pointer_state.pos.1 < 0.0 {
					pointer_state.pos.1 = 0.0;
				}
				if pointer_state.pos.0 > size.0 as f64 {
					pointer_state.pos.0 = size.0 as f64;
				}
				if pointer_state.pos.1 > size.1 as f64 {
					pointer_state.pos.1 = size.1 as f64;
				}
			}
			BackendEvent::PointerButton(pointer_button) => {
				// Handle focus changes
				let pointer_state = inner.pointer.lock().unwrap();
				let pointer_pos = pointer_state.pos;
				drop(pointer_state);
				if let Some(surface) = inner
					.surface_tree
					.get_surface_under_point((pointer_pos.0 as i32, pointer_pos.1 as i32))
				{
					if let Some(old_focused) = inner.focused.take() {
						if surface.as_ref().equals(old_focused.as_ref()) {
							// No focus change, this is the same surface
						} else {
							// Unfocus the previously focused surface
							let surface_data = surface
								.as_ref()
								.user_data()
								.get::<Arc<Mutex<SurfaceData<R::ObjectHandle>>>>()
								.unwrap();
							let surface_data_lock = surface_data.lock().unwrap();
							let client_info_lock = surface_data_lock.client_info.lock().unwrap();
							for keyboard in &client_info_lock.keyboards {
								keyboard.leave(get_input_serial(), &old_focused);
							}
							for pointer in &client_info_lock.pointers {
								pointer.leave(get_input_serial(), &old_focused);
							}
						}
						// Focus the new surface
						let surface_data = surface
							.as_ref()
							.user_data()
							.get::<Arc<Mutex<SurfaceData<R::ObjectHandle>>>>()
							.unwrap();
						let surface_data_lock = surface_data.lock().unwrap();
						let client_info_lock = surface_data_lock.client_info.lock().unwrap();
						for keyboard in &client_info_lock.keyboards {
							keyboard.enter(get_input_serial(), &surface, Vec::new());
						}
						for pointer in &client_info_lock.pointers {
							pointer.enter(get_input_serial(), &surface, 0.0, 0.0);
						}
						inner.focused = Some(surface.clone());
					}
				}

				// Send event to focused window
				if let Some(focused) = inner.focused.clone() {
					let surface_data = focused
						.as_ref()
						.user_data()
						.get::<Arc<Mutex<SurfaceData<R::ObjectHandle>>>>()
						.unwrap();
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
		let backend = Arc::clone(&self.backend);
		let compositor_filter = Filter::new(
			move |(main, _num): (Main<wl_compositor::WlCompositor>, u32), _filter, _dispatch_data| {
				let inner = Arc::clone(&inner);
				let backend = Arc::clone(&backend);
				main.quick_assign(move |_main, request, _dispatch_data| {
					let inner = Arc::clone(&inner);
					let backend = Arc::clone(&backend);
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
							let backend_destructor = Arc::clone(&backend);
							let inner_destructor = Arc::clone(&inner);
							let surface = &*id;
							let mut inner_lock = inner.lock().unwrap();
							inner_lock.surface_tree.add_surface(surface.clone());
							let surface_resource = surface.as_ref();
							//let surface_data_args = Arc::new(Mutex::new(SurfaceData::new(None)));
							let renderer_surface = backend
								.lock()
								.unwrap()
								.render_backend
								.create_object()
								.map_err(|e| log::error!("Failed to create backend surface: {}", e))
								.unwrap();
							let client_info = inner_lock
								.client_manager
								.get_client_info(surface_resource.client().unwrap());
							drop(inner_lock);
							let surface_data: Arc<Mutex<SurfaceData<R::ObjectHandle>>> =
								Arc::new(Mutex::new(SurfaceData::new(client_info, Some(renderer_surface))));
							let surface_data_clone = Arc::clone(&surface_data);
							surface_resource
								.user_data()
								.set_threadsafe(move || Arc::clone(&surface_data_clone));
							id.quick_assign(move |_main, request: wl_surface::Request, _| {
								let surface_data = Arc::clone(&surface_data);
								match request {
									wl_surface::Request::Destroy => {
										// Handled by destructor
									}
									wl_surface::Request::Attach { buffer, x, y } => {
										log::debug!("Got wl_surface attach request");
										let mut surface_data_lock = surface_data.lock().unwrap();
										// Release the previously attached buffer if it hasn't been committed yet
										if let Some(old_buffer) = surface_data_lock.attached_buffer.take() {
											old_buffer.0.release()
										};
										if let Some(buffer) = buffer {
											surface_data_lock.attached_buffer = Some((buffer, (x, y)))
										} else {
											// Attaching a null buffer to a surface is equivalent to unmapping it.
											surface_data_lock.draw = false;
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
										log::debug!("Got wl_surface commit request");
										let mut surface_data_lock = surface_data.lock().unwrap();
										// Move the previously attached buffer to the committed buffer state ("commit the buffer")
										if let Some(attached_buffer) = surface_data_lock.attached_buffer.take() {
											// Release the previously committed buffer if it's still there (i.e. it hasn't been drawn/rendered/copied to GPU yet)
											if let Some(old_buffer) =
												surface_data_lock.committed_buffer.replace(attached_buffer)
											{
												old_buffer.0.release();
											}
											surface_data_lock.draw = true;
										} else {
											log::warn!("A surface was committed without a previously attached buffer");
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
									let mut backend = backend_destructor.lock().unwrap();
									let surface_data = surface
										.as_ref()
										.user_data()
										.get::<Arc<Mutex<SurfaceData<R::ObjectHandle>>>>()
										.unwrap();
									backend
										.render_backend
										.destroy_object(surface_data.lock().unwrap().renderer_data.take().unwrap())
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
						wl_data_device_manager::Request::CreateDataSource { id } => {
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

impl<I: InputBackend, R: RenderBackend> Drop for Compositor<I, R> {
	fn drop(&mut self) {
		log::trace!("Closing wayland socket");
		fs::remove_file("/run/user/1000/wayland-0").unwrap();
	}
}

#[derive(Debug, Error)]
pub enum CompositorError {
	#[error("There was an error creating a wayland socket")]
	SocketError(#[source] io::Error),
}
