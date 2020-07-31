use crate::{
	backend::ShmBuffer,
	compositor::{prelude::*},
	renderer::SurfaceRendererData,
};

pub struct PendingState {
	pub attached_buffer: Option<Option<(wl_buffer::WlBuffer, Point)>>,
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
	/// Contains the client, pointers, and keyboards associated with this surface
	pub client_info: Synced<ClientInfo>,
	/// All of the pending state that has been requested by the client but not yet committed
	pub pending_state: PendingState,
	/// The most recently committed buffer to this surface
	pub committed_buffer: Option<(wl_buffer::WlBuffer, Point)>,
	/// This field is updated whenever a new buffer is committed to avoid re-locking the ShmBuffer mutex
	pub buffer_size: Option<Size>,
	pub input_region: Option<Rect>,
	pub callback: Option<wl_callback::WlCallback>,
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

impl<G: GraphicsBackend + 'static> SurfaceData<G> {
	pub fn new(client_info: Synced<ClientInfo>, renderer_data: SurfaceRendererData<G>) -> Self {
		Self {
			client_info,
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

	pub fn resize_window(&mut self, size: Size) {
		if let Some(ref mut role) = self.role {
			role.resize_window(size);
			if let Some(solid_window_geometry) = role.get_solid_window_geometry() {
				self.size = Some(Size::new(
					size.width + solid_window_geometry.width * 2,
					size.height + solid_window_geometry.height * 2,
				));
			} else {
				self.size = Some(size);
			}
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
			if let Some(new_buffer) = new_buffer.as_ref() {
				let committed_buffer_data = new_buffer.0.as_ref().user_data().get::<Synced<G::ShmBuffer>>().unwrap();
				let committed_buffer_data_lock = committed_buffer_data.lock().unwrap();
				if let Some(role) = self.role.as_mut() {
					role.set_surface_size(Size::new(
						committed_buffer_data_lock.width() as u32,
						committed_buffer_data_lock.height() as u32,
					));
				}
				self.buffer_size = Some(Size::new(
					committed_buffer_data_lock.width() as u32,
					committed_buffer_data_lock.height() as u32,
				))
			} else {
				self.buffer_size = None;
			}
			if let Some(old_buffer) = std::mem::replace(&mut self.committed_buffer, new_buffer) {
				// Release the previously attached buffer if it hasn't been committed yet
				old_buffer.0.release();
			}
		}
		if let Some(new_input_region) = self.pending_state.input_region.take() {
			self.input_region = Some(new_input_region);
		}
	}

	pub fn destroy(&mut self) {
		// TODO: does this need to destroy the SurfaceRenderData too?
		if let Some((buffer, _)) = self.pending_state.attached_buffer.take().and_then(|opt| opt) {
			buffer.release();
		}
		if let Some((buffer, _)) = self.committed_buffer.take() {
			buffer.release();
		}
		if let Some(mut role) = self.role.take() {
			role.destroy();
		}
	}
}
