use std::{
	fmt,
};

use wl_protocols::xdg_shell::*;

use super::prelude::*;
use crate::{
	backend::{ShmBuffer},
	compositor::{
		shm::{BufferData},
		xdg::{XdgSurfaceData},
	},
	renderer::{SurfaceRendererData},
};

impl<I: InputBackend, G: GraphicsBackend> CompositorState<I, G> {
	pub fn handle_surface_request(&mut self, this: Resource<WlSurface, SurfaceData<G>>, request: WlSurfaceRequest) {
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

	pub fn handle_surface_destroy(&mut self, this: Resource<WlSurface, SurfaceData<G>>) {
		this.destroy();
	}

	pub fn handle_surface_attach(&mut self, this: Resource<WlSurface, SurfaceData<G>>, request: wl_surface::AttachRequest) {
		let surface_data = this.get_data();
		let mut surface_data = surface_data.inner.borrow_mut();

		// Release the previously attached buffer if it hasn't been committed yet
		if let Some(Some((old_buffer, _))) = surface_data.pending_state.attached_buffer.take() {
			old_buffer.send_event(WlBufferEvent::Release);
		};
		// Attach the new buffer to the surface
		if let Some(buffer) = request.buffer {
			let buffer = buffer.downcast_data().unwrap();
			surface_data.pending_state.attached_buffer = Some(Some((buffer, Point::new(request.x, request.y))));
		} else {
			// Attaching a null buffer to a surface is equivalent to unmapping it.
			surface_data.pending_state.attached_buffer = Some(None);
		}
	}

	pub fn handle_surface_commit(&mut self, this: Resource<WlSurface, SurfaceData<G>>) {
		let surface_data = this.get_data();
		let old_size = surface_data.inner.borrow().buffer_size;
		surface_data.inner.borrow_mut().commit_pending_state();
		let new_size = surface_data.inner.borrow().buffer_size;

		if old_size != new_size {
			self.handle_surface_size_set(this, old_size, new_size);
		}
	}

	pub fn handle_surface_frame(&mut self, this: Resource<WlSurface, SurfaceData<G>>, request: wl_surface::FrameRequest) {
		let surface_data = this.get_data();
		let surface_data = &mut *surface_data.inner.borrow_mut();
		let callback = request.callback.register_fn(
			(),
			|_, _, _| {},
			|_, _| {},
		);
		
		if let Some(old_callback) = surface_data.callback.replace(callback) {
			old_callback.send_event(WlCallbackEvent::Done(wl_callback::DoneEvent {
				callback_data: get_time_ms(),
			}));
			old_callback.destroy();
		}
	}

	pub fn destroy_surface(&mut self, surface: Resource<WlSurface, SurfaceData<G>>) {
		self.inner.window_manager.remove_surface(surface.clone());

		if self.inner.pointer_focus.as_ref().map(|focus| focus.is(&surface)).unwrap_or(false) {
			self.inner.pointer_focus.take();
		}

		if self.inner.keyboard_focus.as_ref().map(|focus| focus.is(&surface)).unwrap_or(false) {
			self.inner.keyboard_focus.take();
		}

		let surface_data = surface.get_data();
		let mut surface_data = surface_data.inner.borrow_mut();

		match self.graphics_state.renderer.destroy_surface_renderer_data(surface_data.renderer_data.take().unwrap()) {
			Ok(()) => {},
			Err(e) => {
				log::error!("Failed to destroy surface renderer data: {}", e);
			},
		};
	}
}

impl<I: InputBackend, G: GraphicsBackend> CompositorState<I, G> {
	pub fn handle_surface_size_set(&mut self, surface: Resource<WlSurface, SurfaceData<G>>, old_size: Option<Size>, new_size: Option<Size>) {
		match (old_size, new_size) {
			(Some(_old_size), Some(new_size)) => self.handle_surface_resize(surface.clone(), new_size),
			(None, Some(new_size)) => self.handle_surface_map(surface.clone(), new_size),
			(Some(_old_size), None) => self.handle_surface_unmap(surface.clone()),
			(None, None) => {},
		};

		// TODO: this also has to happen on a surface position change
		self.update_surface_outputs(surface);
	}

	pub fn handle_surface_resize(&mut self, surface: Resource<WlSurface, SurfaceData<G>>, new_size: Size) {
		self.inner.window_manager.handle_surface_resize(surface, new_size);
	}

	pub fn handle_surface_map(&mut self, surface: Resource<WlSurface, SurfaceData<G>>, new_size: Size) {
		self.inner.window_manager.handle_surface_map(surface, new_size);
	}

	pub fn handle_surface_unmap(&mut self, surface: Resource<WlSurface, SurfaceData<G>>) {
		self.inner.window_manager.handle_surface_unmap(surface);
	}

	pub fn update_surface_outputs(&mut self, surface: Resource<WlSurface, SurfaceData<G>>) {
		// TODO: add support for exits
		let client = surface.client();
		let client = client.get().unwrap();
		let client_state = client.state::<RefCell<ClientState<G>>>();

		let node = self.inner.window_manager.tree.find(|node| node.surface.borrow().is(&surface));
		
		for output in &client_state.borrow().outputs {
			let output_data: Ref<OutputData<G>> = output.get_data();
			if let Some(geometry) = node.as_ref().and_then(|node| node.node_surface_geometry()) {
				if geometry.intersects(output_data.output.viewport) {
					surface.send_event(WlSurfaceEvent::Enter(wl_surface::EnterEvent {
						output: output.to_untyped(),
					}));
				}
			}
		}
	}
}

pub struct SurfaceData<G: GraphicsBackend> {
	pub inner: RefCell<SurfaceDataInner<G>>,
}

impl<G: GraphicsBackend> SurfaceData<G> {
	pub fn new(client: Handle<Client>, renderer_data: SurfaceRendererData<G>) -> Self {
		Self {
			inner: RefCell::new(SurfaceDataInner {
				client,
				pending_state: PendingState::new(),
				committed_buffer: None,
				buffer_size: None,
				input_region: None,
				callback: None,
				role: None,
				renderer_data: Some(renderer_data),
			}),
		}
	}
}

pub struct PendingState<G: GraphicsBackend> {
	pub attached_buffer: Option<Option<(Resource<WlBuffer, BufferData<G>>, Point)>>,
	pub input_region: Option<Rect>,
}

impl<G: GraphicsBackend> PendingState<G> {
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
pub struct SurfaceDataInner<G: GraphicsBackend> {
	pub client: Handle<Client>,
	/// All of the pending state that has been requested by the client but not yet committed
	pub pending_state: PendingState<G>,
	/// The most recently committed buffer to this surface
	pub committed_buffer: Option<(Resource<WlBuffer, BufferData<G>>, Point)>,
	/// This field is updated whenever a new buffer is committed
	pub buffer_size: Option<Size>,
	pub input_region: Option<Rect>,
	pub callback: Option<Resource<WlCallback, ()>>,
	pub role: Option<Role<G>>,
	/// The data that is necessary for the specific graphics backend to render this surface
	pub renderer_data: Option<SurfaceRendererData<G>>,
}

impl<G: GraphicsBackend> SurfaceDataInner<G> {
	pub fn get_surface_size(&self) -> Option<Size> {
		self.buffer_size
	}

	pub fn get_solid_window_geometry(&self) -> Option<Rect> {
		self.role.as_ref().and_then(|role| role.get_solid_window_geometry())
	}

	/// Commit all pending state to this surface
	pub fn commit_pending_state(&mut self) {
		if let Some(new_buffer) = self.pending_state.attached_buffer.take() {
			if let Some((new_buffer, point)) = new_buffer.as_ref() {
				if point.x != 0 || point.y != 0 {
					// TODO
					log::error!("Buffer attachments with a specific position are not supported yet");
				}
				let committed_buffer_data: Ref<BufferData<G>> = new_buffer.get_data();
				self.buffer_size = Some(Size::new(
					committed_buffer_data.buffer.width() as u32,
					committed_buffer_data.buffer.height() as u32,
				));
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
		if let Some(ref mut role) = self.role {
			role.commit_pending_state();
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

#[derive(Clone)]
pub enum Role<G: GraphicsBackend> {
	XdgSurface(Resource<XdgSurface, XdgSurfaceData<G>>),
}

// TODO: maybe move these to CompositorState impl like in the xdg module?
impl<G: GraphicsBackend> Role<G> {
	pub fn destroy(&mut self) {
		match *self {
			Role::XdgSurface(ref _xdg_surface) => {}
		}
	}

	pub fn commit_pending_state(&mut self) {
		match self {
			Role::XdgSurface(ref xdg_surface) => {
				xdg_surface.get_data().inner.borrow_mut().commit_pending_state()
			}
		}
	}

	pub fn set_surface_size(&mut self, _size: Size) {
		match self {
			Role::XdgSurface(ref _xdg_surface) => log::warn!("Set surface size not fully implemented"),
		}
	}

	pub fn get_solid_window_geometry(&self) -> Option<Rect> {
		match self {
			Role::XdgSurface(ref xdg_surface) => {
				let xdg_surface_data = xdg_surface.get_data();
				let geometry = xdg_surface_data.inner.borrow().solid_window_geometry;
				geometry
			}
		}
	}
}

impl<G: GraphicsBackend> fmt::Debug for Role<G> {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match *self {
			Role::XdgSurface(ref xdg_surface) => {
				let xdg_surface_data = xdg_surface.get_data();
				let res = write!(f, "Role: {:?}", xdg_surface_data.inner.borrow());
				res
			}
		}
	}
}
