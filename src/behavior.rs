use std::{
	cell::{Cell, RefCell}
};

use crate::compositor::prelude::*;

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
		let surface_data: Ref<RefCell<SurfaceData<G>>> = surface.get_user_data();
		let surface_size = surface_data.borrow().buffer_size;
		let owner = Owner::new(Node::new(surface, position, surface_size, true));
		let handle = owner.handle();
		self.nodes.push(owner);
		handle
	}

	pub fn remove_surface(&mut self, surface: Resource<WlSurface>) {
		if let Some(position) = self.nodes.iter().position(|node| node.surface.borrow().is(&surface)) {
			self.nodes.remove(position);
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
}

impl Node {
	pub fn new(surface: Resource<WlSurface>, position: Point, size: Option<Size>, draw: bool) -> Self {
		Self {
			surface: RefCell::new(surface),
			position: Cell::new(position),
			size: Cell::new(size),
			draw: Cell::new(draw),
		}
	}

	pub fn geometry(&self) -> Option<Rect> {
		self.size.get().map(|size| Rect::from(self.position.get(), size))
	}
}

pub struct DumbWindowManagerBehavior<G: GraphicsBackend> {
	pub surface_tree: SurfaceTree<G>,
}

impl<G: GraphicsBackend> DumbWindowManagerBehavior<G> {
	pub fn new() -> Self {
		Self {
			surface_tree: SurfaceTree::new(),
		}
	}
}

fn dumb_rand() -> u32 {
	let mut x = std::time::SystemTime::UNIX_EPOCH.elapsed().unwrap().subsec_nanos();
	x ^= x << 13;
	x ^= x >> 17;
	x ^= x << 5;
	x
}
