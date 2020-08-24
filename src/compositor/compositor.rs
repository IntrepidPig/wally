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
			RefCell::new(RegionData::new()),
			|state, this, request| {
				let state = state.get_mut::<CompositorState<I, G>>();
				state.handle_region_request(this, request);
			},
			|_state, _this| {
				log::warn!("wl_region destructor not implemented");
			}
		);
	}
}

impl<I: InputBackend, G: GraphicsBackend> CompositorState<I, G> {
	pub fn handle_region_request(&mut self, this: Resource<WlRegion>, request: WlRegionRequest) {
		match request {
			WlRegionRequest::Destroy => this.destroy(),
			WlRegionRequest::Add(request) => self.handle_region_add(this, request),
			WlRegionRequest::Subtract(request) => self.handle_region_subtract(this, request),
		}
	}

	pub fn handle_region_add(&mut self, this: Resource<WlRegion>, request: wl_region::AddRequest) {
		let region_data = this.get_user_data();
		region_data.borrow_mut().add(Rect::new(request.x, request.y, request.width as u32, request.height as u32));
	}

	pub fn handle_region_subtract(&mut self, this: Resource<WlRegion>, request: wl_region::SubtractRequest) {
		let region_data = this.get_user_data();
		region_data.borrow_mut().subtract(Rect::new(request.x, request.y, request.width as u32, request.height as u32));
	}
}

pub struct RegionData {
	pub events: Vec<RegionEvent>,
}

impl RegionData {
	pub fn new() -> Self {
		Self {
			events: Vec::new(),
		}
	}

	pub fn add(&mut self, rect: Rect) {
		self.events.push(RegionEvent::Add(rect));
	}

	pub fn subtract(&mut self, rect: Rect) {
		self.events.push(RegionEvent::Subtract(rect));
	}

	pub fn contains_point(&self, point: Point) -> bool {
		self.events.iter().fold(false, |contains, event| match *event {
			RegionEvent::Add(rect) => contains || rect.contains_point(point),
			RegionEvent::Subtract(rect) => contains && !rect.contains_point(point),
		})
	}
}

pub enum RegionEvent {
	Add(Rect),
	Subtract(Rect),
}

impl_user_data!(WlRegion, RefCell<RegionData>);