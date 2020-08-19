use std::convert::TryFrom;
use std::sync::{Arc, Mutex};

use wayland_server::{protocol::*, Filter, Global, Main};

use crate::{
	backend::{GraphicsBackend, InputBackend},
	compositor::Compositor,
};

/* #[derive(Debug)]
pub struct ShmBuffer<G: GraphicsBackend> {
	pub pool: Arc<Mutex<G::ShmPool>>,
	pub offset: usize,
	pub width: usize,
	pub height: usize,
	pub stride: usize,
	pub format: wl_shm::Format,
}

impl<G: GraphicsBackend> ShmBuffer<G> {
	pub fn get_size(&self) -> usize {
		self.stride * self.height
	}

	/* pub unsafe fn get_ptr(&self) -> (*mut u8, MutexGuard<G::ShmPool>) {
		let pool_lock = self.pool.lock().unwrap();
		let ptr = (pool_lock.ptr() as *mut u8).offset(self.offset as isize) as *mut _;
		(ptr, pool_lock)
	}

	pub unsafe fn as_slice<'a>(&self) -> (&'a [u8], MutexGuard<G::ShmPool>) {
		let (ptr, guard) = self.get_ptr();
		assert!(self.offset + self.get_size() <= guard.size());
		let slice = std::slice::from_raw_parts(ptr as *mut _ as *const _, self.get_size());
		(std::mem::transmute(slice), guard)
	} */
} */

impl<I: InputBackend + 'static, G: GraphicsBackend + 'static> Compositor<I, G> {
	pub(crate) fn setup_shm_global(&mut self) -> Global<wl_shm::WlShm> {
		let graphics_backend_state = Arc::clone(&self.graphics_backend_state);
		let shm_filter = Filter::new(
			move |(main, _num): (Main<wl_shm::WlShm>, u32), _filter, _dispatch_data| {
				let graphics_backend_state = Arc::clone(&graphics_backend_state);
				let shm_interface = &*main;
				shm_interface.format(wl_shm::Format::Argb8888);
				shm_interface.format(wl_shm::Format::Xrgb8888);
				main.quick_assign(move |_main, request, _dispatch_data| {
					let graphics_backend_state = Arc::clone(&graphics_backend_state);
					match request {
						wl_shm::Request::CreatePool { id, fd, size } => {
							let mut graphics_backend_state_lock = graphics_backend_state.lock().unwrap();
							let shm_pool = graphics_backend_state_lock
								.renderer
								.create_shm_pool(fd, size as usize)
								.map_err(|e| log::error!("Failed to create shm pool: {}", e))
								.unwrap();
							drop(graphics_backend_state_lock);
							let shm_pool = Arc::new(Mutex::new(shm_pool));
							id.quick_assign(
								move |_main: Main<wl_shm_pool::WlShmPool>, request: wl_shm_pool::Request, _| {
									let graphics_backend_state = Arc::clone(&graphics_backend_state);
									let shm_pool = Arc::clone(&shm_pool);
									match request {
										wl_shm_pool::Request::CreateBuffer {
											id,
											offset,
											width,
											height,
											stride,
											format,
										} => {
											// TODO this doesn't need to be in a Mutex I'm pretty sure because it can't be changed
											let mut graphics_backend_state_lock =
												graphics_backend_state.lock().unwrap();
											let mut shm_pool_lock = shm_pool.lock().unwrap();
											let offset = usize::try_from(offset).unwrap();
											let width = u32::try_from(width).unwrap();
											let height = u32::try_from(height).unwrap();
											let stride = u32::try_from(stride).unwrap();
											let shm_buffer: G::ShmBuffer = graphics_backend_state_lock
												.renderer
												.create_shm_buffer(
													&mut *shm_pool_lock,
													offset,
													width,
													height,
													stride,
													format,
												)
												.unwrap();
											let shm_buffer = Arc::new(Mutex::new(shm_buffer));
											id.as_ref().user_data().set_threadsafe(|| Arc::clone(&shm_buffer));
											id.quick_assign(
												|_main: Main<wl_buffer::WlBuffer>,
												 request: wl_buffer::Request,
												 _dispatch_data| {
													match request {
														wl_buffer::Request::Destroy => {}
														_ => {
															log::warn!("Got unknown request for wl_buffer");
														}
													}
												},
											);
										}
										wl_shm_pool::Request::Resize { size } => {
											let mut graphics_backend_state_lock =
												graphics_backend_state.lock().unwrap();
											let mut shm_pool_lock = shm_pool.lock().unwrap();
											graphics_backend_state_lock
												.renderer
												.resize_shm_pool(&mut *shm_pool_lock, size as usize)
												.unwrap();
										}
										_ => {
											log::warn!("Got unknown request for wl_shm_pool");
										}
									}
								},
							)
						}
						_ => {
							log::warn!("Got unknown request for wl_shm");
						}
					}
				});
			},
		);
		let shm_global = self.display.create_global::<wl_shm::WlShm, _>(1, shm_filter);

		shm_global
	}
}
