use std::{
	os::unix::io::RawFd,
	sync::{Arc, Mutex},
};

use wayland_server::{protocol::*, Filter, Global, Main};

use crate::{backend::Backend, compositor::Compositor};
use std::sync::MutexGuard;

#[derive(Debug)]
pub struct ShmPool {
	pub ptr: *mut u8,
	pub fd: RawFd,
	pub size: usize,
}

unsafe impl Send for ShmPool {}

#[derive(Debug)]
pub struct ShmBuffer {
	pub pool: Arc<Mutex<ShmPool>>,
	pub offset: usize,
	pub width: usize,
	pub height: usize,
	pub stride: usize,
	pub format: wl_shm::Format,
}

impl ShmBuffer {
	pub fn get_size(&self) -> usize {
		self.stride * self.height
	}

	pub unsafe fn get_ptr(&self) -> (*mut u8, MutexGuard<ShmPool>) {
		let pool_lock = self.pool.lock().unwrap();
		let ptr = (pool_lock.ptr as *mut u8).offset(self.offset as isize) as *mut _;
		(ptr, pool_lock)
	}

	pub unsafe fn as_slice<'a>(&self) -> (&'a [u8], MutexGuard<ShmPool>) {
		let (ptr, guard) = self.get_ptr();
		assert!(self.offset + self.get_size() <= guard.size);
		let slice = std::slice::from_raw_parts(ptr as *mut _ as *const _, self.get_size());
		(std::mem::transmute(slice), guard)
	}
}

impl<B: Backend> Compositor<B> {
	pub(crate) fn setup_shm_global(&mut self) -> Global<wl_shm::WlShm> {
		let shm_filter = Filter::new(
			move |(main, _num): (Main<wl_shm::WlShm>, u32), filter, _dispatch_data| {
				let shm_interface = &*main;
				shm_interface.format(wl_shm::Format::Argb8888);
				shm_interface.format(wl_shm::Format::Xrgb8888);
				main.quick_assign(move |main, request, _dispatch_data| {
				match request {
					wl_shm::Request::CreatePool { id, fd, size } => {
						log::debug!("Got request to create shm pool with fd {} and size {}", fd, size);
						let ptr: *mut u8 = unsafe {
							nix::sys::mman::mmap(
								std::ptr::null_mut(),
								size as usize,
								nix::sys::mman::ProtFlags::PROT_READ,
								nix::sys::mman::MapFlags::MAP_SHARED,
								fd,
								0,
							).expect("Failed to mmap shared memory") as *mut u8
						};
						let shm_pool = Arc::new(Mutex::new(ShmPool {
							ptr,
							fd,
							size: size as usize,
						}));
						let shm_pool_clone = Arc::clone(&shm_pool);
						id.as_ref().user_data().set_threadsafe(move || shm_pool_clone);
						id.quick_assign(move |main: Main<wl_shm_pool::WlShmPool>, request: wl_shm_pool::Request, _| {
							let shm_pool = Arc::clone(&shm_pool);
							match request {
								wl_shm_pool::Request::CreateBuffer { id, offset, width, height, stride, format } => {
									log::debug!("Got request to create shm buffer: offset {}, width {}, height {}, stride {}, format {:?}", offset, width, height, stride, format);
									// TODO this doesn't need to be in a Mutex I'm pretty sure because it can't be changed
									let shm_buffer = Arc::new(Mutex::new(ShmBuffer {
										pool: Arc::clone(&shm_pool),
										offset: offset as usize,
										width: width as usize,
										height: height as usize,
										stride: stride as usize,
										format,
									}));
									id.as_ref().user_data().set_threadsafe(|| Arc::clone(&shm_buffer));
									id.quick_assign(|main: Main<wl_buffer::WlBuffer>, request: wl_buffer::Request, _dispatch_data| {
										match request {
											wl_buffer::Request::Destroy => {
												log::debug!("Got request to destroy buffer");
											},
											_ => {
												log::warn!("Got unknown request for wl_buffer");
											},
										}
									});
								},
								wl_shm_pool::Request::Destroy => {
									log::debug!("Got request to destroy shm pool")
								},
								wl_shm_pool::Request::Resize { size } => {
									log::debug!("Got request to resize shm pool to size {}", size);
									unsafe {
										let mut pool = shm_pool.lock().unwrap();
										use nix::sys::mman;
										mman::munmap(pool.ptr as *mut _, pool.size).expect("Failed to unmap shared memory");
										let new_addr = mman::mmap(
											std::ptr::null_mut(),
											size as usize,
											nix::sys::mman::ProtFlags::PROT_READ,
											nix::sys::mman::MapFlags::MAP_SHARED,
											pool.fd,
											0,
										).expect("Failed to remap shared memory") as *mut _;
										pool.ptr = new_addr;
										pool.size = size as usize;
									}
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
			},
		);
		let shm_global = self.display.create_global::<wl_shm::WlShm, _>(1, shm_filter);

		shm_global
	}
}
