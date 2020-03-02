use std::{
	fmt,
	path::Path,
	sync::{Arc, Mutex},
};

use ash::{version::DeviceV1_0, vk, Device};
use thiserror::Error;
use wayland_server::protocol::*;

use crate::{
	backend::RenderBackend,
	compositor::{
		role::Role,
		shm::ShmBuffer,
		surface::{SurfaceData, SurfaceTree},
		xdg::XdgToplevelData,
	},
	renderer::{self, present::PresentBackend, Mvp, ObjectHandle, Renderer, TextureData, TextureSource},
};

pub struct VulkanRenderBackend<P: PresentBackend> {
	renderer: Renderer,
	present_backend: P,
	cursor: ObjectHandle,
}

impl<P: PresentBackend> VulkanRenderBackend<P> {
	pub fn new(mut renderer: Renderer, present_backend: P) -> Self {
		unsafe {
			let cursor_texture = TextureData::create_from_source(
				&renderer.device,
				renderer.queue,
				renderer.command_pool,
				renderer.device_memory_properties,
				ImagePathTextureSource::new(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/left_ptr_000.png")),
			)
			.unwrap();

			let cursor = renderer.create_object_with_texture(cursor_texture).unwrap();

			Self {
				renderer,
				present_backend,
				cursor,
			}
		}
	}
}

impl<P: PresentBackend> fmt::Debug for VulkanRenderBackend<P> {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct("VulkanRenderBackend")
			.field("renderer", &"<renderer>")
			.field("present_backend", &"<present_backend>")
			.field("cursor", &self.cursor)
			.finish()
	}
}

#[derive(Debug)]
pub struct VulkanSurfaceData {
	object_handle: ObjectHandle,
}

#[derive(Debug, Error)]
pub enum VulkanRenderBackendError {
	#[error("An unknown error occurred in the vulkan render backend")]
	Unknown,
}

impl<P: PresentBackend> RenderBackend for VulkanRenderBackend<P> {
	type Error = VulkanRenderBackendError;
	type ShmPool = ();
	type ObjectHandle = VulkanSurfaceData;

	fn update(&mut self) -> Result<(), Self::Error> {
		Ok(())
	}

	fn create_object(&mut self) -> Result<Self::ObjectHandle, Self::Error> {
		unsafe {
			let handle = self
				.renderer
				.create_object_with(|object| {
					let mvp = Mvp::new_surface((0, 0), (1, 1), (1, 1));
					object.update_mvp(mvp).unwrap();
				})
				.unwrap();
			let surface_data = Self::ObjectHandle { object_handle: handle };
			Ok(surface_data)
		}
	}

	fn destroy_object(&mut self, object_handle: Self::ObjectHandle) -> Result<(), Self::Error> {
		unsafe {
			self.renderer.delete_object(object_handle.object_handle);
		}

		Ok(())
	}

	fn render_tree(&mut self, tree: &SurfaceTree<Self>) -> Result<(), Self::Error> {
		unsafe {
			let device = &self.renderer.device.clone();
			let queue = self.renderer.queue;
			let command_pool = self.renderer.command_pool;
			let device_memory_properties = self.renderer.device_memory_properties;
			let sampler = self.renderer.sampler;
			let current_size = self.present_backend.get_current_size();
			let renderer = &mut self.renderer;
			let present_backend = &mut self.present_backend;
			renderer.begin_render_pass(present_backend).map_err(|_| VulkanRenderBackendError::Unknown)?;
			for surface in tree.surfaces_ascending() {
				let surface_data = surface
					.as_ref()
					.user_data()
					.get::<Arc<Mutex<SurfaceData<Self::ObjectHandle>>>>()
					.unwrap();
				let surface_data_lock = &mut *surface_data.lock().unwrap();
				let object_handle = surface_data_lock.renderer_data.as_ref().unwrap().object_handle;
				let object = match renderer.get_object_mut(object_handle) {
					Some(obj) => obj,
					None => {
						log::warn!("Vulkan object referenced by surface was destroyed");
						continue;
					}
				};
				if let Some(committed_buffer) = surface_data_lock.committed_buffer.take() {
					let buffer_data = committed_buffer
						.0
						.as_ref()
						.user_data()
						.get::<Arc<Mutex<ShmBuffer>>>()
						.unwrap();
					let buffer_data_lock = &mut *buffer_data.lock().unwrap();
					let texture = TextureData::create_from_source(
						&device,
						queue,
						command_pool,
						device_memory_properties,
						ShmBufferTextureSource::new(buffer_data_lock),
					)
					.unwrap();
					object.replace_texture(device, sampler, texture).unwrap();
					committed_buffer.0.release();
				}
				if let Some(role) = surface_data_lock.role.as_ref() {
					match role {
						Role::XdgToplevel(toplevel) => {
							let toplevel_data: &Arc<Mutex<XdgToplevelData>> = toplevel
								.as_ref()
								.user_data()
								.get::<Arc<Mutex<XdgToplevelData>>>()
								.unwrap();
							let toplevel_data_lock = toplevel_data.lock().unwrap();
							object
								.update_mvp(
									Mvp::new_surface(toplevel_data_lock.pos, toplevel_data_lock.size, current_size),
								)
								.unwrap();
							renderer.draw_object(object_handle).map_err(|_| VulkanRenderBackendError::Unknown)?;
						}
						Role::Cursor(pointer_state) => {
							let pointer_state = &mut *pointer_state.lock().unwrap();
							object
								.update_mvp(
									Mvp::new_surface(
										(pointer_state.pos.0 as i32, pointer_state.pos.1 as i32),
										(24, 24), // TODO
										current_size,
									),
								)
								.unwrap();
							renderer.draw_object(object_handle).map_err(|_| VulkanRenderBackendError::Unknown)?;
						}
					}
				}
				surface_data_lock.callback.as_ref().map(|callback| callback.done(42));
			}
			let pointer_state = tree.pointer.lock().unwrap();
			//let default_cursor_object = self.renderer.get_object_mut(pointer_state.default.object_handle).unwrap();
			let default_cursor_object = renderer.get_object_mut(self.cursor).unwrap();
			if pointer_state.custom_cursor.is_none() {
				default_cursor_object
					.update_mvp(
						Mvp::new_surface(
							(pointer_state.pos.0 as i32, pointer_state.pos.1 as i32),
							(24, 24), // TODO
							current_size,
						),
					)
					.unwrap();
				renderer.draw_object(self.cursor).map_err(|_| VulkanRenderBackendError::Unknown)?;
			}
			
			renderer.end_render_pass().map_err(|_| VulkanRenderBackendError::Unknown)?;
			renderer.submit_command_buffer().map_err(|_| VulkanRenderBackendError::Unknown)?;
			present_backend.present(renderer).map_err(|_e| {
				log::error!("Presenting failed");
				VulkanRenderBackendError::Unknown
			})?;
		}

		Ok(())
	}

	fn get_size(&self) -> (u32, u32) {
		unsafe { self.present_backend.get_current_size() }
	}
}

pub struct ShmBufferTextureSource<'a> {
	buffer: &'a ShmBuffer,
}

impl<'a> ShmBufferTextureSource<'a> {
	pub fn new(buffer: &'a ShmBuffer) -> Self {
		Self { buffer }
	}
}

impl<'a> renderer::TextureSource for ShmBufferTextureSource<'a> {
	unsafe fn create_texture(
		self,
		device: &Device,
		queue: vk::Queue,
		command_pool: vk::CommandPool,
		device_memory_properties: vk::PhysicalDeviceMemoryProperties,
	) -> Result<TextureData, ()> {
		let vk_format = wl_format_to_vk_format(self.buffer.format);
		let (slice, guard) = self.buffer.as_slice();
		log::debug!("Buffer size: {}", slice.len());
		let (staging_buffer, staging_buffer_memory) = renderer::make_buffer(
			device,
			device_memory_properties,
			slice,
			vk::BufferUsageFlags::TRANSFER_SRC,
		)?;
		drop(guard);
		let image = renderer::create_image(
			device,
			self.buffer.width as u32,
			self.buffer.height as u32,
			vk_format,
			vk::ImageTiling::LINEAR,
			vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
		)?;
		let image_memory_requirements = device.get_image_memory_requirements(image);
		log::debug!("Image memory size: {}", image_memory_requirements.size);
		let image_memory = renderer::allocate_memory(
			device,
			device_memory_properties,
			image_memory_requirements.size,
			vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
		)?;
		renderer::bind_image_memory(device, image, image_memory)?;
		renderer::transition_image_layout(
			device,
			queue,
			command_pool,
			image,
			vk::ImageLayout::UNDEFINED,
			vk::ImageLayout::TRANSFER_DST_OPTIMAL,
		)?;
		let buffer_image_copy = vk::BufferImageCopy {
			buffer_offset: 0,
			buffer_row_length: 0,
			buffer_image_height: 0,
			image_subresource: vk::ImageSubresourceLayers {
				aspect_mask: vk::ImageAspectFlags::COLOR,
				mip_level: 0,
				base_array_layer: 0,
				layer_count: 1,
			},
			image_offset: vk::Offset3D { x: 0, y: 0, z: 0 },
			image_extent: vk::Extent3D {
				width: self.buffer.width as u32,
				height: self.buffer.height as u32,
				depth: 1,
			},
		};
		renderer::record_submit_one_time_commands(device, queue, command_pool, |cmd_buf| {
			device.cmd_copy_buffer_to_image(
				cmd_buf,
				staging_buffer,
				image,
				vk::ImageLayout::TRANSFER_DST_OPTIMAL,
				&[buffer_image_copy],
			);
			Ok(())
		})?;
		renderer::transition_image_layout(
			device,
			queue,
			command_pool,
			image,
			vk::ImageLayout::TRANSFER_DST_OPTIMAL,
			vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
		)?;

		device.destroy_buffer(staging_buffer, None);
		device.free_memory(staging_buffer_memory, None);

		let image_view = renderer::create_image_view(device, image, vk_format, vk::ImageAspectFlags::COLOR)?;

		Ok(TextureData {
			image,
			image_view,
			image_memory,
		})
	}
}

pub struct ImagePathTextureSource<'a> {
	path: &'a Path,
}

impl<'a> ImagePathTextureSource<'a> {
	pub fn new<P: AsRef<Path> + ?Sized>(path: &'a P) -> Self {
		Self { path: path.as_ref() }
	}
}

impl<'a> TextureSource for ImagePathTextureSource<'a> {
	unsafe fn create_texture(
		self,
		device: &Device,
		queue: vk::Queue,
		command_pool: vk::CommandPool,
		device_memory_properties: vk::PhysicalDeviceMemoryProperties,
	) -> Result<TextureData, ()> {
		let vk_format = vk::Format::R8G8B8A8_UNORM;
		let load_image = image::open(self.path)
			.map_err(|e| log::error!("Failed to open image at path '{}': {}", self.path.display(), e))?;
		let image_rgba = load_image.into_rgba();
		let dims = image_rgba.dimensions();
		let image_data = image_rgba.into_raw();
		let (staging_buffer, staging_buffer_memory) = renderer::make_buffer(
			device,
			device_memory_properties,
			image_data.as_slice(),
			vk::BufferUsageFlags::TRANSFER_SRC,
		)?;
		let image = renderer::create_image(
			device,
			dims.0,
			dims.1,
			vk_format,
			vk::ImageTiling::OPTIMAL,
			vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
		)?;
		let image_memory_requirements = device.get_image_memory_requirements(image);
		let image_memory = renderer::allocate_memory(
			device,
			device_memory_properties,
			image_memory_requirements.size,
			vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
		)?;
		renderer::bind_image_memory(device, image, image_memory)?;
		renderer::transition_image_layout(
			device,
			queue,
			command_pool,
			image,
			vk::ImageLayout::UNDEFINED,
			vk::ImageLayout::TRANSFER_DST_OPTIMAL,
		)?;
		let buffer_image_copy = vk::BufferImageCopy {
			buffer_offset: 0,
			buffer_row_length: 0,
			buffer_image_height: 0,
			image_subresource: vk::ImageSubresourceLayers {
				aspect_mask: vk::ImageAspectFlags::COLOR,
				mip_level: 0,
				base_array_layer: 0,
				layer_count: 1,
			},
			image_offset: vk::Offset3D { x: 0, y: 0, z: 0 },
			image_extent: vk::Extent3D {
				width: dims.0,
				height: dims.1,
				depth: 1,
			},
		};
		renderer::record_submit_one_time_commands(device, queue, command_pool, |cmd_buf| {
			device.cmd_copy_buffer_to_image(
				cmd_buf,
				staging_buffer,
				image,
				vk::ImageLayout::TRANSFER_DST_OPTIMAL,
				&[buffer_image_copy],
			);
			Ok(())
		})?;
		renderer::transition_image_layout(
			device,
			queue,
			command_pool,
			image,
			vk::ImageLayout::TRANSFER_DST_OPTIMAL,
			vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
		)?;

		device.destroy_buffer(staging_buffer, None);
		device.free_memory(staging_buffer_memory, None);

		let image_view = renderer::create_image_view(device, image, vk_format, vk::ImageAspectFlags::COLOR)?;

		Ok(TextureData {
			image,
			image_view,
			image_memory,
		})
	}
}

pub fn wl_format_to_vk_format(wl_format: wl_shm::Format) -> vk::Format {
	match wl_format {
		wl_shm::Format::Argb8888 => {
			log::warn!("Converting unsupported format (Argb8888) to Vulkan format, expect color issues");
			vk::Format::B8G8R8A8_UNORM
		}
		wl_shm::Format::Xrgb8888 => {
			log::warn!("Converting unsupported format (Xrgb888) to Vulkan format, expect color issues");
			vk::Format::R8G8B8A8_UNORM
		}
		wl_shm::Format::Bgra8888 => vk::Format::B8G8R8A8_UNORM,
		wl_shm::Format::Rgba8888 => vk::Format::R8G8B8A8_UNORM,
		_ => panic!("Unsupported shm format: {:?}", wl_format),
	}
}
