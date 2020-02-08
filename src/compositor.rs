use std::{
	cell::RefCell,
	fs::{self},
	io::{self},
	rc::Rc,
	time::Duration,
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
	compositor::{
		role::{Role},
	},
	backend::{Backend, BackendEvent}
};
use std::time::Instant;

pub mod seat;
pub mod shell;
pub mod shm;
pub mod xdg;
pub mod role;

static INPUT_SERIAL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

pub fn get_input_serial() -> u32 {
	INPUT_SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

pub struct Compositor<B: Backend + 'static> {
	display: Display,
	inner: Arc<Mutex<CompositorInner>>,
	pub(crate) backend: Arc<Mutex<B>>,
	signal_event_source: Source<Signals>,
	idle_event_source: calloop::Idle,
	display_event_source: calloop::Source<calloop::generic::Generic<calloop::generic::EventedRawFd>>,
}

pub struct CompositorInner {
	running: bool,
	pub client_manager: Rc<RefCell<ClientManager>>,
	pub surface_tree: SurfaceTree,
}

impl CompositorInner {
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

pub struct SurfaceTree {
	surfaces: Vec<wl_surface::WlSurface>,
}

impl SurfaceTree {
	pub fn new() -> Self {
		Self { surfaces: Vec::new() }
	}

	pub fn add_surface(&mut self, surface: wl_surface::WlSurface) {
		self.surfaces.push(surface);
	}

	pub fn surfaces_ascending(&self) -> impl Iterator<Item = &wl_surface::WlSurface> {
		self.surfaces.iter()
	}

	pub fn surfaces_descending(&self) -> impl Iterator<Item = &wl_surface::WlSurface> {
		self.surfaces_ascending().collect::<Vec<_>>().into_iter().rev()
	}
	
	pub fn destroy_surface<B: Backend>(&mut self, surface: wl_surface::WlSurface) {
		// This bit right here doesn't work because dead surfaces lose their ids
		if let Some(i) = self.surfaces.iter().enumerate().find(|(i, test_surface)| dbg!(**test_surface == surface)).map(|x| x.0) {
			let surface = self.surfaces.remove(i);
			let surface_data = surface.as_ref().user_data().get::<Arc<Mutex<SurfaceData<B::SurfaceData>>>>().unwrap();
			let mut surface_data_lock = surface_data.lock().unwrap();
			surface_data_lock.destroy();
		}
	}
}

pub struct ClientManager {
	pub client_resources: Vec<ClientResources>,
}

impl ClientManager {
	pub fn new() -> Self {
		Self {
			client_resources: Vec::new(),
		}
	}

	pub fn get_client_resources_mut(&mut self, client: Client) -> &mut ClientResources {
		// This is written weirdly to bypass borrow checker issues
		if self.client_resources.iter().any(|r| r.client.equals(&client)) {
			self.client_resources
				.iter_mut()
				.find(|r| r.client.equals(&client))
				.unwrap()
		} else {
			self.client_resources.push(ClientResources {
				client,
				keyboard: None,
				pointer: None,
			});
			self.client_resources.last_mut().unwrap()
		}
	}
}

pub struct ClientResources {
	pub client: Client,
	pub keyboard: Option<wl_keyboard::WlKeyboard>,
	pub pointer: Option<wl_pointer::WlPointer>,
}

pub struct SurfaceData<T> {
	pub attached_buffer: Option<(wl_buffer::WlBuffer, (i32, i32))>,
	pub committed_buffer: Option<(wl_buffer::WlBuffer, (i32, i32))>,
	pub callback: Option<wl_callback::WlCallback>,
	pub role: Option<Role>,
	pub renderer_data: T,
}

impl<T> SurfaceData<T> {
	pub fn new(renderer_data: T) -> Self {
		Self {
			attached_buffer: None,
			committed_buffer: None,
			callback: None,
			role: None,
			renderer_data,
		}
	}

	pub fn replace_renderer_data<U>(&self, new_data: U) -> SurfaceData<U> {
		SurfaceData {
			attached_buffer: self.attached_buffer.clone(),
			committed_buffer: self.committed_buffer.clone(),
			callback: self.callback.clone(),
			role: self.role.clone(),
			renderer_data: new_data,
		}
	}
	
	pub fn destroy(&mut self) {
		if let Some((buffer, _)) = self.attached_buffer.take() {
			buffer.release();
		}
		if let Some((buffer, _)) = self.committed_buffer.take() {
			buffer.release();
		}
		if let Some(mut role) = self.role.take() {
			role.destroy();
		}
	}
}

impl<B: Backend + 'static> Compositor<B> {
	pub fn new(backend: B, event_loop_handle: LoopHandle<Compositor<B>>) -> Result<Self, CompositorError> {
		let mut backend = backend;
		let mut display = Display::new();
		//let f = fs::File::create("/run/user/1000/wayland-0").unwrap();
		display
			.add_socket::<&str>(None)
			.map_err(|e| CompositorError::SocketError(e))?;

		let signals = Signals::new(&[Signal::SIGINT]).expect("Failed to setup signal handler");
		let signal_event_source = event_loop_handle
			.insert_source(signals, |e: calloop::signals::Event, compositor: &mut Compositor<B>| {
				log::info!("Received sigint, exiting");
				let mut inner = compositor.inner.lock().unwrap();
				inner.running = false;
			})
			.expect("Failed to insert signal handler in event loop");

		let idle_event_source = event_loop_handle.insert_idle(|wally: &mut Compositor<B>| {
			log::trace!("Finished processing events");
		});

		let mut display_events = calloop::generic::Generic::from_raw_fd(display.get_poll_fd());
		display_events.set_interest(mio::Ready::readable());
		display_events.set_pollopts(mio::PollOpt::edge());
		let display_event_source = event_loop_handle
			.insert_source(
				display_events,
				|e: calloop::generic::Event<calloop::generic::EventedRawFd>, compositor: &mut Compositor<B>| {
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

		let input_events = backend.get_event_source();
		let input_event_source = event_loop_handle
			.insert_source(
				input_events,
				|e: calloop::channel::Event<BackendEvent>, compositor: &mut Compositor<B>| {
					if let calloop::channel::Event::Msg(event) = e {
						log::trace!("Got input event");
						compositor.handle_input_event(event);
					}
				},
			)
			.expect("Failed to insert input event source");

		let client_manager = Rc::new(RefCell::new(ClientManager::new()));

		let inner = CompositorInner {
			running: true,
			client_manager,
			surface_tree: SurfaceTree::new(),
		};

		Ok(Self {
			display,
			inner: Arc::new(Mutex::new(inner)),
			backend: Arc::new(Mutex::new(backend)),
			signal_event_source,
			idle_event_source,
			display_event_source,
		})
	}
	
	pub fn print_debug_info(&self) {
		let inner = self.inner.lock().unwrap();
		let backend = self.backend.lock().unwrap();
		println!("Surfaces:");
		for (i, surface) in inner.surface_tree.surfaces.iter().enumerate() {
			println!("\tSurface@{} {}", surface.as_ref().id(), i);
			if let Some(surface_data_ref) = surface.as_ref().user_data().get::<Arc<Mutex<SurfaceData<B::SurfaceData>>>>() {
				let surface_data_lock = surface_data_ref.lock().unwrap();
				if let Some(role) = surface_data_lock.role.as_ref() {
					println!("\t\tRole: {:?}", role);
				} else {
					println!("\t\tRole: None");
				}
				println!("\t\tAlive: {}", surface.as_ref().is_alive());
				println!("\t\tClient: {}", surface.as_ref().client().map(|client| if client.alive() { "Alive client" } else { "Dead client" }).unwrap_or("No client"));
			}
		}
		
	}

	pub fn start(&mut self, event_loop: &mut EventLoop<Compositor<B>>) {
		while self.inner.lock().unwrap().running {
			let start = Instant::now();
			{
				let mut inner = self.inner.lock().unwrap();
				let mut backend = self.backend.lock().unwrap();
				backend
					.update_input_backend()
					.map_err(|_e| log::error!("Error updating the input backend"))
					.unwrap();
				backend
					.update_render_backend()
					.map_err(|_e| log::error!("Error updating the render backend"))
					.unwrap();
				let inner = &mut *inner;
				let surface_tree = &inner.surface_tree;
				backend
					.render_tree(surface_tree)
					.map_err(|_e| log::error!("Error rendering surface tree"))
					.unwrap();
			}
			// TODO change timeout to something that syncs with rendering somehow
			match event_loop.dispatch(Some(Duration::from_millis(16)), self) {
				Ok(_) => {}
				Err(e) => {
					log::error!("An error occurred in the event loop: {}", e);
				}
			}
			self.display.flush_clients(&mut ());
			//self.print_debug_info();
			let end = start.elapsed();
			//println!("Ran frame in {} ms", end.as_secs_f64() * 1000.0);
		}
	}

	pub fn handle_input_event(&mut self, event: BackendEvent) {
		println!("Got input: {:?}", event);
		let mut inner = self.inner.lock().unwrap();
		match event {
			BackendEvent::StopRequested => {
				inner.running = false;
			}
			BackendEvent::KeyPress(key_press) => {
				let inner = &mut *inner;
				let client_manager = &mut inner.client_manager;
				let surface_tree = &inner.surface_tree;
				for client_resources in &mut client_manager.borrow_mut().client_resources {
					log::debug!("Found client resources");
					if let Some(keyboard) = client_resources.keyboard.as_ref() {
						log::debug!("Sending key event");
						for surface in surface_tree.surfaces_descending() {
							let surface_resource = surface.as_ref();
							let keyboard_resource = keyboard.as_ref();
							if surface_resource
								.client()
								.unwrap()
								.equals(&keyboard_resource.client().unwrap())
							{
								keyboard.enter(get_input_serial(), surface, Vec::new());
								break;
							}
						}
						keyboard.key(key_press.serial, key_press.time, key_press.key, key_press.state);
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
			move |(main, _num): (Main<wl_compositor::WlCompositor>, u32), filter, _dispatch_data| {
				let inner = Arc::clone(&inner);
				let backend = Arc::clone(&backend);
				main.quick_assign(move |main, request, _dispatch_data| {
					let inner = Arc::clone(&inner);
					let backend = Arc::clone(&backend);
					match request {
						wl_compositor::Request::CreateRegion { id } => {
							log::debug!("Got request to create region");
							id.quick_assign(move |main, request, _| {
								let inner = Arc::clone(&inner);
								let backend = Arc::clone(&backend);
								match request {
									wl_region::Request::Destroy => {
										log::debug!("Got request to destroy region");
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
							{
								let mut inner = inner.lock().unwrap();
								inner.surface_tree.add_surface(surface.clone());
							}
							let surface_resource = surface.as_ref();
							let surface_data_args = Arc::new(Mutex::new(SurfaceData::new(())));
							let renderer_surface = backend
								.lock()
								.unwrap()
								.create_surface(surface.clone())
								.map_err(|e| log::error!("Failed to create backend surface"))
								.unwrap();
							let surface_data = Arc::new(Mutex::new(SurfaceData::new(renderer_surface)));
							let surface_data_clone = Arc::clone(&surface_data);
							surface_resource
								.user_data()
								.set_threadsafe(move || Arc::clone(&surface_data_clone));
							id.quick_assign(move |main, request: wl_surface::Request, _| {
								let inner = Arc::clone(&inner);
								let backend = Arc::clone(&backend);
								let surface_data = Arc::clone(&surface_data);
								match request {
									wl_surface::Request::Destroy => {
										// Handled by destructor
									}
									wl_surface::Request::Attach { buffer, x, y } => {
										log::debug!("Got wl_surface attach request");
										let mut surface_data_lock = surface_data.lock().unwrap();
										// I guess it's possible for a client to attach a null buffer to a surface?
										if let Some(buffer) = buffer {
											// Release the previously attached buffer if it hasn't been committed yet
											if let Some(old_buffer) =
												surface_data_lock.attached_buffer.replace((buffer, (x, y)))
											{
												old_buffer.0.release()
											};
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
							id.assign_destructor(Filter::new(move |surface: wl_surface::WlSurface, _filter, _dispatch_data| {
								log::debug!("Got wl_surface destroy request");
								let mut backend = backend_destructor.lock().unwrap();
								backend
									.destroy_surface(surface.clone())
									.map_err(|e| log::error!("Failed to destroy surface"))
									.unwrap();
								let mut inner = inner_destructor.lock().unwrap();
								inner.trim_dead_clients();
							}));
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
			|(main, _num): (Main<wl_data_device_manager::WlDataDeviceManager>, u32), filter, _dispatch_data| {
				main.quick_assign(
					|main, request: wl_data_device_manager::Request, _dispatch_data| match request {
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

impl<B: Backend> Drop for Compositor<B> {
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
