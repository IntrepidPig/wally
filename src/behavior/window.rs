use std::{
	cell::{Cell, RefCell}
};

use crate::compositor::prelude::*;

impl<I: InputBackend, G: GraphicsBackend> CompositorState<I, G> {
	pub fn set_surface_active(&mut self, surface: Resource<WlSurface, SurfaceData<G>>) {
		let surface_data = surface.get_data();
		let surface_data = surface_data.inner.borrow();
		if let Some(ref role) = surface_data.role {
			match *role {
				Role::XdgSurface(ref xdg_surface) => self.set_xdg_surface_active(xdg_surface.clone()),
			}
		}
	}

	pub fn unset_surface_active(&mut self, surface: Resource<WlSurface, SurfaceData<G>>) {
		let surface_data = surface.get_data();
		let surface_data = surface_data.inner.borrow();
		if let Some(ref role) = surface_data.role {
			match *role {
				Role::XdgSurface(ref xdg_surface) => self.unset_xdg_surface_active(xdg_surface.clone()),
			}
		}
	}

	pub fn focus_surface_keyboard(&mut self, surface: Resource<WlSurface, SurfaceData<G>>) {
		let client = surface.client();
		let client = client.get().unwrap();
		let client_state = client.state::<RefCell<ClientState<G>>>();
		
		if let Some(ref seat) = client_state.borrow().seat {
			let seat_data = seat.get_data();
			let seat_data = seat_data.inner.borrow();
			if let Some(ref keyboard) = seat_data.keyboard {
				let serial = seat_data.next_keyboard_serial();
				self.inner.keyboard_state.send_current_keyboard_modifiers(keyboard.clone(), serial);
				keyboard.send_event(WlKeyboardEvent::Enter(wl_keyboard::EnterEvent {
					serial: serial.as_u32(),
					surface: surface.to_untyped(),
					keys: Vec::new(), // TODO: actual value
				}));
			}
		}

		self.inner.keyboard_focus = Some(surface.clone());
	}

	pub fn unfocus_surface_keyboard(&mut self, surface: Resource<WlSurface, SurfaceData<G>>) {
		let client = surface.client();
		let client = client.get().unwrap();
		let client_state = client.state::<RefCell<ClientState<G>>>();
		
		if let Some(ref seat) = client_state.borrow().seat {
			let seat_data = seat.get_data();
			let seat_data = seat_data.inner.borrow();
			if let Some(ref keyboard) = seat_data.keyboard {
				keyboard.send_event(WlKeyboardEvent::Leave(wl_keyboard::LeaveEvent {
					serial: seat_data.next_keyboard_serial().as_u32(),
					surface: surface.to_untyped(),
				}));
			}
		}

		self.inner.keyboard_focus = None;
	}

	pub fn focus_surface_pointer(&mut self, surface: Resource<WlSurface, SurfaceData<G>>, point: Point) {
		let client = surface.client();
		let client = client.get().unwrap();
		let client_state = client.state::<RefCell<ClientState<G>>>();
		
		if let Some(ref seat) = client_state.borrow().seat {
			let seat_data = seat.get_data();
			let seat_data = seat_data.inner.borrow();
			if let Some(ref pointer) = seat_data.pointer {
				pointer.send_event(WlPointerEvent::Enter(wl_pointer::EnterEvent {
					serial: seat_data.next_pointer_serial().as_u32(),
					surface: surface.to_untyped(),
					surface_x: (point.x as f64).into(),
					surface_y: (point.y as f64).into(),
				}));
			}
		}

		self.inner.pointer_focus = Some(surface)
	}

	pub fn unfocus_surface_pointer(&mut self, surface: Resource<WlSurface, SurfaceData<G>>) {
		let client = surface.client();
		let client = client.get().unwrap();
		let client_state = client.state::<RefCell<ClientState<G>>>();

		if let Some(ref seat) = client_state.borrow().seat {
			let seat_data = seat.get_data();
			let seat_data = seat_data.inner.borrow();
			if let Some(ref pointer) = seat_data.pointer {
				pointer.send_event(WlPointerEvent::Leave(wl_pointer::LeaveEvent {
					serial: seat_data.next_pointer_serial().as_u32(),
					surface: surface.to_untyped(),
				}));
			}
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

	pub fn add_surface(&mut self, surface: Resource<WlSurface, SurfaceData<G>>) -> Handle<Node<G>> {
		let position = Point::new((dumb_rand() % 200 + 50) as i32, (dumb_rand() % 200 + 50) as i32);
		self.tree.add_window(surface, position)
	}

	pub fn remove_surface(&mut self, surface: Resource<WlSurface, SurfaceData<G>>) {
		self.tree.remove_surface(surface);
	}

	pub fn focus_surface(&mut self, surface: Resource<WlSurface, SurfaceData<G>>) {
		self.tree.focus_surface(surface);
		// TODO: bring to top
	}

	pub fn handle_surface_resize(&mut self, surface: Resource<WlSurface, SurfaceData<G>>, new_size: Size) {
		let surface_data = surface.get_data();
		let window_size = surface_data.inner.borrow().get_solid_window_geometry().map(|geometry| geometry.size()).unwrap_or(new_size);
		if let Some(node) = self.tree.find_surface(&surface) {
			node.size.set(Some(window_size));
		}
	}

	pub fn handle_surface_map(&mut self, surface: Resource<WlSurface, SurfaceData<G>>, new_size: Size) {
		let surface_data = surface.get_data();
		let window_size = surface_data.inner.borrow().get_solid_window_geometry().map(|geometry| geometry.size()).unwrap_or(new_size);
		if let Some(node) = self.tree.find_surface(&surface) {
			node.size.set(Some(window_size));
			node.draw.set(true);
		}
	}

	pub fn handle_surface_unmap(&mut self, surface: Resource<WlSurface, SurfaceData<G>>) {
		if let Some(node) = self.tree.find_surface(&surface) {
			node.draw.set(false);
		}
	}

	pub fn get_surface_under_point(&self, point: Point) -> Option<Ref<Node<G>>> {
		self.tree.get_surface_under_point(point)
	}

	pub fn get_window_under_point(&self, point: Point) -> Option<Ref<Node<G>>> {
		self.tree.get_window_under_point(point)
	}
}

#[derive(Debug)]
pub struct SurfaceTree<G: GraphicsBackend + ?Sized> {
	pub(crate) nodes: Vec<Owner<Node<G>>>,
	phantom: PhantomData<G>,
}

impl<G: GraphicsBackend> SurfaceTree<G> {
	pub fn new() -> Self {
		Self {
			nodes: Vec::new(),
			phantom: PhantomData,
		}
	}

	pub fn add_window(&mut self, surface: Resource<WlSurface, SurfaceData<G>>, mut position: Point) -> Handle<Node<G>> {
		// TODO: ensure surface isn't already added
		let surface_data = surface.get_data();
		let mut size = surface_data.inner.borrow().buffer_size;
		if let Some(window_geometry) = surface_data.inner.borrow().get_solid_window_geometry() {
			position.x += window_geometry.x;
			position.y += window_geometry.y;
			size = Some(window_geometry.size());
		}
		let node = Owner::new(Node::new(surface, position, size, true));
		let handle = node.handle();
		self.nodes.push(node);
		handle
	}

	pub fn remove_surface(&mut self, surface: Resource<WlSurface, SurfaceData<G>>) {
		if let Some(position) = self.nodes.iter().position(|node| node.surface.borrow().is(&surface)) {
			self.nodes.remove(position);
		}
	}

	pub fn focus_surface(&mut self, surface: Resource<WlSurface, SurfaceData<G>>) {
		for node in &mut self.nodes {
			node.focused.set(node.surface.borrow().is(&surface));
		}
	}

	fn get_surface_under_point(&self, point: Point) -> Option<Ref<Node<G>>> {
		for node in self.nodes_descending() {
			if let Some(geometry) = node.node_surface_geometry() {
				if geometry.contains_point(point) {
					return Some(node.clone())
				}
			}
		}
		None
	}

	fn get_window_under_point(&self, point: Point) -> Option<Ref<Node<G>>> {
		for node in self.nodes_descending() {
			if let Some(geometry) = node.node_geometry() {
				if geometry.contains_point(point) {
					return Some(node.clone())
				}
			}
		}

		None
	}

	pub fn find<F: Fn(&Owner<Node<G>>) -> bool>(&self, f: F) -> Option<Ref<Node<G>>> {
		self.nodes.iter().find(|node| f(node)).map(|node| node.custom_ref())
	}

	pub fn find_surface(&self, surface: &Resource<WlSurface, SurfaceData<G>>) -> Option<Ref<Node<G>>> {
		self.find(|node| node.surface.borrow().is(surface))
	}

	pub fn nodes_ascending(&self) -> impl Iterator<Item = Ref<Node<G>>> {
		self.nodes.iter().map(|node| node.custom_ref())
	}

	pub fn nodes_descending(&self) -> impl Iterator<Item = Ref<Node<G>>> {
		self.nodes.iter().rev().map(|node| node.custom_ref())
	}
}

#[derive(Debug, Clone)]
pub struct Node<G: GraphicsBackend> {
	pub surface: RefCell<Resource<WlSurface, SurfaceData<G>>>,
	pub position: Cell<Point>,
	pub size: Cell<Option<Size>>,
	pub draw: Cell<bool>,
	pub focused: Cell<bool>,
	_phantom: PhantomData<G>,
}

impl<G: GraphicsBackend> Node<G> {
	pub fn new(surface: Resource<WlSurface, SurfaceData<G>>, position: Point, size: Option<Size>, draw: bool) -> Self {
		Self {
			surface: RefCell::new(surface),
			position: Cell::new(position),
			size: Cell::new(size),
			draw: Cell::new(draw),
			focused: Cell::new(false),
			_phantom: PhantomData,
		}
	}

	pub fn node_position(&self) -> Point {
		self.position.get()
	}

	pub fn node_geometry(&self) -> Option<Rect> {
		self.size.get().map(|size| Rect::from(self.position.get(), size))
	}

	pub fn node_size(&self) -> Option<Size> {
		self.size.get()
	}

	/// Returns the geometry of this node's surface scaled to match the surface window to the node.
	/// Returns none if the node or the surface has no size.
	pub fn node_surface_geometry(&self) -> Option<Rect> {
		let surface = self.surface.borrow();
		let surface_data = surface.get_data();
		let surface_data = surface_data.inner.borrow();
		let surface_window_geometry = surface_data.get_solid_window_geometry();
		match (self.node_geometry(), surface_data.get_surface_size()) {
			(Some(node_geometry), Some(surface_size)) => {
				let surface_window_geometry = surface_window_geometry.unwrap_or(Rect::new(0, 0, surface_size.width, surface_size.height));
				let (node_surface_x, node_surface_width) = calc_outer_bounds(
					node_geometry.x as f32,
					node_geometry.width as f32,
					surface_window_geometry.x as f32,
					surface_window_geometry.width as f32,
					surface_size.width as f32,
				);
				let (node_surface_x, node_surface_width) = (node_surface_x.round() as i32, node_surface_width.round() as u32);
				let (node_surface_y, node_surface_height) = calc_outer_bounds(
					node_geometry.y as f32,
					node_geometry.height as f32,
					surface_window_geometry.y as f32,
					surface_window_geometry.height as f32,
					surface_size.height as f32,
				);
				let (node_surface_y, node_surface_height) = (node_surface_y.round() as i32, node_surface_height.round() as u32);
				Some(Rect::new(node_surface_x, node_surface_y, node_surface_width, node_surface_height))
			}
			_ => None,
		}
	}

	/// Translate a point from node-surface coordinates to surface coordinates.
	/// Returns none if the node or the surface has no size.
	pub fn node_surface_point_to_surface_point(&self, node_point: Point) -> Option<Point> {
		let surface = self.surface.borrow();
		let surface_data = surface.get_data();
		let surface_data = surface_data.inner.borrow();
		match (self.node_surface_geometry(), surface_data.get_surface_size()) {
			(Some(node_surface_geometry), Some(surface_size)) => {
				let cx = surface_size.width as f32 / node_surface_geometry.width as f32;
				let cy = surface_size.height as f32 / node_surface_geometry.height as f32;
				Some(node_point.scale(cx, cy))
			},
			_ => None,
		}
	}
}

// Calculates the surface size and position of a node by by scaling the actual surface size by
// the ratio of the surface size to the node size and 
// ## Parameters
// nq: node coordinate
// nv: node size
// wq: window coordinate
// wv: window size
// sv: surface size
// ## Returns
// nsq: node surface coordinate
// nsv: node surface size
fn calc_outer_bounds(nq: f32, nv: f32, wq: f32, wv: f32, sv: f32) -> (f32, f32) {
	let c = nv / wv;
	let nsq = nq - wq * c;
	let nsv = sv * c;
	(nsq, nsv)
}

fn dumb_rand() -> u32 {
	let mut x = std::time::SystemTime::UNIX_EPOCH.elapsed().unwrap().subsec_nanos();
	x ^= x << 13;
	x ^= x >> 17;
	x ^= x << 5;
	x
}
