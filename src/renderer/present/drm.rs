use std::{
	os::unix::io::{RawFd},
	os::raw::{c_char, c_void},
	ptr::{self},
	ffi::{CStr, CString},
};

use ash::{
	Device, Entry, Instance,
	version::{EntryV1_0, DeviceV1_0, DeviceV1_1, InstanceV1_0, InstanceV1_1},
	vk::{self},
};
use winit::{
	window::{Window, WindowBuilder},
	event_loop::{EventLoop, ControlFlow},
};

use crate::{
	backend::{
		drm::{DrmRenderBackend, Card},
	},
	renderer::{
		self,
		present::{PresentBackend, PresentBackendSetup},
	}
};

const MAX_FRAMES_IN_FLIGHT: usize = 2;

pub struct DrmPresentBackend {
	drm_render_backend: DrmRenderBackend,
	present_image: vk::Image,
	present_image_view: vk::ImageView,
	present_image_memory: vk::DeviceMemory,
	framebuffer: vk::Framebuffer,
	command_buffer: vk::CommandBuffer,
	rendering_complete_semaphores: Vec<vk::Semaphore>,
	in_flight_fences: Vec<vk::Fence>,
	current_frame: usize,
}

impl DrmPresentBackend {

}

impl PresentBackend for DrmPresentBackend {
	type CreateArgs = gbm::Device<Card>;
	
	unsafe fn create(
		entry: &Entry,
		instance: &Instance,
		physical_device: vk::PhysicalDevice,
		device: &Device,
		command_pool: vk::CommandPool,
		device_memory_properties: vk::PhysicalDeviceMemoryProperties,
		create_args: Self::CreateArgs,
	) -> Result<PresentBackendSetup<Self>, ()> {
		log::trace!("Initializing DRM present backend");
		let surface_loader = ash::extensions::khr::Surface::new(entry, instance);
		
		log::trace!("Initializing DRM render backend");
		let drm_render_backend = DrmRenderBackend::new(create_args).map_err(|e| {
			log::error!("Failed to initialize DRM");
		})?;
		log::trace!("Initialized DRM render backend successfully");
		let size = drm_render_backend.size;
		
		let format = vk::Format::B8G8R8A8_UNORM;
		let (fb_image, fb_image_view, fb_memory) = renderer::create_fb_image(device, device_memory_properties, drm_render_backend.framebuffer_dma_buf_fd, size.0, size.1, format)?;
		let (depth_image, depth_image_view, depth_memory) = renderer::create_depth_image(device, device_memory_properties, size.0, size.1)?;
		//let memory_fd = renderer::get_memory_fd(&entry, &instance, &device, fb_memory)?;
		
		let render_pass = renderer::create_render_pass(device, format)?;
		let framebuffer = renderer::create_fb(&device, render_pass, fb_image_view, depth_image_view, size.0, size.1)?;
		
		let command_buffers = renderer::allocate_command_buffers(device, command_pool, 1)?;
		
		let semaphore_create_info = vk::SemaphoreCreateInfo::default();
		
		let rendering_complete_semaphores = (0..MAX_FRAMES_IN_FLIGHT).into_iter().map(|_| renderer::create_semaphore(device)).collect::<Result<Vec<_>, ()>>()?;
		
		let in_flight_fences = (0..MAX_FRAMES_IN_FLIGHT).into_iter().map(|_| {
			let fence_create_info = vk::FenceCreateInfo::builder()
				.flags(vk::FenceCreateFlags::SIGNALED);
			device.create_fence(&fence_create_info, None)
				.map_err(|e| log::error!("Failed to create fence: {}", e))
		}).collect::<Result<Vec<_>, ()>>()?;
		
		let drm_present_backend = Self {
			drm_render_backend,
			present_image: fb_image,
			present_image_view: fb_image_view,
			present_image_memory: fb_memory,
			framebuffer,
			command_buffer: command_buffers[0],
			rendering_complete_semaphores,
			in_flight_fences,
			current_frame: 0,
		};
		
		Ok(PresentBackendSetup {
			present_backend: drm_present_backend,
			render_pass,
			depth_image,
			depth_image_view,
			depth_memory
		})
	}
	
	unsafe fn get_current_size(&self) -> (u32, u32) {
		self.drm_render_backend.size
	}
	
	unsafe fn present<F: FnOnce(&renderer::Renderer, vk::CommandBuffer, vk::Fence, vk::RenderPassBeginInfo, &[vk::PipelineStageFlags], &[vk::Semaphore], &[vk::Semaphore]) -> Result<(), ()>>(&mut self, base: &mut renderer::Renderer, f: F) -> Result<(), ()> {
		log::debug!("Presenting");
		// Wait for this frame to finish submitting in the queue
		base.device.wait_for_fences(&[self.in_flight_fences[self.current_frame]], true, std::u64::MAX)
			.map_err(|e| log::error!("Error waiting for fence: {}", e))?;
		base.device.reset_fences(&[self.in_flight_fences[self.current_frame]])
			.unwrap();
		
		let clear_values = [
			vk::ClearValue {
				color: vk::ClearColorValue {
					float32: [1.0, 1.0, 1.0, 1.0],
				}
			},
			vk::ClearValue {
				depth_stencil: vk::ClearDepthStencilValue {
					depth: 1.0,
					stencil: 0,
				}
			},
		];
		
		let render_pass_begin_info = vk::RenderPassBeginInfo::builder()
			.render_pass(base.render_pass)
			.framebuffer(self.framebuffer)
			.render_area(vk::Rect2D {
				offset: vk::Offset2D {
					x: 0,
					y: 0,
				},
				extent: vk::Extent2D {
					width: self.drm_render_backend.size.0,
					height: self.drm_render_backend.size.1,
				},
			})
			.clear_values(&clear_values);
		
		let command_buffer = self.command_buffer;
		let wait_mask = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
		
		let wait_semaphores = [self.rendering_complete_semaphores[self.current_frame]];
		let signal_semaphores = [self.rendering_complete_semaphores[self.current_frame]];
		
		f(base, command_buffer, self.in_flight_fences[self.current_frame], render_pass_begin_info.build(), &wait_mask, &signal_semaphores, &wait_semaphores)?;
		
		self.current_frame = (self.current_frame + 1) % MAX_FRAMES_IN_FLIGHT;
		log::trace!("Presented");
		
		Ok(())
	}
}