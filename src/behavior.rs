use crate::compositor::prelude::*;

pub struct WindowManager<G: GraphicsBackend> {
	pub manager_impl: Box<dyn WindowManagerBehavior<G>>,
}

impl<G: GraphicsBackend + 'static> WindowManager<G> {
	pub fn new(manager_impl: Box<dyn WindowManagerBehavior<G>>) -> Self {
		Self { manager_impl }
	}

	pub fn get_surface_under_point(&self, point: Point) -> Option<Resource<WlSurface>> {
		self.manager_impl.get_surface_under_point(point)
	}

	pub fn get_window_under_point(&self, point: Point) -> Option<Resource<WlSurface>> {
		self.manager_impl.get_window_under_point(point)
	}
}

pub trait WindowManagerBehavior<G: GraphicsBackend + 'static> {
	fn add_surface(&mut self, surface: Resource<WlSurface>);

	fn surfaces_ascending(&self) -> Box<dyn Iterator<Item = Resource<WlSurface>> + '_>;

	fn handle_surface_resize(&mut self, surface: Resource<WlSurface>, size: Size);

	fn get_surface_under_point(&self, point: Point) -> Option<Resource<WlSurface>> {
		let mut got_surface = None;
		for surface in self.surfaces_ascending() {
			let surface_data = surface.get_data::<RefCell<SurfaceData<G>>>().unwrap();
			if surface_data.borrow()
				.try_get_surface_geometry()
				.map(|geometry| geometry.contains_point(point))
				.unwrap_or(false)
			{
				got_surface = Some(surface);
			}
		}
		got_surface
	}

	fn get_window_under_point(&self, point: Point) -> Option<Resource<WlSurface>> {
		let mut got_surface = None;
		for surface in self.surfaces_ascending() {
			let surface_data = surface.get_data::<RefCell<SurfaceData<G>>>().unwrap();
			if surface_data.borrow()
				.try_get_window_geometry()
				.map(|geometry| geometry.contains_point(point))
				.unwrap_or(false)
			{
				got_surface = Some(surface);
			}
		}
		got_surface
	}
}

pub struct SurfaceTree<G: GraphicsBackend + ?Sized> {
	pub(crate) nodes: Vec<Node>,
	phantom: PhantomData<G>,
}

#[derive(Clone)]
pub struct Node {
	pub surface: Resource<WlSurface>,
}

impl From<Resource<WlSurface>> for Node {
	fn from(surface: Resource<WlSurface>) -> Self {
		Node { surface }
	}
}

impl<G: GraphicsBackend + 'static> SurfaceTree<G> {
	pub fn new() -> Self {
		Self {
			nodes: Vec::new(),
			phantom: PhantomData,
		}
	}

	pub fn add_surface(&mut self, surface: Resource<WlSurface>) {
		self.nodes.push(Node::from(surface));
	}

	pub fn nodes_ascending(&self) -> impl Iterator<Item = &Node> {
		self.nodes.iter().map(|node| node)
	}

	pub fn nodes_descending(&self) -> impl Iterator<Item = &Node> {
		self.nodes_ascending().collect::<Vec<_>>().into_iter().rev()
	}
}

pub struct DumbWindowManagerBehavior<G: GraphicsBackend> {
	pub surface_tree: SurfaceTree<G>,
}

impl<G: GraphicsBackend + 'static> DumbWindowManagerBehavior<G> {
	pub fn new() -> Self {
		Self {
			surface_tree: SurfaceTree::new(),
		}
	}
}

impl<G: GraphicsBackend + 'static> WindowManagerBehavior<G> for DumbWindowManagerBehavior<G> {
	fn add_surface(&mut self, surface: Resource<WlSurface>) {
		let surface_data = surface.get_data::<RefCell<SurfaceData<G>>>().unwrap();
		if let Some(ref _role) = surface_data.borrow().role {
			// TODO: get the position and size from the role... unless you don't want to. it is dumb after all
			let position = Point::new((dumb_rand() % 200 + 50) as i32, (dumb_rand() % 200 + 50) as i32);
			let size = Size::new(500, 375);
			surface_data.borrow_mut().set_window_position(position);
			surface_data.borrow_mut().resize_window(size);
		} else {
			panic!("Can't add a surface without a role");
		}
		self.surface_tree.add_surface(surface);
	}

	fn handle_surface_resize(&mut self, _surface: Resource<WlSurface>, _new_size: Size) {
		log::warn!("Surface resize handling not implemented");
	}

	fn surfaces_ascending(&self) -> Box<dyn Iterator<Item = Resource<WlSurface>> + '_> {
		Box::new(self.surface_tree.nodes_ascending().map(|node| node.surface.clone()))
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
