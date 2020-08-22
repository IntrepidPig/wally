use crate::{
	compositor::{Compositor, prelude::*},
};

impl<I: InputBackend + 'static, G: GraphicsBackend + 'static> Compositor<I, G> {
	pub fn setup_compositor_global(&mut self) {
		self.server.register_global::<WlCompositor, _>(|new: NewResource<_>| {
			new.register_fn((), |state, this, request| {
				let state = state.get_mut::<CompositorState<I, G>>();
				state.handle_compositor_request(this, request);
			});
		});
	}
}

impl<I: InputBackend + 'static, G: GraphicsBackend + 'static> CompositorState<I, G> {
	pub fn handle_compositor_request(&mut self, this: Resource<WlCompositor>, request: WlCompositorRequest) {
		match request {
			WlCompositorRequest::CreateSurface(request) => self.handle_surface_create(this, request),
			WlCompositorRequest::CreateRegion(request) => self.handle_region_create(this, request),
		}
	}

	pub fn handle_surface_create(&mut self, this: Resource<WlCompositor>, request: wl_compositor::CreateSurfaceRequest) {
		log::trace!("Creating surface");
		let surface_renderer_data = self.graphics_state.renderer.create_surface_renderer_data().unwrap();
		let surface_data = RefCell::new(SurfaceData::new(this.client(), surface_renderer_data));
		request.id.register_fn(surface_data, |state, this, request| {
			let state = state.get_mut::<Self>();
			state.handle_surface_request(this, request);
		});
		/* id.assign_destructor(Filter::new(
			move |surface: wl_surface::WlSurface, _filter, _dispatch_data| {
				log::trace!("Destroying wl_surface");
				let mut graphics_backend_state_lock = graphics_backend_destructor.lock().unwrap();
				let surface_data = surface.get_synced::<SurfaceData<G>>();
				graphics_backend_state_lock
					.renderer
					.destroy_surface_renderer_data(
						surface_data.lock().unwrap().renderer_data.take().unwrap(),
					)
					.map_err(|e| log::error!("Failed to destroy surface: {}", e))
					.unwrap();
				let mut inner = inner_destructor.lock().unwrap();
				inner.trim_dead_clients();
			},
		)); */
	}

	pub fn handle_region_create(&mut self, _this: Resource<WlCompositor>, request: wl_compositor::CreateRegionRequest) {
		request.id.register_fn((), |_state, _this, _request| {
			log::warn!("Regions not implemented");
		});
	}
}