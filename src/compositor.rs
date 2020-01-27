use std::{
	time::{Duration},
	fs::{self},
	env::{self},
	rc::{Rc, Weak},
	io::{self},
	os::{
		unix::io::FromRawFd,
	},
	path::{Path},
	cell::{RefCell},
};

use wayland_server::{Display, Client, Resource, Main, protocol::{
	*,
}, Filter};
use wayland_protocols::{
	xdg_shell::server::{
		xdg_wm_base, xdg_surface, xdg_toplevel,
	},
};
use calloop::{
	EventLoop, LoopHandle, Source,
	signals::{Signal, Signals},
	channel::{Sender, Channel},
};
use thiserror::{Error};
use wayland_server::protocol::wl_surface::Request;
use std::ops::Deref;
use calloop::channel::Event;
use crate::backend::{Backend, BackendEvent};

pub struct Compositor<B: Backend + 'static> {
	running: bool,
	pub display: Display,
	signal_event_source: Source<Signals>,
	idle_event_source: calloop::Idle,
	shm_mmap: Option<B::ShmPool>,
	internal_event_sender: Sender<CompositorEvent<B>>,
	internal_event_source: Source<Channel<CompositorEvent<B>>>,
	pub client_manager: Rc<RefCell<ClientManager>>,
	pub backend: Rc<RefCell<B>>,
	shm_filter: Filter<wl_shm::Request>,
	tmp: i32,
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
			self.client_resources.iter_mut().find(|r| r.client.equals(&client)).unwrap()
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

enum CompositorEvent<B: Backend + 'static> {
	NewShm(B::ShmPool),
}

impl<B: Backend + 'static> Compositor<B> {
	pub fn new(backend: B, event_loop_handle: LoopHandle<Compositor<B>>) -> Result<Self, CompositorError> {
		let mut display = Display::new();
		//let f = fs::File::create("/run/user/1000/wayland-0").unwrap();
		display.add_socket::<&str>(None).map_err(|e| CompositorError::SocketError(e))?;
		
		let signals = calloop::signals::Signals::new(&[calloop::signals::Signal::SIGINT]).expect("Failed to setup signal handler");
		let signal_event_source = event_loop_handle.insert_source(signals, |e: calloop::signals::Event, compositor: &mut Compositor<B>| {
			log::info!("Received sigint, exiting");
			compositor.running = false;
		}).expect("Failed to insert signal handler in event loop");
		
		let idle_event_source = event_loop_handle.insert_idle(|wally: &mut Compositor<B>| {
			log::trace!("Finished processing events");
		});
		
		let (internal_event_sender, rx) = calloop::channel::channel();
		let internal_event_source = event_loop_handle.insert_source(rx, |event, compositor| {
			match event {
				calloop::channel::Event::Msg(CompositorEvent::NewShm(buf)) => {
					compositor.shm_mmap = Some(buf);
				},
				calloop::channel::Event::Closed => {},
			}
		}).expect("Failed to insert internal compositor message handler in event loop");
		
		let client_manager = Rc::new(RefCell::new(ClientManager::new()));
		
		Ok(Self {
			running: true,
			display,
			signal_event_source,
			idle_event_source,
			shm_mmap: None,
			internal_event_sender,
			internal_event_source,
			client_manager,
			backend: Rc::new(RefCell::new(backend)),
			shm_filter: Filter::new(|_, _| {}),
			tmp: 0,
		})
	}
	
	pub fn start(&mut self, event_loop: &mut EventLoop<Compositor<B>>) {
		while self.running {
			self.display.dispatch(Duration::from_millis(16)).expect("Failed to dispatch display events");
			match event_loop.dispatch(Some(Duration::from_millis(16)), self) {
				Ok(_) => {},
				Err(e) => {
					log::error!("An error occurred in the event loop: {}", e);
				}
			}
			self.display.flush_clients();
			self.backend.borrow_mut().update_input_backend().map_err(|_e| log::error!("Error updating the input backend")).unwrap();
			self.backend.borrow_mut().update_render_backend().map_err(|_e| log::error!("Error updating the render backend")).unwrap();
		}
	}
	
	pub fn handle_input_event(&mut self, event: BackendEvent) {
		println!("Got input: {:?}", event);
	}
	
	pub fn init(&mut self) {
		self.setup_globals();
	}
	
	pub fn setup_globals(&mut self) {
		self.setup_compositor_global();
		self.setup_shm_global();
		self.setup_seat_global();
		self.setup_data_device_manager_global();
		//self.setup_wl_shell_global();
		self.setup_xdg_wm_base_global();
	}
	
	fn setup_compositor_global(&mut self) {
		self.display.create_global::<wl_compositor::WlCompositor, _>(4, |main, version| {
			main.assign_mono(|main, request: wl_compositor::Request| {
				match request {
					wl_compositor::Request::CreateRegion {
						id
					} => {
						log::debug!("Got request to create region");
					},
					wl_compositor::Request::CreateSurface {
						id
					} => {
						id.assign_mono(|main, request: wl_surface::Request| {
							match request {
								wl_surface::Request::Destroy => {
									log::debug!("Got wl_surface destroy request");
								},
								wl_surface::Request::Attach { .. } => {
									log::debug!("Got wl_surface attach request");
								},
								wl_surface::Request::Damage { .. } => {
									log::debug!("Got wl_surface damage request");
								},
								wl_surface::Request::Frame { .. } => {
									log::debug!("Got wl_surface frame request");
								},
								wl_surface::Request::SetOpaqueRegion { .. } => {
									log::debug!("Got wl_surface set_opaque_region request");
								},
								wl_surface::Request::SetInputRegion { .. } => {
									log::debug!("Got wl_surface set_input_region request");
								},
								wl_surface::Request::Commit => {
									log::debug!("Got wl_surface commit request");
								},
								wl_surface::Request::SetBufferTransform { .. } => {
									log::debug!("Got wl_surface set_buffer_transform request");
								},
								wl_surface::Request::SetBufferScale { .. } => {
									log::debug!("Got wl_surface set_buffer_scale request");
								},
								wl_surface::Request::DamageBuffer { .. } => {
									log::debug!("Got wl_surface damage_buffer request");
								},
								_ => {
									log::warn!("Got unknown request for wl_surface");
								}
							}
						});
						log::debug!("Got request to create surface");
					},
					_ => {
						log::warn!("Got unknown request for wl_compositor");
					},
				}
			});
		});
	}
	
	fn setup_shm_global(&mut self) {
		let sender = self.internal_event_sender.clone();
		self.display.create_global::<wl_shm::WlShm, _>(1, move |main, version| {
			let sender = sender.clone();
			//main.deref().format(wl_shm::Format::Bgra8888);
			//main.deref().format(wl_shm::Format::Argb8888);
			main.assign_mono(move |main: Main<wl_shm::WlShm>, request: wl_shm::Request| {
				match request {
					wl_shm::Request::CreatePool { id, fd, size } => {
						log::debug!("Got request to create shm pool with fd {} and size {}", fd, size);
						/*let ptr: *mut std::ffi::c_void = unsafe {
							nix::sys::mman::mmap(
								std::ptr::null_mut() as *mut _,
								size as usize,
								nix::sys::mman::ProtFlags::PROT_READ | nix::sys::mman::ProtFlags::PROT_WRITE,
								nix::sys::mman::MapFlags::MAP_SHARED,
								fd,
								0,
							).expect("Failed to mmap shared memory")
						};
						sender.send(CompositorEvent::NewShm(shm));*/
						id.assign_mono(move |main: Main<wl_shm_pool::WlShmPool>, request: wl_shm_pool::Request| {
							match request {
								wl_shm_pool::Request::CreateBuffer { id, offset, width, height, stride, format } => {
									log::debug!("Got request to create shm buffer: offset {}, width {}, height {}, stride {}, format {:?}", offset, width, height, stride, format);
									//let buffer = id.deref().clone();
								},
								wl_shm_pool::Request::Destroy => {
									log::debug!("Got request to destroy shm pool")
								},
								wl_shm_pool::Request::Resize { size } => {
									log::debug!("Got request to resize shm pool to size {}", size);
								},
								_ => {
									log::warn!("Got unknown request for wl_shm_pool");
								},
							}
						})
					},
					_ => {
						log::warn!("Got unknown request for wl_shm");
					},
				}
			});
		});
	}
	
	fn setup_seat_global(&mut self) {
		let client_mgr = Rc::clone(&self.client_manager);
		self.display.create_global::<wl_seat::WlSeat, _>(6, move |main, version| {
			let client_mgr = Rc::clone(&client_mgr);
			main.assign_mono(move |main, request: wl_seat::Request| {
				let client_mgr = Rc::clone(&client_mgr);
				match request {
					wl_seat::Request::GetPointer { id } => {
						log::debug!("Got get_pointer request for wl_seat");
						let pointer = id.deref().clone();
						let resource = Resource::from(pointer.clone());
						client_mgr.borrow_mut().get_client_resources_mut(resource.client().unwrap()).pointer = Some(pointer);
					},
					wl_seat::Request::GetKeyboard { id } => {
						log::debug!("Got get_keyboard request for wl_seat");
						let keyboard = id.deref().clone();
						let resource = Resource::from(keyboard.clone());
						client_mgr.borrow_mut().get_client_resources_mut(resource.client().unwrap()).keyboard = Some(keyboard);
					},
					wl_seat::Request::GetTouch { .. } => {
						log::debug!("Got get_touch request for wl_seat");
					},
					wl_seat::Request::Release => {
						log::debug!("Got release request for wl_seat");
					},
					_ => {
						log::warn!("Got unknown request for wl_seat");
					},
				}
			});
		});
	}
	
	fn setup_data_device_manager_global(&mut self) {
		self.display.create_global::<wl_data_device_manager::WlDataDeviceManager, _>(3, |main, version| {
			main.assign_mono(|main, request: wl_data_device_manager::Request| {
				match request {
					wl_data_device_manager::Request::CreateDataSource { id } => {
						log::debug!("Got create_data_source request for wl_data_device_manager");
					},
					wl_data_device_manager::Request::GetDataDevice { id: _, seat: _ } => {
						log::debug!("Got get_data_device request for wl_data_device_manager");
					},
					_ => {
						log::warn!("Got unknown request for wl_data_device_manager");
					},
				}
			})
		});
	}
	
	fn setup_wl_shell_global(&mut self) {
		self.display.create_global::<wl_shell::WlShell, _>(1, |main, version| {
			main.assign_mono(|main, request: wl_shell::Request| {
				match request {
					wl_shell::Request::GetShellSurface {
						id, surface
					} => {
						log::debug!("Got get_shell_surface request for wl_shell");
					},
					_ => {
						log::warn!("Got unknown request for wl_shell");
					},
				}
			})
		});
	}
	
	fn setup_xdg_wm_base_global(&mut self) {
		self.display.create_global::<xdg_wm_base::XdgWmBase, _>(2, |main, version| {
			main.assign_mono(|main, request: xdg_wm_base::Request| {
				match request {
					xdg_wm_base::Request::Destroy => {
						log::debug!("Got xdg_wm_base destroy request");
					},
					xdg_wm_base::Request::CreatePositioner {
						id,
					} => {
						log::debug!("Got xdg_wm_base create_positioner request");
					},
					xdg_wm_base::Request::GetXdgSurface {
						id,
						surface,
					} => {
						log::debug!("Got xdg_wm_base get_xdg_surface request");
						id.assign_mono(|main, request: xdg_surface::Request| {
							match request {
								xdg_surface::Request::Destroy => {
									log::debug!("Got xdg_surface destroy request");
								},
								xdg_surface::Request::GetToplevel {
									id,
								} => {
									log::debug!("Got xdg_surface get_top_level request");
									id.assign_mono(|main, request: xdg_toplevel::Request| {
										match request {
											xdg_toplevel::Request::Destroy => {
												log::debug!("Got xdg_toplevel destroy request");
											},
											xdg_toplevel::Request::SetParent {
												parent,
											} => {
												log::debug!("Got xdg_toplevel set_parent request");
											},
											xdg_toplevel::Request::SetTitle {
												title,
											} => {
												log::debug!("Got xdg_toplevel set_title request");
											},
											xdg_toplevel::Request::SetAppId {
												app_id,
											} => {
												log::debug!("Got xdg_toplevel set_app_id request");
											},
											xdg_toplevel::Request::ShowWindowMenu {
												seat,
												serial,
												x,
												y,
											} => {
												log::debug!("Got xdg_toplevel show_window_meny request");
											},
											xdg_toplevel::Request::Move {
												seat,
												serial,
											} => {
												log::debug!("Got xdg_toplevel move request");
											},
											xdg_toplevel::Request::Resize {
												seat,
												serial,
												edges,
											} => {
												log::debug!("Got xdg_toplevel resize request");
											},
											xdg_toplevel::Request::SetMaxSize {
												width,
												height,
											} => {
												log::debug!("Got xdg_toplevel set_max_size request");
											},
											xdg_toplevel::Request::SetMinSize {
												width,
												height,
											} => {
												log::debug!("Got xdg_toplevel set_min_size request");
											},
											xdg_toplevel::Request::SetMaximized => {
												log::debug!("Got xdg_toplevel set_maximized request");
											},
											xdg_toplevel::Request::UnsetMaximized => {
												log::debug!("Got xdg_toplevel unset_maximized request");
											},
											xdg_toplevel::Request::SetFullscreen {
												output,
											} => {
												log::debug!("Got xdg_toplevel set_fullscreen request");
											},
											xdg_toplevel::Request::UnsetFullscreen => {
												log::debug!("Got xdg_toplevel unset_fullscreen request");
											},
											xdg_toplevel::Request::SetMinimized => {
												log::debug!("Got xdg_toplevel set_minimized request");
											},
											_ => {
												log::warn!("Got unknown request for xdg_toplevel");
											}
										}
									});
								},
								xdg_surface::Request::GetPopup {
									id,
									parent,
									positioner,
								} => {
									log::debug!("Got xdg_surface get_popup request");
								},
								xdg_surface::Request::SetWindowGeometry {
									x,
									y,
									width,
									height,
								} => {
									log::debug!("Got xdg_surface set_window_geometry request");
								},
								xdg_surface::Request::AckConfigure {
									serial,
								} => {
									log::debug!("Got xdg_surface ack_configure request");
								},
								_ => {
									log::warn!("Got unknown request for xdg_surface")
								}
							}
						});
					},
					xdg_wm_base::Request::Pong {
						serial: u32,
					} => {
						log::debug!("Got xdg_wm_base pong request");
					},
					_ => {
						log::warn!("Got unknown request for xdg_wm_base");
					},
				}
			});
		});
	}
}

impl<B: Backend> Drop for Compositor<B> {
	fn drop(&mut self) {
		log::trace!("Closing wayland socket");
		//let runtime_dir = env::var_os("XDG_RUNTIME_DIR").unwrap();
		fs::remove_file("/run/user/1000/wayland-0").unwrap();
	}
}

#[derive(Debug, Error)]
pub enum CompositorError {
	#[error("There was an error creating a wayland socket")]
	SocketError(#[source] io::Error),
}