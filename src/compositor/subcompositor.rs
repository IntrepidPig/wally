use super::prelude::*;

impl<I: InputBackend, G: GraphicsBackend> Compositor<I, G> {
	pub fn setup_subcompositor_global(&mut self) {
		self.server.register_global::<WlSubcompositor, _>(|new: NewResource<_>| {
			new.register_fn(
				(),
				|state, this, request| {
					let state = state.get_mut::<CompositorState<I, G>>();
					state.handle_subcompositor_request(this, request);
				},
				|_state, _this| {
					log::warn!("wl_compositor destructor not implemented");
				},
			);
		});
	}
}

impl<I: InputBackend, G: GraphicsBackend> CompositorState<I, G> {
	pub fn handle_subcompositor_request(&mut self, this: Resource<WlSubcompositor, ()>, request: WlSubcompositorRequest) {
		match request {
			WlSubcompositorRequest::Destroy => self.handle_subcompositor_destroy(this),
			WlSubcompositorRequest::GetSubsurface(request) => self.handle_subcompositor_get_subsurface(this, request),
		}
	}

	pub fn handle_subcompositor_destroy(&mut self, this: Resource<WlSubcompositor, ()>) {
		this.destroy();	
	}

	pub fn handle_subcompositor_get_subsurface(&mut self, _this: Resource<WlSubcompositor, ()>, request: wl_subcompositor::GetSubsurfaceRequest) {
		request.id.register_fn(
			(),
			|_state, _this, request| {
				log::warn!("Unhandled request for wl_subsurface: {:?}", request)
			},
			|_state, _this| {
				log::warn!("wl_subsurface destructor not implemented");
			},
		);
	}
}