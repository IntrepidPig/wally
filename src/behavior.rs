use crate::compositor::prelude::*;

pub struct WindowManager<G: GraphicsBackend> {
	pub manager_impl: Box<dyn WindowManagerBehavior<G>>,
}

impl<G: GraphicsBackend + 'static> WindowManager<G> {
	pub fn new(manager_impl: Box<dyn WindowManagerBehavior<G>>) -> Self {
		Self { manager_impl }
	}

	pub fn get_surface_under_point(&self, point: Point) -> Option<wl_surface::WlSurface> {
		self.manager_impl.get_surface_under_point(point)
	}

	pub fn get_window_under_point(&self, point: Point) -> Option<wl_surface::WlSurface> {
		self.manager_impl.get_window_under_point(point)
	}
}

pub trait WindowManagerBehavior<G: GraphicsBackend + 'static> {
	fn add_surface(&mut self, surface: wl_surface::WlSurface);

	fn surfaces_ascending<'a>(&'a self) -> Box<dyn Iterator<Item = &'a wl_surface::WlSurface> + 'a>;

	fn handle_surface_resize(&mut self, surface: wl_surface::WlSurface, size: Size);

	fn get_surface_under_point(&self, point: Point) -> Option<wl_surface::WlSurface> {
		let mut got_surface = None;
		for surface in self.surfaces_ascending() {
			let surface_data = surface.as_ref().user_data().get::<Synced<SurfaceData<G>>>().unwrap();
			let surface_data_lock = surface_data.lock().unwrap();
			if surface_data_lock
				.try_get_surface_geometry()
				.map(|geometry| geometry.contains_point(point))
				.unwrap_or(false)
			{
				got_surface = Some(surface);
			}
		}
		got_surface.cloned()
	}

	fn get_window_under_point(&self, point: Point) -> Option<wl_surface::WlSurface> {
		let mut got_surface = None;
		for surface in self.surfaces_ascending() {
			let surface_data = surface.as_ref().user_data().get::<Synced<SurfaceData<G>>>().unwrap();
			let surface_data_lock = surface_data.lock().unwrap();
			if surface_data_lock
				.try_get_window_geometry()
				.map(|geometry| geometry.contains_point(point))
				.unwrap_or(false)
			{
				got_surface = Some(surface);
			}
		}
		got_surface.cloned()
	}
}

pub struct SurfaceTree<G: GraphicsBackend + ?Sized> {
	pub(crate) nodes: Vec<Node>,
	pub pointer: Arc<Mutex<PointerState>>,
	phantom: PhantomData<G>,
}

#[derive(Clone)]
pub struct Node {
	pub wl_surface: wl_surface::WlSurface,
}

impl From<wl_surface::WlSurface> for Node {
	fn from(wl_surface: wl_surface::WlSurface) -> Self {
		Node { wl_surface }
	}
}

impl<G: GraphicsBackend + 'static> SurfaceTree<G> {
	pub fn new(pointer: Arc<Mutex<PointerState>>) -> Self {
		Self {
			nodes: Vec::new(),
			pointer,
			phantom: PhantomData,
		}
	}

	pub fn add_surface(&mut self, surface: wl_surface::WlSurface) {
		self.nodes.push(Node::from(surface));
	}

	pub fn nodes_ascending(&self) -> impl Iterator<Item = &Node> {
		self.nodes.iter().map(|node| node)
	}

	pub fn nodes_descending(&self) -> impl Iterator<Item = &Node> {
		self.nodes_ascending().collect::<Vec<_>>().into_iter().rev()
	}

	pub fn destroy_surface(&mut self, surface: wl_surface::WlSurface) {
		// This bit right here doesn't work because dead surfaces lose their ids
		if let Some(i) = self
			.nodes
			.iter()
			.enumerate()
			.find(|(_i, test_surface)| test_surface.wl_surface == surface)
			.map(|x| x.0)
		{
			let surface = self.nodes.remove(i);
			let surface_data = surface
				.wl_surface
				.as_ref()
				.user_data()
				.get::<Arc<Mutex<SurfaceData<G>>>>()
				.unwrap();
			let mut surface_data_lock = surface_data.lock().unwrap();
			surface_data_lock.destroy();
		}
	}
}

pub struct DumbWindowManagerBehavior<G: GraphicsBackend> {
	pub surface_tree: SurfaceTree<G>,
}

impl<G: GraphicsBackend + 'static> DumbWindowManagerBehavior<G> {
	pub fn new(pointer_state: Synced<PointerState>) -> Self {
		Self {
			surface_tree: SurfaceTree::new(pointer_state),
		}
	}
}

impl<G: GraphicsBackend + 'static> WindowManagerBehavior<G> for DumbWindowManagerBehavior<G> {
	fn add_surface(&mut self, surface: wl_surface::WlSurface) {
		log::debug!("Added surface");
		let surface_data = surface.as_ref().user_data().get::<Synced<SurfaceData<G>>>().unwrap();
		let mut surface_data_lock = surface_data.lock().unwrap();
		if let Some(ref role) = surface_data_lock.role {
			let position = Point::new((dumb_rand() % 200 + 50) as i32, (dumb_rand() % 200 + 50) as i32);
			let size = Size::new(500, 375);
			surface_data_lock.set_window_position(position);
			surface_data_lock.resize_window(size);
		} else {
			panic!("Can't add a surface without a role");
		}
		drop(surface_data_lock);
		self.surface_tree.add_surface(surface);
	}

	fn handle_surface_resize(&mut self, surface: wl_surface::WlSurface, _new_size: Size) {
		let surface_data = surface.as_ref().user_data().get::<Synced<SurfaceData<G>>>().unwrap();
		let mut _surface_data_lock = surface_data.lock().unwrap();
		log::warn!("Surface resize handling not implemented");
	}

	fn surfaces_ascending<'a>(&'a self) -> Box<dyn Iterator<Item = &'a wl_surface::WlSurface> + 'a> {
		Box::new(self.surface_tree.nodes_ascending().map(|node| &node.wl_surface))
	}
}

static mut XOR_STATE: u32 = 0;

fn dumb_rand() -> u32 {
	unsafe {
		if XOR_STATE == 0 {
			XOR_STATE = std::time::SystemTime::UNIX_EPOCH.elapsed().unwrap().subsec_nanos();
		}
		let mut x = XOR_STATE;
		x ^= x << 13;
		x ^= x >> 17;
		x ^= x << 5;
		XOR_STATE = x;
		x
	}
}
