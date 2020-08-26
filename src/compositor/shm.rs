use std::convert::TryFrom;

use crate::{
	backend::{GraphicsBackend, InputBackend},
	compositor::{Compositor, prelude::*},
};

const SUPPORTED_FORMATS: &[wl_shm::Format] = &[wl_shm::Format::Argb8888, wl_shm::Format::Xrgb8888];

impl<I: InputBackend, G: GraphicsBackend> Compositor<I, G> {
	pub(crate) fn setup_shm_global(&mut self) {
		self.server.register_global(|new: NewResource<WlShm>| {
			let shm = new.register_fn(
				(),
				|state, this, request| {
					let state = state.get_mut::<CompositorState<I, G>>();
					state.handle_shm_request(this, request);
				},
				|_state, _this| {
					log::warn!("wl_shm destructor not implemented");
				},
			);
			for &format in SUPPORTED_FORMATS {
				shm.send_event(WlShmEvent::Format(wl_shm::FormatEvent {
					format,
				}));
			}
		});
	}
}

impl<I: InputBackend, G: GraphicsBackend> CompositorState<I, G> {
	pub fn handle_shm_request(&mut self, this: Resource<WlShm, ()>, request: WlShmRequest) {
		match request {
			WlShmRequest::CreatePool(request) => self.handle_shm_create_pool(this, request),
		}
	}

	pub fn handle_shm_create_pool(&mut self, _this: Resource<WlShm, ()>, request: wl_shm::CreatePoolRequest) {
		let shm_pool = self
			.graphics_state
			.renderer
			.create_shm_pool(request.fd, request.size as usize)
			.expect("Failed to create shm pool");
		
		request.id.register_fn(
			ShmPoolData::<G>::new(RefCell::new(shm_pool)),
			|state, this, request| {
				let state = state.get_mut::<CompositorState<I, G>>();
				state.handle_shm_pool_request(this, request);
			},
			|_state, _this| {
				log::warn!("wl_shm_pool destructor not implemented");
			},
		);
	}

	pub fn handle_shm_pool_request(&mut self, this: Resource<WlShmPool, ShmPoolData<G>>, request: WlShmPoolRequest) {
		match request {
			WlShmPoolRequest::Destroy => self.handle_shm_pool_destroy(this),
			WlShmPoolRequest::CreateBuffer(request) => self.handle_shm_pool_create_buffer(this, request),
			WlShmPoolRequest::Resize(request) => self.handle_shm_pool_resize(this, request),
		}
	}

	pub fn handle_shm_pool_destroy(&mut self, this: Resource<WlShmPool, ShmPoolData<G>>) {
		this.destroy();
	}

	pub fn handle_shm_pool_create_buffer(&mut self, this: Resource<WlShmPool, ShmPoolData<G>>, request: wl_shm_pool::CreateBufferRequest) {
		let shm_pool: Ref<ShmPoolData<G>> = this.get_data();

		let offset = usize::try_from(request.offset).unwrap();
		let width = u32::try_from(request.width).unwrap();
		let height = u32::try_from(request.height).unwrap();
		let stride = u32::try_from(request.stride).unwrap();

		let shm_buffer: G::ShmBuffer = self.graphics_state.renderer
			.create_shm_buffer(
				&mut *shm_pool.pool.borrow_mut(),
				offset,
				width,
				height,
				stride,
				request.format,
			)
			.expect("Failed to create shm buffer");
		
		request.id.register_fn(
			BufferData::<G>::new(shm_buffer),
			|state, this, request| {
				let state = state.get_mut::<Self>();
				state.handle_buffer_request(this, request);
			},
			|_state, _this| {
				log::warn!("wl_buffer destructor not implemented");
			},
		);
	}

	pub fn handle_shm_pool_resize(&mut self, this: Resource<WlShmPool, ShmPoolData<G>>, request: wl_shm_pool::ResizeRequest) {
		let shm_pool: Ref<ShmPoolData<G>> = this.get_data();
		self.graphics_state
			.renderer
			.resize_shm_pool(&mut *shm_pool.pool.borrow_mut(), request.size as usize)
			.expect("Failed to resize shm pool");
	}

	pub fn handle_buffer_request(&mut self, this: Resource<WlBuffer, BufferData<G>>, request: WlBufferRequest) {
		match request {
			WlBufferRequest::Destroy => self.handle_buffer_destroy(this),
		}
	}

	pub fn handle_buffer_destroy(&mut self, this: Resource<WlBuffer, BufferData<G>>) {
		this.destroy();
	}
}

pub struct ShmPoolData<G: GraphicsBackend> {
	pub pool: RefCell<G::ShmPool>,
}

impl<G: GraphicsBackend> ShmPoolData<G> {
	pub fn new(pool: RefCell<G::ShmPool>) -> Self {
		Self {
			pool,
		}
	}
}

pub struct BufferData<G: GraphicsBackend> {
	pub buffer: G::ShmBuffer,
}

impl<G: GraphicsBackend> BufferData<G> {
	pub fn new(buffer: G::ShmBuffer) -> Self {
		Self {
			buffer,
		}
	}
}
