use crate::{backend::ShmBuffer, compositor::prelude::*, renderer::SurfaceRendererData};

impl<I: InputBackend, G: GraphicsBackend> CompositorState<I, G> {
	pub fn handle_surface_request(&mut self, this: Resource<WlSurface>, request: WlSurfaceRequest) {
		match request {
			WlSurfaceRequest::Destroy => self.handle_surface_destroy(this),
			WlSurfaceRequest::Attach(request) => self.handle_surface_attach(this, request),
			WlSurfaceRequest::Damage(_request) => {},
			WlSurfaceRequest::Frame(request) => self.handle_surface_frame(this, request),
			WlSurfaceRequest::SetOpaqueRegion { .. } => {},
			WlSurfaceRequest::SetInputRegion { .. } => {},
			WlSurfaceRequest::Commit => self.handle_surface_commit(this),
			WlSurfaceRequest::SetBufferTransform { .. } => {},
			WlSurfaceRequest::SetBufferScale { .. } => {},
			WlSurfaceRequest::DamageBuffer { .. } => {},
		}
	}

	pub fn handle_surface_destroy(&mut self, this: Resource<WlSurface>) {
		log::debug!("wl_surface::destroy request received");
		this.destroy();
	}

	pub fn handle_surface_attach(&mut self, this: Resource<WlSurface>, request: wl_surface::AttachRequest) {
		let surface_data: Ref<RefCell<SurfaceData<G>>> = this.get_user_data();
		let surface_data = &mut *surface_data.borrow_mut();

		// Release the previously attached buffer if it hasn't been committed yet
		if let Some(Some((old_buffer, _))) = surface_data.pending_state.attached_buffer.take() {
			old_buffer.send_event(WlBufferEvent::Release);
		};
		// Attach the new buffer to the surface
		if let Some(buffer) = request.buffer {
			surface_data.pending_state.attached_buffer = Some(Some((buffer, Point::new(request.x, request.y))));
		} else {
			// Attaching a null buffer to a surface is equivalent to unmapping it.
			surface_data.pending_state.attached_buffer = Some(None);
		}
	}

	pub fn handle_surface_commit(&mut self, this: Resource<WlSurface>) {
		let surface_data: Ref<RefCell<SurfaceData<G>>> = this.get_user_data();
		let mut surface_data = surface_data.borrow_mut();

		// TODO: relying on the impl of ShmBuffer to ascertain the size of the buffer is probably unsound if the ShmBuffer impl lies.
		// So that trait should either be unsafe, or Shm should be moved out of the Rendering backend and EasyShm should be made canonical
		surface_data.commit_pending_state();
		if let Some((ref committed_buffer, _point)) = surface_data.committed_buffer {
			let buffer_data: Ref<BufferData<G>> = committed_buffer.get_user_data();
			let _new_size = Size::new(buffer_data.buffer.width(), buffer_data.buffer.height());

			log::debug!("TODO: handle surface resize as window manager");
		}
	}

	pub fn handle_surface_frame(&mut self, this: Resource<WlSurface>, request: wl_surface::FrameRequest) {
		let surface_data: Ref<RefCell<SurfaceData<G>>> = this.get_user_data();
		let surface_data = &mut *surface_data.borrow_mut();
		let callback = request.callback.register_fn(
			(),
			|_, _, _| {},
			|_, _| {},
		);
		surface_data.callback = Some(callback);
	}
}

pub struct PendingState {
	pub attached_buffer: Option<Option<(Resource<WlBuffer>, Point)>>,
	pub input_region: Option<Rect>,
}

impl PendingState {
	pub fn new() -> Self {
		Self {
			attached_buffer: None,
			input_region: None,
		}
	}
}
/// This is the data associated with every surface. It is used to store the pending and committed state of the surface
/// (including pending and committed WlBuffers), the data required by the graphics backend for each surface, the
/// location and size of the surface, and the input devices useable by the surface.
///
/// Surfaces are not the same as windows. The implication of this is that the position of a window cannot be
/// set by setting the position of a surface, because surfaces can define things outside the borders of a window.
/// As such, methods on this struct that deal with geometry should specify whether they deal with window geometry, or
/// surface geometry. Operations on window geometry will translate given geometry to surface geometry and return window
/// geometry as translated from surface geometry. Window geometry is determined in a role specific manner.
///
/// The surface's role determines how it's geometry is decided.
pub struct SurfaceData<G: GraphicsBackend> {
	pub client: Handle<Client>,
	/// All of the pending state that has been requested by the client but not yet committed
	pub pending_state: PendingState,
	/// The most recently committed buffer to this surface
	pub committed_buffer: Option<(Resource<WlBuffer>, Point)>,
	/// This field is updated whenever a new buffer is committed to avoid re-locking the ShmBuffer mutex
	pub buffer_size: Option<Size>,
	pub input_region: Option<Rect>,
	pub callback: Option<Resource<WlCallback>>,
	pub role: Option<Role>,
	/// The data that is necessary for the specific graphics backend to render this surface
	pub renderer_data: Option<SurfaceRendererData<G>>,
	/// The current position of this surface in global compositor coordinates. None means the surface
	/// has no known position and, as such, will not be displayed.
	pub position: Option<Point>,
	/// The current size of this surface as dictated by the window manager. None means the surface
	/// has no known size and, as such, will not be displayed.
	size: Option<Size>,
}

impl<G: GraphicsBackend> SurfaceData<G> {
	pub fn new(client: Handle<Client>, renderer_data: SurfaceRendererData<G>) -> Self {
		Self {
			client,
			pending_state: PendingState::new(),
			committed_buffer: None,
			buffer_size: None,
			input_region: None,
			callback: None,
			role: None,
			renderer_data: Some(renderer_data),
			position: None,
			size: None,
		}
	}

	/// Set the position of the surface in order for the window geometry to be at the given position
	pub fn set_window_position(&mut self, position: Point) {
		if let Some(solid_window_geometry) = self.role.as_ref().and_then(|role| role.get_solid_window_geometry()) {
			self.position = Some(Point::new(
				position.x - solid_window_geometry.x,
				position.y - solid_window_geometry.y,
			));
		} else {
			self.position = Some(position)
		}
	}

	pub fn resize_window(&self, size: Size) {
		if let Some(ref role) = self.role {
			role.request_resize(size);
		} else {
			log::warn!("Tried to resize window with no role set");
		}
	}

	/// Returns the geometry of the window if both a position and size are set
	pub fn try_get_window_geometry(&self) -> Option<Rect> {
		// woah
		self.position
			.and_then(|position| {
				self.role
					.as_ref()
					.and_then(|role| {
						role.get_solid_window_geometry()
							.map(|solid_window_geometry| solid_window_geometry.size())
					})
					.map(|size| (position, size))
			})
			.map(Rect::from)
	}

	/// Returns the true geometry of the surface if a buffer is committed and the position is set
	pub fn try_get_surface_geometry(&self) -> Option<Rect> {
		if let Some(surface_position) = self.try_get_surface_position() {
			if let Some(buffer_size) = self.buffer_size {
				Some(Rect::from((surface_position, buffer_size)))
			} else {
				None
			}
		} else {
			None
		}
	}

	/// Returns the position of this
	pub fn try_get_surface_position(&self) -> Option<Point> {
		if let Some(window_position) = self.position {
			// Offset the window position by the solid window geometry coordinates to get the surface position
			if let Some(solid_window_geometry) = self.role.as_ref().and_then(|role| role.get_solid_window_geometry()) {
				Some(Point::new(
					window_position.x - solid_window_geometry.x,
					window_position.y - solid_window_geometry.y,
				))
			} else {
				Some(window_position)
			}
		} else {
			None
		}
	}

	/// Commit all pending state to this surface
	pub fn commit_pending_state(&mut self) {
		if let Some(new_buffer) = self.pending_state.attached_buffer.take() {
			if let Some((new_buffer, point)) = new_buffer.as_ref() {
				if point.x != 0 || point.y != 0 {
					// TODO
					log::error!("Buffer attachments with a specific position are not supported yet");
				}
				let committed_buffer_data: Ref<BufferData<G>> = new_buffer.get_user_data();
				if let Some(role) = self.role.as_mut() {
					role.set_surface_size(Size::new(
						committed_buffer_data.buffer.width() as u32,
						committed_buffer_data.buffer.height() as u32,
					));
				}
				self.buffer_size = Some(Size::new(
					committed_buffer_data.buffer.width() as u32,
					committed_buffer_data.buffer.height() as u32,
				))
			} else {
				self.buffer_size = None;
			}
			if let Some((old_buffer, _point)) = std::mem::replace(&mut self.committed_buffer, new_buffer) {
				// Release the previously attached buffer if it hasn't been committed yet
				old_buffer.send_event(WlBufferEvent::Release);
			}
		}
		if let Some(new_input_region) = self.pending_state.input_region.take() {
			self.input_region = Some(new_input_region);
		}
	}

	pub fn destroy(&mut self) {
		if let Some((buffer, _point)) = self.pending_state.attached_buffer.take().and_then(|opt| opt) {
			buffer.send_event(WlBufferEvent::Release);
		}
		if let Some((buffer, _point)) = self.committed_buffer.take() {
			buffer.send_event(WlBufferEvent::Release)
		}
		if let Some(mut role) = self.role.take() {
			role.destroy();
		}
	}
}

impl_user_data_graphics!(WlSurface, RefCell<SurfaceData<G>>);
