use crate::compositor::prelude::*;

pub struct SurfaceTree<R: RenderBackend + ?Sized> {
	pub(crate) surfaces: Vec<wl_surface::WlSurface>,
	pub pointer: Arc<Mutex<PointerState>>,
	phantom: PhantomData<R>,
}

impl<R: RenderBackend> SurfaceTree<R> {
	pub fn new(pointer: Arc<Mutex<PointerState>>) -> Self {
		Self {
			surfaces: Vec::new(),
			pointer,
			phantom: PhantomData,
		}
	}

	pub fn add_surface(&mut self, surface: wl_surface::WlSurface) {
		self.surfaces.push(surface);
	}

	pub fn surfaces_ascending(&self) -> impl Iterator<Item = &wl_surface::WlSurface> {
		self.surfaces.iter()
	}

	pub fn surfaces_descending(&self) -> impl Iterator<Item = &wl_surface::WlSurface> {
		self.surfaces_ascending().collect::<Vec<_>>().into_iter().rev()
	}

	pub fn destroy_surface(&mut self, surface: wl_surface::WlSurface) {
		// This bit right here doesn't work because dead surfaces lose their ids
		if let Some(i) = self
			.surfaces
			.iter()
			.enumerate()
			.find(|(_i, test_surface)| **test_surface == surface)
			.map(|x| x.0)
		{
			let surface = self.surfaces.remove(i);
			let surface_data = surface
				.as_ref()
				.user_data()
				.get::<Arc<Mutex<SurfaceData<R::ObjectHandle>>>>()
				.unwrap();
			let mut surface_data_lock = surface_data.lock().unwrap();
			surface_data_lock.destroy();
		}
	}

	pub fn get_surface_under_point(&self, point: (i32, i32)) -> Option<wl_surface::WlSurface> {
		let mut i = self.surfaces.len() - 1;
		loop {
			let surface = &self.surfaces[i];
			let surface_data = surface
				.as_ref()
				.user_data()
				.get::<Arc<Mutex<SurfaceData<R::ObjectHandle>>>>()
				.unwrap();
			let surface_data_lock = surface_data.lock().unwrap();
			let (x, y, width, height) = if let Some(geometry) = surface_data_lock.get_geometry() {
				geometry
			} else {
				if i == 0 {
					break;
				} else {
					i -= 1;
					continue;
				}
			};
			if point.0 >= x && point.1 >= y && point.0 <= x + width as i32 && point.1 <= y + height as i32 {
				return Some(surface.clone());
			}
			if i == 0 {
				break;
			} else {
				i -= 1;
			}
		}
		None
	}
}

pub struct SurfaceData<T> {
	pub client_info: Synced<ClientInfo>,
	pub attached_buffer: Option<(wl_buffer::WlBuffer, (i32, i32))>,
	pub committed_buffer: Option<(wl_buffer::WlBuffer, (i32, i32))>,
	pub draw: bool,
	pub callback: Option<wl_callback::WlCallback>,
	pub role: Option<Role>,
	pub renderer_data: Option<T>,
}

impl<T> SurfaceData<T> {
	pub fn new(client_info: Synced<ClientInfo>, renderer_data: Option<T>) -> Self {
		Self {
			client_info,
			attached_buffer: None,
			committed_buffer: None,
			draw: true,
			callback: None,
			role: None,
			renderer_data,
		}
	}

	/*pub fn replace_renderer_data<U>(&self, new_data: U) -> SurfaceData<U> {
		SurfaceData {
			attached_buffer: self.attached_buffer.clone(),
			committed_buffer: self.committed_buffer.clone(),
			callback: self.callback.clone(),
			role: self.role.clone(),
			renderer_data: new_data,
		}
	}*/

	pub fn get_geometry(&self) -> Option<(i32, i32, u32, u32)> {
		self.role.as_ref().and_then(|role| role.get_geometry())
	}

	pub fn destroy(&mut self) {
		if let Some((buffer, _)) = self.attached_buffer.take() {
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
