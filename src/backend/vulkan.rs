use std::{fmt, os::unix::io::RawFd, path::Path};

use festus::{
	geometry::*,
	present::PresentBackend,
	renderer::{self, texture::BufferTextureSource, Renderer, TextureSource, VulkanTextureData},
	rk::{
		ash::{version::DeviceV1_0, vk},
		Device,
	},
};
use thiserror::Error;
use wayland_server::protocol::*;

use super::RgbaInfo;
use crate::backend::{
	easy_shm::{EasyShmBuffer, EasyShmPool},
	GraphicsBackend, Vertex,
};

pub struct VulkanGraphicsBackend<P: PresentBackend> {
	renderer: Renderer,
	present_backend: P,
}

impl<P: PresentBackend> VulkanGraphicsBackend<P> {
	pub fn new(renderer: Renderer, present_backend: P) -> Self {
		Self {
			renderer,
			present_backend,
		}
	}
}

impl<P: PresentBackend> fmt::Debug for VulkanGraphicsBackend<P> {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct("VulkanGraphicsBackend")
			.field("renderer", &"<renderer>")
			.field("present_backend", &"<present_backend>")
			.finish()
	}
}

#[derive(Debug, Error)]
pub enum VulkanGraphicsBackendError {
	#[error("An unknown error occurred in the vulkan render backend")]
	Unknown,
	#[error("Failed to import shared memory (easy_shm) file descriptor: {0}")]
	ShmImportFailed(nix::Error),
	#[error("Shared memory pool (easy_shm) resize failed: {0}")]
	ShmResizeFailed(nix::Error),
	#[error("Vulkan error: {0}")]
	VulkanError(vk::Result),
}

impl<P: PresentBackend + 'static> GraphicsBackend for VulkanGraphicsBackend<P> {
	type Error = VulkanGraphicsBackendError;

	type ShmPool = EasyShmPool;
	type ShmBuffer = EasyShmBuffer;

	type VertexBufferHandle = festus::renderer::VertexBufferHandle;
	type MvpBufferHandle = festus::renderer::MvpBufferHandle;

	type RenderTargetHandle = festus::renderer::VulkanRenderTargetHandle;
	type TextureHandle = festus::renderer::TextureHandle;

	fn update(&mut self) -> Result<(), Self::Error> {
		Ok(())
	}

	fn create_shm_pool(&mut self, fd: RawFd, size: usize) -> Result<Self::ShmPool, Self::Error> {
		unsafe { EasyShmPool::create(fd, size).map_err(|e| VulkanGraphicsBackendError::ShmImportFailed(e)) }
	}

	fn resize_shm_pool(&mut self, pool: &mut Self::ShmPool, new_size: usize) -> Result<(), Self::Error> {
		unsafe {
			pool.resize(new_size).map_err(|e| {
				log::error!("An error occurred resizing a shm pool: {}", e);
				VulkanGraphicsBackendError::ShmResizeFailed(e)
			})
		}
	}

	fn create_shm_buffer(
		&mut self,
		shm_pool: &mut Self::ShmPool,
		offset: usize,
		width: u32,
		height: u32,
		stride: u32,
		format: wl_shm::Format,
	) -> Result<Self::ShmBuffer, Self::Error> {
		unsafe {
			Ok(EasyShmBuffer {
				pool: shm_pool.duplicate(), // TODO this is probably unsound, I was tired when I wrote it
				offset,
				width,
				height,
				stride,
				format,
			})
		}
	}

	fn create_texture_from_rgba(&mut self, rgba: RgbaInfo) -> Result<Self::TextureHandle, Self::Error> {
		unsafe {
			self.renderer
				.create_texture(BufferTextureSource {
					width: rgba.width,
					height: rgba.height,
					format: vk::Format::R8G8B8A8_UNORM,
					buffer: rgba.data,
				})
				.map_err(|_e| VulkanGraphicsBackendError::Unknown)
		}
	}

	fn create_texture_from_shm_buffer(
		&mut self,
		shm_buffer: &Self::ShmBuffer,
	) -> Result<Self::TextureHandle, Self::Error> {
		unsafe {
			let texture_source = EasyShmBufferTextureSource::new(shm_buffer);
			let texture_handle = self.renderer.create_texture(texture_source).map_err(|_e| {
				log::error!("An unknown error occurred while creating a texture");
				VulkanGraphicsBackendError::VulkanError(vk::Result::ERROR_UNKNOWN)
			})?;
			Ok(texture_handle)
		}
	}

	fn create_vertex_buffer(
		&mut self,
		vertices: &[Vertex],
		indices: &[u32],
	) -> Result<Self::VertexBufferHandle, Self::Error> {
		let vertices = vertices
			.iter()
			.map(|vertex| festus::renderer::Vertex {
				pos: festus::math::Point3::new(vertex.pos[0], vertex.pos[1], vertex.pos[2]),
				col: festus::math::Vec4::new(1.0, 0.0, 0.0, 1.0),
				tex: festus::math::Point2::new(vertex.uv[0], vertex.uv[1]),
			})
			.collect::<Vec<_>>();
		unsafe {
			self.renderer.create_vertex_buffer(&vertices, indices).map_err(|_e| {
				log::error!("An unknown error occurred creating a vertex buffer");
				VulkanGraphicsBackendError::VulkanError(vk::Result::ERROR_UNKNOWN)
			})
		}
	}

	fn create_mvp_buffer(&mut self, mvp: [[[f32; 4]; 4]; 3]) -> Result<Self::MvpBufferHandle, Self::Error> {
		let mvp = renderer::Mvp::from(mvp);
		unsafe {
			self.renderer.create_mvp_buffer(mvp).map_err(|_e| {
				log::error!("An unknown error occurred creating an MVP buffer");
				VulkanGraphicsBackendError::VulkanError(vk::Result::ERROR_UNKNOWN)
			})
		}
	}

	fn map_mvp_buffer(&mut self, handle: Self::MvpBufferHandle) -> Option<&mut [[[f32; 4]; 4]; 3]> {
		unsafe {
			self.renderer
				.resources
				.get_mvp_buffer(handle)
				.map(|mvp_buffer| &mut *(mvp_buffer.mvp_buffer_memory_map as *mut [[[f32; 4]; 4]; 3]))
		}
	}

	// A lot of assumptions are made by this function right now.
	fn create_texture(&mut self, size: Size) -> Result<Self::TextureHandle, Self::Error> {
		unsafe {
			self.renderer.create_texture(UninitTextureSource { size }).map_err(|_| {
				log::error!("An unknown error occurred creating a texture");
				VulkanGraphicsBackendError::Unknown
			})
		}
	}

	fn create_render_target(&mut self, size: Size) -> Result<Self::RenderTargetHandle, Self::Error> {
		unsafe {
			self.renderer.create_render_target(size).map_err(|_e| {
				log::error!("An unknown error occurred while creating a render target");
				VulkanGraphicsBackendError::Unknown
			})
		}
	}

	unsafe fn begin_render_pass(&mut self, target: Self::RenderTargetHandle) -> Result<(), Self::Error> {
		self.renderer.begin_render_pass(target).map_err(|_e| {
			log::error!("An unknown error occurred while beginning the render pass");
			VulkanGraphicsBackendError::Unknown
		})?;
		Ok(())
	}

	unsafe fn draw(
		&mut self,
		vertex_buffer: Self::VertexBufferHandle,
		texture: Self::TextureHandle,
		mvp: Self::MvpBufferHandle,
	) -> Result<(), Self::Error> {
		self.renderer.draw(vertex_buffer, texture, mvp).map_err(|_e| {
			log::error!("An unknown error occurred while drawing a surface");
			VulkanGraphicsBackendError::Unknown
		})?;
		Ok(())
	}

	unsafe fn end_render_pass(&mut self, target: Self::RenderTargetHandle) -> Result<(), Self::Error> {
		self.renderer.end_render_pass().map_err(|_e| {
			log::error!("An unknown error occurred while ending the render pass");
			VulkanGraphicsBackendError::Unknown
		})?;
		self.renderer.submit_command_buffer(target).map_err(|_e| {
			log::error!("An unknown error occurred while submitting the command buffer");
			VulkanGraphicsBackendError::Unknown
		})?;
		Ok(())
	}

	fn present_target(&mut self, handle: Self::RenderTargetHandle) -> Result<(), Self::Error> {
		unsafe {
			self.present_backend.present(&mut self.renderer, handle).map_err(|_e| {
				log::error!("An unknown error occurred while presenting a render result");
				VulkanGraphicsBackendError::Unknown
			})
		}
	}

	fn destroy_texture(&mut self, handle: Self::TextureHandle) -> Result<(), Self::Error> {
		unsafe {
			self.renderer.destroy_texture(handle);
		}
		Ok(())
	}

	fn destroy_vertex_buffer(&mut self, handle: Self::VertexBufferHandle) -> Result<(), Self::Error> {
		unsafe {
			self.renderer.destroy_vertex_buffer(handle);
		}
		Ok(())
	}

	fn destroy_mvp_buffer(&mut self, handle: Self::MvpBufferHandle) -> Result<(), Self::Error> {
		unsafe {
			self.renderer.destroy_mvp_buffer(handle);
		}
		Ok(())
	}

	fn destroy_render_target(&mut self, handle: Self::RenderTargetHandle) -> Result<(), Self::Error> {
		unsafe {
			self.renderer.destroy_render_target(handle);
		}
		Ok(())
	}

	fn get_size(&self) -> Size {
		unsafe { self.present_backend.get_current_size() }
	}
}

pub struct UninitTextureSource {
	size: Size,
}

impl renderer::TextureSource for UninitTextureSource {
	unsafe fn create_texture(
		self,
		device: &Device,
		queue: vk::Queue,
		command_pool: vk::CommandPool,
		device_memory_properties: vk::PhysicalDeviceMemoryProperties,
	) -> Result<VulkanTextureData, ()> {
		let format = vk::Format::R8G8B8A8_UNORM;
		let image = renderer::create_image(
			device,
			self.size.width,
			self.size.height,
			format,
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
			vk::ImageLayout::GENERAL,
		)?;

		let image_view = renderer::create_image_view(device, image, format, vk::ImageAspectFlags::COLOR)?;

		let texture_data = VulkanTextureData {
			image,
			image_view,
			image_memory,
			size: self.size,
		};

		Ok(texture_data)
	}
}

pub struct EasyShmBufferTextureSource<'a> {
	buffer: &'a EasyShmBuffer,
}

impl<'a> EasyShmBufferTextureSource<'a> {
	pub fn new(buffer: &'a EasyShmBuffer) -> Self {
		Self { buffer }
	}
}

impl<'a> renderer::TextureSource for EasyShmBufferTextureSource<'a> {
	unsafe fn create_texture(
		self,
		device: &Device,
		queue: vk::Queue,
		command_pool: vk::CommandPool,
		device_memory_properties: vk::PhysicalDeviceMemoryProperties,
	) -> Result<VulkanTextureData, ()> {
		let vk_format = wl_format_to_vk_format(self.buffer.format);
		let slice = self.buffer.as_slice();
		log::debug!("Buffer size: {}", slice.len());
		let (staging_buffer, staging_buffer_memory) = renderer::make_buffer(
			device,
			device_memory_properties,
			slice,
			vk::BufferUsageFlags::TRANSFER_SRC,
		)?;
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

		Ok(VulkanTextureData {
			image,
			image_view,
			image_memory,
			size: Size::new(self.buffer.width as u32, self.buffer.height as u32),
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
	) -> Result<VulkanTextureData, ()> {
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

		Ok(VulkanTextureData {
			image,
			image_view,
			image_memory,
			size: Size::new(dims.0, dims.1),
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
