use crate::{
	compositor::{Compositor, prelude::*},
};

impl<I: InputBackend, G: GraphicsBackend> Compositor<I, G> {
	pub fn setup_data_device_manager_global(&mut self) {
		self.server.register_global(|new: NewResource<WlDataDeviceManager>| {
			log::warn!("wl_data_device_manager interface not implemented");
			new.register_fn(
				(),
				|_state, _this, _request| {
					
				},
				|_state, _this| {
					log::warn!("wl_data_device_manager destructor not implemented");
				},
			);
		});
	}
}