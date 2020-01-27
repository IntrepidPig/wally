use crate::backend::{AsRawFd, RawFd, InputBackend, BackendEvent, RenderBackend};

pub struct WindowRenderBackend {

}

impl WindowRenderBackend {
	pub fn new() -> Result<Self, ()> {
		unimplemented!()
	}
}

impl RenderBackend for WindowRenderBackend {
	type Error = ();
	type ShmPool = ();
	
	fn update(&mut self) -> Result<(), Self::Error> {
		Ok(())
	}
	
	fn create_shm_pool(&mut self, fd: RawFd, size: usize) -> Result<Self::ShmPool, Self::Error> {
		Ok(())
	}
}