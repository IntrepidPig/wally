use crate::backend::{BackendEvent, InputBackend};
use calloop::channel::Channel;

pub struct HeadlessInputBackend {}

impl InputBackend for HeadlessInputBackend {
	type Error = ();

	fn update(&mut self) -> Result<(), Self::Error> {
		Ok(())
	}

	fn get_event_source(&mut self) -> Channel<BackendEvent> {
		let (sender, channel) = calloop::channel::channel();
		channel
	}
}

/*
pub struct HeadlessRenderBackend {

}

impl RenderBackend for HeadlessRenderBackend {
	type Error = ();
	type ShmPool = ();

	fn update(&mut self) -> Result<(), Self::Error> {
		Ok(())
	}

	fn add_shm_texture(&mut self, buffer: ShmBuffer) -> Result<(), Self::Error> {
		unimplemented!()
	}
}*/
