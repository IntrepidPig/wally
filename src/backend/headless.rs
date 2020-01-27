use crate::backend::{RawFd, InputBackend, BackendEvent, RenderBackend};

pub struct HeadlessInputBackend {

}

impl InputBackend for HeadlessInputBackend {
	type Error = ();
	
	fn update(&mut self) -> Result<(), Self::Error> {
		Ok(())
	}
	
	fn poll_for_events(&mut self) -> Option<BackendEvent> {
		None
	}
}

pub struct HeadlessRenderBackend {

}

impl RenderBackend for HeadlessRenderBackend {
	type Error = ();
	type ShmPool = ();
	
	fn update(&mut self) -> Result<(), Self::Error> {
		Ok(())
	}
	
	fn create_shm_pool(&mut self, fd: RawFd, size: usize) -> Result<Self::ShmPool, Self::Error> {
		Ok(())
	}
}