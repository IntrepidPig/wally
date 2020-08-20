use std::os::unix::io::RawFd;

// TODO remove this festus dependency
use festus::{geometry::*, math::*};
use thiserror::Error;
use wayland_server::protocol::*;

use crate::{
	backend::{GraphicsBackend, RgbaInfo, Vertex},
	compositor::{prelude::*, surface::SurfaceData},
};

#[derive(Debug)]
pub struct Output<G: GraphicsBackend> {
	handle: G::OutputHandle,
	render_target_handle: G::RenderTargetHandle,
	pub viewport: Rect,
}

// Deriving this doesn't work for some reason
impl<G: GraphicsBackend> Clone for Output<G> {
	fn clone(&self) -> Self {
		Self {
			handle: self.handle,
			render_target_handle: self.render_target_handle,
			viewport: self.viewport,
		}
	}
}
impl<G: GraphicsBackend> Copy for Output<G> {}

pub struct Renderer<G: GraphicsBackend> {
	// TODO not pub, b/c soundness (this should be fixable once output infrastructure is in place)
	pub(crate) backend: G,
	// TODO: reorganize this to prevent cloning of this all the time to avoid borrow check issues
	outputs: Vec<Output<G>>,
	// This should always be some, and is only optional for initialization purposes
	cursor_plane: Option<Plane<G>>,
}

impl<G: GraphicsBackend> Renderer<G> {
	pub fn init(mut backend: G) -> Result<Self, G::Error> {
		// Create a render target for each output, placing each new viewport next horizontally
		let mut current_width = 0;
		let outputs = backend
			.get_current_outputs()
			.into_iter()
			.map(|handle| {
				let info = backend.get_output_info(handle)?;
				let x = current_width as i32;
				current_width += info.size.width;
				let render_target_handle = backend.create_render_target(info.size)?;
				let viewport = Rect::new(x, 0, info.size.width, info.size.height);
				let output = Output {
					handle,
					render_target_handle,
					viewport,
				};
				Ok(output)
			})
			.collect::<Result<Vec<_>, _>>()?;

		let mut renderer = Self {
			backend,
			outputs,
			cursor_plane: None,
		};

		// Load the cursor image
		let cursor_image_path = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/cursor_0.png");
		let load_image = image::open(cursor_image_path)
			.map_err(|e| log::error!("Failed to open image at path '{}': {}", cursor_image_path, e))
			.unwrap();
		let image_rgba = load_image.into_rgba();
		let dims = image_rgba.dimensions();
		let image_data = image_rgba.into_raw();
		let cursor_plane = renderer.create_plane_from_rgba(
			Rect::new(0, 0, 24, 24),
			RgbaInfo {
				width: dims.0,
				height: dims.1,
				data: &image_data,
			},
		)?;
		renderer.cursor_plane = Some(cursor_plane);
		Ok(renderer)
	}

	pub fn update(&mut self) -> Result<(), G::Error> {
		self.backend.update()
	}

	pub fn create_shm_pool(&mut self, fd: RawFd, size: usize) -> Result<G::ShmPool, G::Error> {
		self.backend.create_shm_pool(fd, size)
	}

	pub fn resize_shm_pool(&mut self, shm_pool: &mut G::ShmPool, new_size: usize) -> Result<(), G::Error> {
		self.backend.resize_shm_pool(shm_pool, new_size)
	}

	pub fn create_shm_buffer(
		&mut self,
		shm_pool: &mut G::ShmPool,
		offset: usize,
		width: u32,
		height: u32,
		stride: u32,
		format: wl_shm::Format,
	) -> Result<G::ShmBuffer, G::Error> {
		self.backend
			.create_shm_buffer(shm_pool, offset, width, height, stride, format)
	}

	pub fn create_plane_with_texture(
		&mut self,
		geometry: Rect,
		texture_handle: G::TextureHandle,
	) -> Result<Plane<G>, G::Error> {
		let vertices = &[
			Vertex {
				pos: [0.0, 0.0, 0.0],
				uv: [0.0, 0.0],
			},
			Vertex {
				pos: [1.0, 0.0, 0.0],
				uv: [1.0, 0.0],
			},
			Vertex {
				pos: [0.0, 1.0, 0.0],
				uv: [0.0, 1.0],
			},
			Vertex {
				pos: [1.0, 1.0, 0.0],
				uv: [1.0, 1.0],
			},
		];
		let indices = &[0, 1, 2, 1, 2, 3];
		let vertex_buffer_handle = self.backend.create_vertex_buffer(vertices, indices)?;
		// Use a dummy view size since it will be overwritten before drawing anyway
		let mvp_buffer_handle = self
			.backend
			.create_mvp_buffer(self.create_mvp(Size::new(1, 1), geometry))?;
		let plane = Plane {
			vertex_buffer_handle,
			mvp_buffer_handle,
			texture_handle,
		};
		Ok(plane)
	}

	fn create_mvp(&self, view_size: Size, geometry: Rect) -> [[[f32; 4]; 4]; 3] {
		let pos = Point2::from(geometry.point());
		let size = Vec2::from(geometry.size());
		let view_size = Vec2::from(view_size);

		let scale = Mat4::new_nonuniform_scaling(&Vec3::new(size.x, size.y, 1.0));
		let model = nalgebra::Isometry3::translation(pos.x, pos.y, 0.0).to_homogeneous() * scale;

		let eye = Point3::new(0.0, 0.0, 0.0);
		let target = Point3::new(0.0, 0.0, 1.0);
		let view = nalgebra::Isometry3::look_at_lh(&eye, &target, &Vec3::y());

		let projection = nalgebra::Orthographic3::new(0.0, view_size.x, 0.0, view_size.y, -1.0, 1.0);

		let mvp = festus::renderer::Mvp {
			model: model,
			view: view.to_homogeneous(),
			projection: *projection.as_matrix(),
		};

		mvp.into()
	}

	/// Create a new plane positioned at `Point` from the given Rgba data
	pub fn create_plane_from_rgba(&mut self, geometry: Rect, rgba: RgbaInfo) -> Result<Plane<G>, G::Error> {
		let texture_handle = self.backend.create_texture_from_rgba(rgba)?;
		self.create_plane_with_texture(geometry, texture_handle)
	}

	pub fn create_surface_renderer_data(&mut self) -> Result<SurfaceRendererData<G>, G::Error> {
		Ok(SurfaceRendererData { plane: None })
	}

	// TODO: handle other sorts of buffers (DMA buffers!)
	pub fn create_texture_from_wl_buffer(
		&mut self,
		wl_buffer: wl_buffer::WlBuffer,
	) -> Result<G::TextureHandle, G::Error> {
		let buffer_data = wl_buffer.get_synced::<G::ShmBuffer>();
		let buffer_data_lock = &mut *buffer_data.lock().unwrap();
		let texture_handle = self.backend.create_texture_from_shm_buffer(buffer_data_lock)?;
		Ok(texture_handle)
	}

	pub fn outputs(&self) -> Vec<Output<G>> {
		self.outputs.clone()
	}

	pub fn render_scene<'a, F: Fn(SceneRenderState<G>) -> Result<(), G::Error>>(
		&'a mut self,
		f: F,
	) -> Result<(), G::Error> {
		for output in self.outputs.clone() {
			unsafe {
				self.backend.begin_render_pass(output.render_target_handle)?;
				let scene_render_state = SceneRenderState { renderer: self };
				// TODO: This should really be an FnOnce and not be called in a loop
				f(scene_render_state)?;
				self.backend.end_render_pass(output.render_target_handle)?;
			}
		}

		Ok(())
	}

	pub fn present(&mut self) -> Result<(), G::Error> {
		for output in self.outputs.clone() {
			let render_target_handle = output.render_target_handle;
			self.backend.present_target(output.handle, render_target_handle)?;
		}
		Ok(())
	}

	pub fn destroy_vertex_buffer(&mut self, handle: G::VertexBufferHandle) -> Result<(), G::Error> {
		self.backend.destroy_vertex_buffer(handle)
	}

	pub fn destroy_mvp_buffer(&mut self, handle: G::MvpBufferHandle) -> Result<(), G::Error> {
		self.backend.destroy_mvp_buffer(handle)
	}

	pub fn destroy_texture(&mut self, handle: G::TextureHandle) -> Result<(), G::Error> {
		self.backend.destroy_texture(handle)
	}

	pub fn destroy_render_target(&mut self, handle: G::RenderTargetHandle) -> Result<(), G::Error> {
		self.backend.destroy_render_target(handle)
	}

	pub fn destroy_plane(&mut self, plane: Plane<G>) -> Result<(), G::Error> {
		self.destroy_vertex_buffer(plane.vertex_buffer_handle)?;
		self.destroy_mvp_buffer(plane.mvp_buffer_handle)?;
		self.destroy_texture(plane.texture_handle)?;
		Ok(())
	}

	pub fn destroy_surface_renderer_data(
		&mut self,
		surface_renderer_data: SurfaceRendererData<G>,
	) -> Result<(), G::Error> {
		if let Some(plane) = surface_renderer_data.plane {
			self.destroy_plane(plane)?;
		}
		Ok(())
	}
}

/// A `Plane` represents a textured rectangle that can be drawn on a render target. It consists of a
/// vertex buffer, an MVP (uniform) buffer, and a texture. In the future, the vertex buffer should be
/// moved to be stored as a singleton in the renderer instance because it is never modified, and all
/// manipulation of the drawing is done through the MVP buffer and the texture.
pub struct Plane<G: GraphicsBackend> {
	vertex_buffer_handle: G::VertexBufferHandle,
	mvp_buffer_handle: G::MvpBufferHandle,
	texture_handle: G::TextureHandle,
}

pub struct SurfaceRendererData<G: GraphicsBackend> {
	pub plane: Option<Plane<G>>,
}

/// SceneRenderState represents an in progress draw call.
pub struct SceneRenderState<'a, G: GraphicsBackend> {
	// TODO! make this private and make all required methods available through a SceneRenderState impl, thus
	// making this interface sound. Right now it is UNSOUND!! (like a lot of other things in this crate)
	pub renderer: &'a mut Renderer<G>,
}

impl<'a, G: GraphicsBackend + 'static> SceneRenderState<'a, G> {
	pub fn draw(
		&mut self,
		vertex_buffer: G::VertexBufferHandle,
		texture: G::TextureHandle,
		mvp: G::MvpBufferHandle,
	) -> Result<(), G::Error> {
		unsafe {
			self.renderer.backend.draw(vertex_buffer, texture, mvp)?;
		}
		Ok(())
	}

	/// Draw a surface on
	pub fn draw_surface(&mut self, surface: wl_surface::WlSurface) -> Result<(), G::Error> {
		let surface_data = surface.get_synced::<SurfaceData<G>>();
		let surface_data_lock = &mut *surface_data.lock().unwrap();

		// If the surface has been committed a buffer that hasn't been uploaded to the graphics
		// backend yet, do that now.
		// TODO: don't ignore the buffer/texture offset
		if let Some(committed_buffer) = surface_data_lock.committed_buffer.take() {
			let texture = self
				.renderer
				.create_texture_from_wl_buffer(committed_buffer.clone().0)
				.unwrap();
			if let Some(ref mut renderer_data) = surface_data_lock.renderer_data {
				if let Some(ref mut plane) = renderer_data.plane {
					let old_texture = std::mem::replace(&mut plane.texture_handle, texture);
					self.renderer.destroy_texture(old_texture)?;
				} else {
					// Use a dummy value for the geometry because it will be overwritten before drawing TODO clean this up?
					let plane = self
						.renderer
						.create_plane_with_texture(Rect::new(0, 0, 1, 1), texture)?;
					renderer_data.plane = Some(plane);
				}
			} else {
				panic!("Tried to draw a surface whose renderer data has been destroyed");
			}
			committed_buffer.0.release();
		}

		// If the surface has known geometry and a plane ready for drawing, write the geometry data to the surfaces MVP buffer and draw the surface
		let surface_geometry_opt = surface_data_lock.try_get_surface_geometry();
		if let Some(ref mut plane) = surface_data_lock
			.renderer_data
			.as_mut()
			.and_then(|renderer_data| renderer_data.plane.as_mut())
		{
			if let Some(surface_geometry) = surface_geometry_opt {
				for output in self.renderer.outputs.clone() {
					if let Some(output_local_point) = get_local_coordinates(output.viewport, surface_geometry) {
						let mut output_local_geometry = surface_geometry;
						output_local_geometry.x = output_local_point.x;
						output_local_geometry.y = output_local_point.y;
						let mvp = self.renderer.create_mvp(output.viewport.size(), output_local_geometry);
						self.renderer
							.backend
							.map_mvp_buffer(plane.mvp_buffer_handle)
							.map(|mvp_map| *mvp_map = mvp);
						self.draw(
							plane.vertex_buffer_handle,
							plane.texture_handle,
							plane.mvp_buffer_handle,
						)?;
					}
				}
			}
		}

		surface_data_lock
			.callback
			.take()
			.map(|callback| callback.done(crate::compositor::get_input_serial()));

		Ok(())
	}

	pub fn draw_cursor(&mut self, position: Point) -> Result<(), G::Error> {
		// TODO: nah
		const CURSOR_WIDTH: u32 = 24;
		const CURSOR_HEIGHT: u32 = 24;
		const CURSOR_HOTSPOT_X: i32 = 4;
		const CURSOR_HOTSPOT_Y: i32 = 4;
		let cursor_rect = Rect::new(
			position.x - CURSOR_HOTSPOT_X,
			position.y - CURSOR_HOTSPOT_Y,
			CURSOR_WIDTH,
			CURSOR_HEIGHT,
		);

		for output in self.renderer.outputs.clone() {
			if let Some(output_local_coordinates) = get_local_coordinates(output.viewport, cursor_rect) {
				let output_local_rect = Rect::new(
					output_local_coordinates.x,
					output_local_coordinates.y,
					cursor_rect.width,
					cursor_rect.height,
				);
				let mvp = self.renderer.create_mvp(output.viewport.size(), output_local_rect);
				// I wrote this at 12:34 AM
				if let Some((vertex_buffer_handle, texture_handle, mvp_buffer_handle)) =
					if let Some(ref cursor_plane) = self.renderer.cursor_plane {
						let mvp_map = self
							.renderer
							.backend
							.map_mvp_buffer(cursor_plane.mvp_buffer_handle)
							.unwrap();
						*mvp_map = mvp;
						Some((
							cursor_plane.vertex_buffer_handle,
							cursor_plane.texture_handle,
							cursor_plane.mvp_buffer_handle,
						))
					} else {
						None
					} {
					self.draw(vertex_buffer_handle, texture_handle, mvp_buffer_handle)?;
				}
			}
		}

		Ok(())
	}
}

fn get_local_coordinates(viewport: Rect, rect: Rect) -> Option<Point> {
	if rect.intersects(viewport) {
		Some(Point::new(rect.x - viewport.x, rect.y - viewport.y))
	} else {
		None
	}
}

#[derive(Debug, Error)]
pub enum RendererError<G: GraphicsBackend + 'static>
where
	Self: From<G::Error>,
{
	#[error("An error occurred in the graphics backend")]
	GraphicsBackendError(#[source] G::Error),
}
