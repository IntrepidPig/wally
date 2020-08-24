use std::{
	cell::{Cell, RefCell}
};

use crate::compositor::prelude::*;

impl<I: InputBackend, G: GraphicsBackend> CompositorState<I, G> {
	pub fn set_surface_active(&mut self, surface: Resource<WlSurface>) {
		let surface_data: Ref<RefCell<SurfaceData<G>>> = surface.get_user_data();
		let surface_data = surface_data.borrow();
		if let Some(ref role) = surface_data.role {
			match *role {
				Role::XdgSurface(ref xdg_surface) => self.set_xdg_surface_active(xdg_surface.clone()),
			}
		}
	}

	pub fn unset_surface_active(&mut self, surface: Resource<WlSurface>) {
		let surface_data: Ref<RefCell<SurfaceData<G>>> = surface.get_user_data();
		let surface_data = surface_data.borrow();
		if let Some(ref role) = surface_data.role {
			match *role {
				Role::XdgSurface(ref xdg_surface) => self.unset_xdg_surface_active(xdg_surface.clone()),
			}
		}
	}

	pub fn focus_surface_keyboard(&mut self, surface: Resource<WlSurface>) {
		let client = surface.client();
		let client = client.get().unwrap();
		let client_state = client.state::<RefCell<ClientState>>();
		
		for keyboard in &client_state.borrow().keyboards {
			self.send_keyboard_modifiers(keyboard.clone());
			let enter_event = wl_keyboard::EnterEvent {
				serial: get_input_serial(),
				surface: surface.clone(),
				keys: Vec::new(), // TODO: actual value
			};
			keyboard.send_event(WlKeyboardEvent::Enter(enter_event));
		}
		self.inner.keyboard_focus = Some(surface.clone());
	}

	pub fn unfocus_surface_keyboard(&mut self, surface: Resource<WlSurface>) {
		let client = surface.client();
		let client = client.get().unwrap();
		let client_state = client.state::<RefCell<ClientState>>();
		
		for keyboard in &client_state.borrow().keyboards {
			keyboard.send_event(WlKeyboardEvent::Leave(wl_keyboard::LeaveEvent {
				serial: get_input_serial(),
				surface: surface.clone(),
			}));
		}

		self.inner.keyboard_focus = None;
	}

	pub fn focus_surface_pointer(&mut self, surface: Resource<WlSurface>, point: Point) {
		let client = surface.client();
		let client = client.get().unwrap();
		let client_state = client.state::<RefCell<ClientState>>();
		
		for pointer in &client_state.borrow().pointers {
			pointer.send_event(WlPointerEvent::Enter(wl_pointer::EnterEvent {
				serial: get_input_serial(),
				surface: surface.clone(),
				surface_x: (point.x as f64).into(),
				surface_y: (point.y as f64).into(),
			}));
		}
		self.inner.pointer_focus = Some(surface)
	}

	pub fn unfocus_surface_pointer(&mut self, surface: Resource<WlSurface>) {
		let client = surface.client();
		let client = client.get().unwrap();
		let client_state = client.state::<RefCell<ClientState>>();

		for pointer in &client_state.borrow().pointers {
			pointer.send_event(WlPointerEvent::Leave(wl_pointer::LeaveEvent {
				serial: get_input_serial(),
				surface: surface.clone(),
			}));
		}
		self.inner.pointer_focus = None;
	}
}

#[derive(Debug)]
pub struct WindowManager<G: GraphicsBackend> {
	pub tree: SurfaceTree<G>,
}

impl<G: GraphicsBackend> WindowManager<G> {
	pub fn new() -> Self {
		Self {
			tree: SurfaceTree::new(),
		}
	}

	pub fn add_surface(&mut self, surface: Resource<WlSurface>) -> Handle<Node> {
		let position = Point::new((dumb_rand() % 200 + 50) as i32, (dumb_rand() % 200 + 50) as i32);
		self.tree.add_surface(surface, position)
	}

	pub fn remove_surface(&mut self, surface: Resource<WlSurface>) {
		self.tree.remove_surface(surface);
	}

	pub fn focus_surface(&mut self, surface: Resource<WlSurface>) {
		self.tree.focus_surface(surface);
		// TODO: bring to top
	}

	pub fn handle_surface_resize(&mut self, surface: Resource<WlSurface>, new_size: Size) {
		if let Some(node) = self.tree.find_surface(&surface) {
			node.size.set(Some(new_size));
		}
	}

	pub fn handle_surface_map(&mut self, surface: Resource<WlSurface>, new_size: Size) {
		if let Some(node) = self.tree.find_surface(&surface) {
			node.size.set(Some(new_size));
			node.draw.set(true);
		}
	}

	pub fn handle_surface_unmap(&mut self, surface: Resource<WlSurface>) {
		if let Some(node) = self.tree.find_surface(&surface) {
			node.draw.set(false);
		}
	}

	pub fn get_surface_under_point(&self, point: Point) -> Option<Ref<Node>> {
		self.tree.get_surface_under_point(point)
	}

	pub fn get_window_under_point(&self, point: Point) -> Option<Ref<Node>> {
		self.tree.get_window_under_point(point)
	}
}

#[derive(Debug)]
pub struct SurfaceTree<G: GraphicsBackend + ?Sized> {
	pub(crate) nodes: Vec<Owner<Node>>,
	phantom: PhantomData<G>,
}

impl<G: GraphicsBackend> SurfaceTree<G> {
	pub fn new() -> Self {
		Self {
			nodes: Vec::new(),
			phantom: PhantomData,
		}
	}

	pub fn add_surface(&mut self, surface: Resource<WlSurface>, position: Point) -> Handle<Node> {
		// TODO: ensure surface isn't already added
		let surface_data: Ref<RefCell<SurfaceData<G>>> = surface.get_user_data();
		let surface_size = surface_data.borrow().buffer_size;
		let node = Owner::new(Node::new(surface, position, surface_size, true));
		let handle = node.handle();
		self.nodes.push(node);
		handle
	}

	pub fn remove_surface(&mut self, surface: Resource<WlSurface>) {
		if let Some(position) = self.nodes.iter().position(|node| node.surface.borrow().is(&surface)) {
			self.nodes.remove(position);
		}
	}

	pub fn focus_surface(&mut self, surface: Resource<WlSurface>) {
		for node in &mut self.nodes {
			node.focused.set(node.surface.borrow().is(&surface));
		}
	}

	fn get_surface_under_point(&self, point: Point) -> Option<Ref<Node>> {
		for node in self.nodes_descending() {
			if let Some(geometry) = node.geometry() {
				if geometry.contains_point(point) {
					return Some(node.clone())
				}
			}
		}
		None
	}

	fn get_window_under_point(&self, point: Point) -> Option<Ref<Node>> {
		for node in self.nodes_descending() {
			if let Some(mut geometry) = node.geometry() {
				let surface = node.surface.borrow();
				let surface_data: Ref<RefCell<SurfaceData<G>>> = surface.get_user_data();
				if let Some(mut window_geometry) = surface_data.borrow().get_solid_window_geometry() {
					window_geometry.x += point.x;
					window_geometry.y += point.y;
					geometry = window_geometry;
				};
				if geometry.contains_point(point) {
					return Some(node.clone())
				}
			}
		}

		None
	}

	pub fn find<F: Fn(&Owner<Node>) -> bool>(&self, f: F) -> Option<Ref<Node>> {
		self.nodes.iter().find(|node| f(node)).map(|node| node.custom_ref())
	}

	pub fn find_surface(&self, surface: &Resource<WlSurface>) -> Option<Ref<Node>> {
		self.find(|node| node.surface.borrow().is(surface))
	}

	pub fn nodes_ascending(&self) -> impl Iterator<Item = Ref<Node>> {
		self.nodes.iter().map(|node| node.custom_ref())
	}

	pub fn nodes_descending(&self) -> impl Iterator<Item = Ref<Node>> {
		self.nodes.iter().rev().map(|node| node.custom_ref())
	}
}

#[derive(Debug, Clone)]
pub struct Node {
	pub surface: RefCell<Resource<WlSurface>>,
	pub position: Cell<Point>,
	pub size: Cell<Option<Size>>,
	pub draw: Cell<bool>,
	pub focused: Cell<bool>,
}

impl Node {
	pub fn new(surface: Resource<WlSurface>, position: Point, size: Option<Size>, draw: bool) -> Self {
		Self {
			surface: RefCell::new(surface),
			position: Cell::new(position),
			size: Cell::new(size),
			draw: Cell::new(draw),
			focused: Cell::new(false),
		}
	}

	pub fn geometry(&self) -> Option<Rect> {
		self.size.get().map(|size| Rect::from(self.position.get(), size))
	}
}

fn dumb_rand() -> u32 {
	let mut x = std::time::SystemTime::UNIX_EPOCH.elapsed().unwrap().subsec_nanos();
	x ^= x << 13;
	x ^= x >> 17;
	x ^= x << 5;
	x
}
