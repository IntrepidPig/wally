use crate::math::*;

#[derive(Debug, Clone, Copy)]
pub struct Point {
	pub x: i32,
	pub y: i32,
}

impl Point {
	pub fn new(x: i32, y: i32) -> Self {
		Self { x, y }
	}
}

impl From<Point> for Point2 {
	fn from(t: Point) -> Self {
		Point2::new(t.x as f32, t.y as f32)
	}
}

impl From<Point> for Vec2 {
	fn from(t: Point) -> Self {
		Vec2::new(t.x as f32, t.y as f32)
	}
}

#[derive(Debug, Clone, Copy)]
pub struct Size {
	pub width: u32,
	pub height: u32,
}

impl Size {
	pub fn new(width: u32, height: u32) -> Self {
		Self { width, height }
	}
}

impl From<Size> for Vec2 {
	fn from(t: Size) -> Self {
		Vec2::new(t.width as f32, t.height as f32)
	}
}

#[derive(Debug, Clone, Copy)]
pub struct Rect {
	pub x: i32,
	pub y: i32,
	pub width: u32,
	pub height: u32,
}

impl Rect {
	pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
		Self { x, y, width, height }
	}

	pub fn point(self) -> Point {
		Point { x: self.x, y: self.y }
	}

	pub fn size(self) -> Size {
		Size {
			width: self.width,
			height: self.height,
		}
	}

	pub fn contains_point(self, point: Point) -> bool {
		point.x >= self.x && point.y >= self.y && point.x <= self.x + self.width as i32 && point.y <= self.y + self.height as i32
	}
}

impl From<(Point, Size)> for Rect {
	fn from(t: (Point, Size)) -> Self {
		Rect {
			x: t.0.x,
			y: t.0.y,
			width: t.1.width,
			height: t.1.height,
		}
	}
}