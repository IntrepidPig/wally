use super::prelude::*;

impl<I: InputBackend, G: GraphicsBackend> Compositor<I, G> {
	pub fn setup_compositor_global(&mut self) {
		self.server.register_global::<WlCompositor, _>(|new: NewResource<_>| {
			new.register_fn(
				(),
				|state, this, request| {
					let state = state.get_mut::<CompositorState<I, G>>();
					state.handle_compositor_request(this, request);
				},
				|_state, _this| {
					log::warn!("wl_compositor destructor not implemented");
				},
			);
		});
	}
}

impl<I: InputBackend, G: GraphicsBackend> CompositorState<I, G> {
	pub fn handle_compositor_request(&mut self, this: Resource<WlCompositor>, request: WlCompositorRequest) {
		match request {
			WlCompositorRequest::CreateSurface(request) => self.handle_surface_create(this, request),
			WlCompositorRequest::CreateRegion(request) => self.handle_region_create(this, request),
		}
	}

	pub fn handle_surface_create(&mut self, this: Resource<WlCompositor>, request: wl_compositor::CreateSurfaceRequest) {
		let surface_renderer_data = self.graphics_state.renderer.create_surface_renderer_data().unwrap();
		let surface_data = RefCell::new(SurfaceData::new(this.client(), surface_renderer_data));
		request.id.register_fn(
			surface_data,
			|state, this, request| {
				let state = state.get_mut::<Self>();
				state.handle_surface_request(this, request);
			},
			|state, this| {
				let state = state.get_mut::<Self>();
				state.destroy_surface(this);
			},
		);
	}

	pub fn handle_region_create(&mut self, _this: Resource<WlCompositor>, request: wl_compositor::CreateRegionRequest) {
		request.id.register_fn(
			(),
			|_state, _this, _request| {
				log::warn!("Regions not implemented");
			},
			|_state, _this| {
				log::warn!("wl_region destructor not implemented");
			}
		);
	}
}