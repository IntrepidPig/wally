use crate::{
	compositor::{Compositor, prelude::*},
};

impl<I: InputBackend, G: GraphicsBackend> Compositor<I, G> {
	pub fn setup_data_device_manager_global(&mut self) {
		self.server.register_global(|new: NewResource<WlDataDeviceManager>| {
			log::warn!("wl_data_device_manager interface not implemented");
			new.register_fn(
				(),
				|state, this, request| {
					let state = state.get_mut::<CompositorState<I, G>>();
					state.handle_data_device_manager_request(this, request);
				},
				|_state, _this| {
					log::warn!("wl_data_device_manager destructor not implemented");
				},
			);
		});
	}
}

impl<I: InputBackend, G: GraphicsBackend> CompositorState<I, G> {
	pub fn handle_data_device_manager_request(&mut self, this: Resource<WlDataDeviceManager, ()>, request: WlDataDeviceManagerRequest) {
		match request {
			WlDataDeviceManagerRequest::CreateDataSource(request) => self.handle_data_device_manager_create_data_source(this, request),
			WlDataDeviceManagerRequest::GetDataDevice(request) => self.handle_data_device_manager_get_data_device(this, request),
		}
	}

	pub fn handle_data_device_manager_create_data_source(&mut self, _this: Resource<WlDataDeviceManager, ()>, request: wl_data_device_manager::CreateDataSourceRequest) {
		request.id.register_fn(
			(),
			|state, this, request| {
				let state = state.get_mut::<Self>();
				state.handle_data_source_request(this, request);
			},
			|_state, _this| {
				log::warn!("wl_data_source destructor not implemented");
			},
		);
	}

	pub fn handle_data_device_manager_get_data_device(&mut self, _this: Resource<WlDataDeviceManager, ()>, request: wl_data_device_manager::GetDataDeviceRequest) {
		request.id.register_fn(
			(),
			|_state, _this, request| {
				log::warn!("Unhandled request for wl_data_device_manager: {:?}", request);
			},
			|_state, _this| {
				log::warn!("wl_data_device_manager destructor not implemented");
			},
		);
	}

	pub fn handle_data_source_request(&mut self, this: Resource<WlDataSource, ()>, request: WlDataSourceRequest) {
		match request {
			WlDataSourceRequest::Offer(request) => self.handle_data_source_offer(this, request),
			WlDataSourceRequest::Destroy => self.handle_data_source_destroy(this),
			WlDataSourceRequest::SetActions(request) => self.handle_data_source_set_actions(this, request),
		}
	}

	pub fn handle_data_source_offer(&mut self, _this: Resource<WlDataSource, ()>, _request: wl_data_source::OfferRequest) {
		log::warn!("wl_data_source::offer not implemented");
	}

	pub fn handle_data_source_destroy(&mut self, this: Resource<WlDataSource, ()>) {
		this.destroy()
	}

	pub fn handle_data_source_set_actions(&mut self, _this: Resource<WlDataSource, ()>, _request: wl_data_source::SetActionsRequest) {
		log::warn!("wl_data_source::set_actions not implemented");
	}
}