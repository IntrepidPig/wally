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

use crate::{
	backend::{RenderBackend},
	renderer::{
		present::{PresentBackend},
	},
};
use crate::renderer::present::SurfaceCreator;

pub mod present;

pub struct Renderer {
	pub(crate) entry: Entry,
	pub(crate) instance: Instance,
	pub(crate) physical_device: vk::PhysicalDevice,
	pub(crate) device: Device,
	pub(crate) queue_family_index: u32,
	pub(crate) queue: vk::Queue,
	pub(crate) command_pool: vk::CommandPool,
	pub(crate) setup_command_buffer: vk::CommandBuffer,
	//pub(crate) //draw_command_buffer: vk::CommandBuffer,
	pub(crate) device_memory_properties: vk::PhysicalDeviceMemoryProperties,
	/*pub(crate) fb_image: vk::Image,
	pub(crate) fb_image_view: vk::ImageView,
	pub(crate) fb_memory: vk::DeviceMemory,*/
	pub(crate) depth_image: vk::Image,
	pub(crate) depth_image_view: vk::ImageView,
	pub(crate) depth_memory: vk::DeviceMemory,
	/*pub(crate) memory_fd: RawFd,
	pub(crate) framebuffer: vk::Framebuffer,*/
	pub(crate) vertex_buffer: vk::Buffer,
	pub(crate) vertex_memory: vk::DeviceMemory,
	pub(crate) index_buffer: vk::Buffer,
	pub(crate) index_memory: vk::DeviceMemory,
	pub(crate) vertex_shader_module: vk::ShaderModule,
	pub(crate) fragment_shader_module: vk::ShaderModule,
	pub(crate) viewports: [vk::Viewport; 1],
	pub(crate) scissors: [vk::Rect2D; 1],
	pub(crate) pipeline: vk::Pipeline,
	pub(crate) render_pass: vk::RenderPass,
	pub(crate) present_complete_semaphore: vk::Semaphore,
	pub(crate) rendering_complete_semaphore: vk::Semaphore,
}

impl Renderer {
	pub fn new<S: SurfaceCreator, P: PresentBackend<S>>(create_args: P::CreateArgs, surface_creator_create_args: S::CreateArgs) -> Result<(Self, P), ()> {
		log::trace!("Initializing renderer");
		unsafe {
			let entry = create_entry()?;
			let instance = create_instance(&entry)?;
			let debug_report_callback = create_debug_report_callback(&entry, &instance)?;
			let physical_device = get_physical_device(&instance)?;
			let queue_family_index = get_queue_family_index(&instance, physical_device)?;
			let device = create_device(&instance, physical_device, queue_family_index)?;
			let queue = get_queue(&device, queue_family_index)?;
			
			let vertex_shader_source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/shader.vert"));
			let fragment_shader_source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/shader.frag"));
			let vertex_shader_module = create_shader(&device, std::io::Cursor::new(vertex_shader_source), shaderc::ShaderKind::Vertex)?;
			let fragment_shader_module = create_shader(&device, std::io::Cursor::new(fragment_shader_source), shaderc::ShaderKind::Fragment)?;
			
			let device_memory_properties = get_physical_device_memory_properties(&instance, physical_device);
			let vertices = [
				Vertex::new([-0.5, 0.5, 0.0], [1.0, 0.0, 0.0]),
				Vertex::new([0.0, -0.5, 0.0], [0.0, 1.0, 0.0]),
				Vertex::new([0.5, 0.5, 0.0], [0.0, 0.0, 1.0]),
			];
			let indices = [0u32, 1, 2];
			let (vertex_buffer, vertex_memory) = make_buffer(&device, device_memory_properties, &vertices, vk::BufferUsageFlags::VERTEX_BUFFER)?;
			let (index_buffer, index_memory) = make_buffer(&device, device_memory_properties, &indices, vk::BufferUsageFlags::INDEX_BUFFER)?;
			
			let command_pool = create_command_pool(&device, queue_family_index)?;
			
			let present::PresentBackendSetup { present_backend, render_pass, depth_image, depth_image_view, depth_memory, _phantom } = P::create(&entry, &instance, physical_device, &device, command_pool, device_memory_properties, create_args, surface_creator_create_args)?;
			let size = present_backend.get_current_size();
			let viewports = [create_viewport(size.0 as f32, size.1 as f32)];
			let scissors = [create_scissor(size.0, size.1)];
			let pipeline = create_pipeline(&device, render_pass, vertex_shader_module, fragment_shader_module, &viewports, &scissors)?;
			
			let present_complete_semaphore = create_semaphore(&device)?;
			let rendering_complete_semaphore = create_semaphore(&device)?;
			
			let command_buffers = allocate_command_buffers(&device, command_pool, 1)?;
			let setup_command_buffer = command_buffers[0];
			
			let fence = create_fence(&device)?;
			record_submit_command_buffer(&device, queue, setup_command_buffer, fence,&[], &[], &[], |cmd_buf| {
				let layout_transition_barriers = vk::ImageMemoryBarrier::builder()
					.image(depth_image)
					.dst_access_mask(vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE)
					.new_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
					.old_layout(vk::ImageLayout::UNDEFINED)
					.subresource_range(vk::ImageSubresourceRange::builder()
						.aspect_mask(vk::ImageAspectFlags::DEPTH)
						.layer_count(1)
						.level_count(1)
						.build());
				
				device.cmd_pipeline_barrier(
					cmd_buf,
					vk::PipelineStageFlags::BOTTOM_OF_PIPE,
					vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
					vk::DependencyFlags::empty(),
					&[],
					&[],
					&[layout_transition_barriers.build()],
				);
				
				Ok(())
			})?;
			device.wait_for_fences(&[fence], true, std::u64::MAX).unwrap();
			device.destroy_fence(fence, None);
			
			let renderer = Self {
				entry,
				instance,
				physical_device,
				queue_family_index,
				device,
				queue,
				command_pool,
				setup_command_buffer,
				device_memory_properties,
				depth_image,
				depth_image_view,
				depth_memory,
				vertex_buffer,
				vertex_memory,
				index_buffer,
				index_memory,
				vertex_shader_module,
				fragment_shader_module,
				viewports,
				scissors,
				pipeline,
				render_pass,
				present_complete_semaphore,
				rendering_complete_semaphore,
			};
			
			Ok((renderer, present_backend))
		}
	}
	
	pub unsafe fn render<S: SurfaceCreator, P: PresentBackend<S>>(&mut self, present_backend: &mut P) -> Result<(), ()> {
		present_backend.present(self, |base, cmd_buf, fence, render_pass_begin_info, wait_mask, wait_semaphores, signal_semaphores| {
			record_submit_command_buffer(&base.device, base.queue, cmd_buf, fence, wait_mask, wait_semaphores, signal_semaphores, |cmd_buf| {
				base.device.cmd_begin_render_pass(cmd_buf, &render_pass_begin_info, vk::SubpassContents::INLINE);
				base.device.cmd_bind_pipeline(cmd_buf, vk::PipelineBindPoint::GRAPHICS, base.pipeline);
				base.device.cmd_set_viewport(cmd_buf, 0, &base.viewports);
				base.device.cmd_set_scissor(cmd_buf, 0, &base.scissors);
				base.device.cmd_bind_vertex_buffers(cmd_buf, 0, &[base.vertex_buffer], &[0]);
				base.device.cmd_bind_index_buffer(cmd_buf, base.index_buffer, 0, vk::IndexType::UINT32);
				base.device.cmd_draw_indexed(cmd_buf, 3, 1, 0, 0, 0);
				base.device.cmd_end_render_pass(cmd_buf);
				Ok(())
			})?;
			
			Ok(())
		})?;
		
		Ok(())
	}
}

unsafe fn create_entry() -> Result<Entry, ()> {
	let entry = Entry::new().map_err(|e| {
		log::error!("Failed to create Vulkan entry point: {}", e);
	})?;
	
	/*let entry = Entry::new_custom(
		|| {
			shared_library::dynamic_library::DynamicLibrary::open(Some(&std::path::Path::new("/home/intrepidpig/dev/mesa/build/src/amd/vulkan/libvulkan_radeon.so")))
				.map_err(ash::LoadingError::LibraryLoadError)
				.map(std::sync::Arc::new)
		},
		|vk_lib, name| unsafe {
			vk_lib
				.symbol(&*name.to_string_lossy())
				.unwrap_or(ptr::null_mut())
		},
	).map_err(|e| {
		log::error!("Failed to create Vulkan entry point: {}", e);
	})?;*/
	
	Ok(entry)
}

unsafe fn create_instance(entry: &Entry) -> Result<Instance, ()> {
	let app_name = CStr::from_bytes_with_nul(b"Wally Renderer\0").unwrap();
	let app_info = vk::ApplicationInfo::builder()
		.application_name(app_name)
		.application_version(0)
		.engine_name(app_name)
		.engine_version(0)
		.api_version(vk::make_version(1, 0, 0));
	
	let layer_names: &[*const c_char] = &[
		CStr::from_bytes_with_nul(b"VK_LAYER_LUNARG_standard_validation\0").unwrap().as_ptr(),
	];
	let extension_names: &[*const c_char] = &[
		CStr::from_bytes_with_nul(b"VK_EXT_debug_utils\0").unwrap().as_ptr(),
		CStr::from_bytes_with_nul(b"VK_KHR_display\0").unwrap().as_ptr(),
		CStr::from_bytes_with_nul(b"VK_KHR_surface\0").unwrap().as_ptr(),
		CStr::from_bytes_with_nul(b"VK_KHR_xlib_surface\0").unwrap().as_ptr(),
		//CStr::from_bytes_with_nul(b"VK_KHR_wayland_surface\0").unwrap().as_ptr(),
		CStr::from_bytes_with_nul(b"VK_KHR_external_memory_capabilities\0").unwrap().as_ptr(),
		CStr::from_bytes_with_nul(b"VK_KHR_get_physical_device_properties2\0").unwrap().as_ptr(),
	];
	
	let instance_create_info = vk::InstanceCreateInfo::builder()
		.application_info(&app_info)
		.enabled_layer_names(layer_names)
		.enabled_extension_names(extension_names);
	
	let instance = entry.create_instance(&instance_create_info, None)
		.map_err( |e| {
			log::error!("Failed to create instance: {}", e);
		})?;
	
	Ok(instance)
}

unsafe extern "system" fn debug_callback(
	message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
	message_types: vk::DebugUtilsMessageTypeFlagsEXT,
	p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
	p_user_data: *mut c_void,
) -> vk::Bool32 {
	println!("{:?}", CStr::from_ptr((*p_callback_data).p_message));
	let backtrace_severities = vk::DebugUtilsMessageSeverityFlagsEXT::WARNING | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR;
	if message_severity & backtrace_severities != vk::DebugUtilsMessageSeverityFlagsEXT::empty() {
		let bt = backtrace::Backtrace::new();
		println!("{:?}", bt);
	}
	vk::FALSE
}

unsafe fn create_debug_report_callback(entry: &Entry, instance: &Instance) -> Result<vk::DebugUtilsMessengerEXT, ()> {
	let debug_utils_messenger_create_info = vk::DebugUtilsMessengerCreateInfoEXT::builder()
		.message_severity(vk::DebugUtilsMessageSeverityFlagsEXT::INFO
			| vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
			| vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
			| vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE
		)
		.message_type(vk::DebugUtilsMessageTypeFlagsEXT::GENERAL | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION)
		.pfn_user_callback(Some(debug_callback));
	
	let debug_messenger = ash::extensions::ext::DebugUtils::new(entry, instance);
	let debug_utils_messenger = debug_messenger.create_debug_utils_messenger(&debug_utils_messenger_create_info, None)
		.map_err(|e| {
			log::error!("Failed to create debug utils messenger: {}", e);
		})?;
	
	Ok(debug_utils_messenger)
}

unsafe fn get_physical_device(instance: &Instance) -> Result<vk::PhysicalDevice, ()> {
	let mut physical_devices = instance.enumerate_physical_devices()
		.map_err(|_| {
			log::error!("Failed to enumerate physical devices");
		})?;
	let physical_device = physical_devices.remove(0);
	
	Ok(physical_device)
}

unsafe fn get_queue_family_index(instance: &Instance, physical_device: vk::PhysicalDevice) -> Result<u32, ()> {
	instance.get_physical_device_queue_family_properties(physical_device)
		.into_iter()
		.enumerate()
		.filter_map(|(index, info)| {
			if info.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
				Some(index as u32)
			} else {
				None
			}
		})
		.next()
		.ok_or_else(|| {
			log::error!("No graphics enabled queue family available");
		})
}

unsafe fn create_device(instance: &Instance, physical_device: vk::PhysicalDevice, queue_family_index: u32) -> Result<Device, ()> {
	let priorities = [1.0];
	
	let queue_infos = [vk::DeviceQueueCreateInfo::builder()
		.queue_family_index(queue_family_index)
		.queue_priorities(&priorities)
		.build()];
	
	let extension_names = &[
		b"VK_KHR_swapchain\0" as *const _ as *const c_char,
		b"VK_KHR_external_memory\0" as *const _ as *const c_char,
		b"VK_KHR_external_memory_fd\0" as *const _ as *const c_char,
		//b"VK_KHR_display_swapchain\0" as *const _ as *const c_char,
		b"VK_EXT_external_memory_dma_buf\0" as *const _ as *const c_char,
	];
	
	let features = vk::PhysicalDeviceFeatures::builder()
		.shader_clip_distance(true)
		.build();
	
	let device_info = vk::DeviceCreateInfo::builder()
		.queue_create_infos(&queue_infos)
		.enabled_extension_names(extension_names)
		.enabled_features(&features);
	
	let device = instance.create_device(physical_device, &device_info, None)
		.map_err(|e| {
			log::error!("Failed to create logical device: {}", e);
		})?;
	
	Ok(device)
}

unsafe fn get_queue(device: &Device, queue_family_index: u32) -> Result<vk::Queue, ()> {
	let queue = device.get_device_queue(queue_family_index, 0);
	
	Ok(queue)
}

unsafe fn create_command_pool(device: &Device, queue_family_index: u32) -> Result<vk::CommandPool, ()> {
	let pool_create_info = vk::CommandPoolCreateInfo::builder()
		.flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
		.queue_family_index(queue_family_index);
	
	let command_pool = device.create_command_pool(&pool_create_info, None)
		.map_err(|e| {
			log::error!("Failed to create command pool: {}", e);
		})?;
	
	Ok(command_pool)
}

pub(crate) unsafe fn allocate_command_buffers(device: &Device, command_pool: vk::CommandPool, amount: u32) -> Result<Vec<vk::CommandBuffer>, ()> {
	let command_buffer_allocate_info = vk::CommandBufferAllocateInfo::builder()
		.command_pool(command_pool)
		.level(vk::CommandBufferLevel::PRIMARY)
		.command_buffer_count(amount);
	
	let command_buffers = device.allocate_command_buffers(&command_buffer_allocate_info)
		.map_err(|e| {
			log::error!("Failed to allocate command buffers: {}", e);
		})?;
	
	Ok(command_buffers)
}

unsafe fn create_image(device: &Device, width: u32, height: u32, format: vk::Format, tiling: vk::ImageTiling, usage: vk::ImageUsageFlags) -> Result<vk::Image, ()> {
	let image_extents = vk::Extent3D::builder()
		.width(width)
		.height(height)
		.depth(1);
	
	let image_create_info = vk::ImageCreateInfo::builder()
		.image_type(vk::ImageType::TYPE_2D)
		.format(format)
		.extent(*image_extents)
		.mip_levels(1)
		.array_layers(1)
		.samples(vk::SampleCountFlags::TYPE_1)
		.tiling(tiling)
		.usage(usage)
		.sharing_mode(vk::SharingMode::EXCLUSIVE)
		.initial_layout(vk::ImageLayout::UNDEFINED);
	
	let image = device.create_image(&image_create_info, None)
		.map_err(|e| {
			log::error!("Failed to create image: {}", e);
		})?;
	
	Ok(image)
}

unsafe fn create_test_image(device: &Device, device_memory_properties: vk::PhysicalDeviceMemoryProperties, width: u32, height: u32, format: vk::Format) -> Result<(vk::Image, vk::ImageView, vk::DeviceMemory), ()> {
	let image = create_image(device, width, height, format, vk::ImageTiling::LINEAR, vk::ImageUsageFlags::COLOR_ATTACHMENT)?;
	let image_memory_requirements = device.get_image_memory_requirements(image);
	let memory = allocate_memory(device, device_memory_properties, image_memory_requirements.size, vk::MemoryPropertyFlags::DEVICE_LOCAL)?;
	bind_image_memory(device, image, memory)?;
	let view = create_image_view(device, image, format, vk::ImageAspectFlags::COLOR)?;
	Ok((image, view, memory))
}

pub(crate) unsafe fn create_fb_image(device: &Device, device_memory_properties: vk::PhysicalDeviceMemoryProperties, dma_buf_fd: RawFd, width: u32, height: u32, format: vk::Format) -> Result<(vk::Image, vk::ImageView, vk::DeviceMemory), ()> {
	let image = create_image(device, width, height, format, vk::ImageTiling::LINEAR, vk::ImageUsageFlags::COLOR_ATTACHMENT)?;
	let image_memory_requirements = device.get_image_memory_requirements(image);
	let memory = import_dma_buf_fd(device, device_memory_properties, dma_buf_fd, image_memory_requirements.size)?;
	bind_image_memory(device, image, memory)?;
	let view = create_image_view(device, image, format, vk::ImageAspectFlags::COLOR)?;
	Ok((image, view, memory))
}

unsafe fn create_depth_image(device: &Device, device_memory_properties: vk::PhysicalDeviceMemoryProperties, width: u32, height: u32) -> Result<(vk::Image, vk::ImageView, vk::DeviceMemory), ()> {
	let format = vk::Format::D32_SFLOAT;
	let image = create_image(device, width, height, format, vk::ImageTiling::OPTIMAL, vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT)?;
	let image_memory_requirements = device.get_image_memory_requirements(image);
	let memory = allocate_memory(device, device_memory_properties, image_memory_requirements.size, vk::MemoryPropertyFlags::DEVICE_LOCAL)?;
	bind_image_memory(device, image, memory)?;
	let view = create_image_view(device, image, format, vk::ImageAspectFlags::DEPTH)?;
	Ok((image, view, memory))
}

pub(crate) unsafe fn create_fb(device: &Device, render_pass: vk::RenderPass, present_image_view: vk::ImageView, depth_image_view: vk::ImageView, width: u32, height: u32) -> Result<vk::Framebuffer, ()> {
	let fb_attachments = [present_image_view, depth_image_view];
	let framebuffer_create_info = vk::FramebufferCreateInfo::builder()
		.render_pass(render_pass)
		.attachments(&fb_attachments)
		.width(width)
		.height(height)
		.layers(1);
	
	device.create_framebuffer(&framebuffer_create_info, None)
		.map_err(|e| {
			log::error!("Failed to create framebuffer: {}", e);
		})
}

unsafe fn bind_image_memory(device: &Device, image: vk::Image, memory: vk::DeviceMemory)  -> Result<(), ()> {
	device.bind_image_memory(image, memory, 0)
		.map_err(|e| {
			log::error!("Failed to bind device memory: {}", e);
		})
}

pub(crate) unsafe fn create_image_view(device: &Device, image: vk::Image, format: vk::Format, aspect_flags: vk::ImageAspectFlags) -> Result<vk::ImageView, ()> {
	let create_view_info = vk::ImageViewCreateInfo::builder()
		.view_type(vk::ImageViewType::TYPE_2D)
		.format(format)
		.components(vk::ComponentMapping {
			r: vk::ComponentSwizzle::R,
			g: vk::ComponentSwizzle::G,
			b: vk::ComponentSwizzle::B,
			a: vk::ComponentSwizzle::A,
		})
		.subresource_range(vk::ImageSubresourceRange {
			aspect_mask: aspect_flags,
			base_mip_level: 0,
			level_count: 1,
			base_array_layer: 0,
			layer_count: 1,
		})
		.image(image);
	
	let image_view = device.create_image_view(&create_view_info, None)
		.map_err(|e| {
			log::error!("Failed to create image view: {}", e);
		})?;
	
	Ok(image_view)
}

unsafe fn get_physical_device_memory_properties(instance: &Instance, physical_device: vk::PhysicalDevice) -> vk::PhysicalDeviceMemoryProperties {
	instance.get_physical_device_memory_properties(physical_device)
}

unsafe fn get_memory_and_heap_index(device_memory_properties: vk::PhysicalDeviceMemoryProperties, memory_properties: vk::MemoryPropertyFlags) -> Result<(u32, u32), ()> {
	device_memory_properties.memory_types.iter()
		.enumerate()
		.find(|(idx, mem_type)| (mem_type.property_flags & memory_properties) == memory_properties)
		.map(|(idx, mem_type)| (idx as u32, mem_type.heap_index))
		.ok_or_else(|| {
			log::error!("Failed to find a suitable memory type");
		})
}

unsafe fn import_dma_buf_fd(device: &Device, device_memory_properties: vk::PhysicalDeviceMemoryProperties, fb_dma_buf_fd: RawFd, size: vk::DeviceSize) -> Result<vk::DeviceMemory, ()> {
	let memory_properties = vk::MemoryPropertyFlags::DEVICE_LOCAL | vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
	let (mem_type_index, heap_index) = get_memory_and_heap_index(device_memory_properties, memory_properties)?;
	
	let mut import_memory_fd_info = vk::ImportMemoryFdInfoKHR::builder()
		.fd(fb_dma_buf_fd)
		.handle_type(vk::ExternalMemoryHandleTypeFlags::EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF);
	
	let allocate_memory_info = vk::MemoryAllocateInfo::builder()
		.allocation_size(size)
		.memory_type_index(mem_type_index)
		.push_next(&mut import_memory_fd_info);
	
	let memory = device.allocate_memory(&allocate_memory_info, None)
		.map_err(|e| {
			log::error!("Failed to import dma_buf memory: {}", e);
		})?;
	
	Ok(memory)
}

unsafe fn allocate_memory(device: &Device, device_memory_properties: vk::PhysicalDeviceMemoryProperties, size: vk::DeviceSize, memory_properties: vk::MemoryPropertyFlags) -> Result<vk::DeviceMemory, ()> {
	let (mem_type_index, heap_index) = get_memory_and_heap_index(device_memory_properties, memory_properties)?;
	
	let allocate_memory_info = vk::MemoryAllocateInfo::builder()
		.allocation_size(size)
		.memory_type_index(mem_type_index);
	
	let memory = device.allocate_memory(&allocate_memory_info, None)
		.map_err(|e| {
			log::error!("Failed to allocate memory: {}", e);
		})?;
	
	Ok(memory)
}

pub(crate) unsafe fn get_memory_fd(entry: &Entry, instance: &Instance, device: &Device, memory: vk::DeviceMemory) -> Result<RawFd, ()> {
	let memory_get_fd_info = vk::MemoryGetFdInfoKHR::builder()
		.memory(memory)
		.handle_type(vk::ExternalMemoryHandleTypeFlags::EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF | vk::ExternalMemoryHandleTypeFlags::EXTERNAL_MEMORY_HANDLE_TYPE_HOST_ALLOCATION | vk::ExternalMemoryHandleTypeFlags::EXTERNAL_MEMORY_HANDLE_TYPE_HOST_MAPPED_FOREIGN_MEMORY | vk::ExternalMemoryHandleTypeFlags::EXTERNAL_MEMORY_HANDLE_TYPE_OPAQUE_FD);
	
	let external_memory_fd_fn: vk::KhrExternalMemoryFdFn = vk::KhrExternalMemoryFdFn::load(|name| {
		std::mem::transmute(entry.get_instance_proc_addr(instance.handle(), name.as_ptr()))
	});
	
	let mut fd = -1;
	
	let res = external_memory_fd_fn.get_memory_fd_khr(device.handle(), &memory_get_fd_info.build() as *const _, &mut fd as *mut _);
	if res != vk::Result::SUCCESS {
		log::error!("Failed to create dmabuf fd from vulkan memory allocation: {}", res);
		return Err(());
	}
	
	Ok(fd)
}

pub(crate) unsafe fn create_render_pass(device: &Device, format: vk::Format) -> Result<vk::RenderPass, ()> {
	let render_pass_attachments = [
		vk::AttachmentDescription {
			flags: vk::AttachmentDescriptionFlags::empty(),
			format,
			samples: vk::SampleCountFlags::TYPE_1,
			load_op: vk::AttachmentLoadOp::CLEAR,
			store_op: vk::AttachmentStoreOp::STORE,
			stencil_load_op: vk::AttachmentLoadOp::DONT_CARE,
			stencil_store_op: vk::AttachmentStoreOp::DONT_CARE,
			initial_layout: vk::ImageLayout::UNDEFINED,
			final_layout: vk::ImageLayout::PRESENT_SRC_KHR,
		},
		vk::AttachmentDescription {
			flags: vk::AttachmentDescriptionFlags::empty(),
			format: vk::Format::D32_SFLOAT,
			samples: vk::SampleCountFlags::TYPE_1,
			load_op: vk::AttachmentLoadOp::CLEAR,
			store_op: vk::AttachmentStoreOp::DONT_CARE,
			stencil_load_op: vk::AttachmentLoadOp::DONT_CARE,
			stencil_store_op: vk::AttachmentStoreOp::DONT_CARE,
			initial_layout: vk::ImageLayout::UNDEFINED,
			final_layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
		},
	];
	let color_attachment_refs = [
		vk::AttachmentReference {
			attachment: 0,
			layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
		},
	];
	let depth_attachment_ref = vk::AttachmentReference {
		attachment: 1,
		layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
	};
	let dependencies = [
		vk::SubpassDependency {
			src_subpass: vk::SUBPASS_EXTERNAL,
			src_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
			dst_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
			dst_access_mask: vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
			..Default::default()
		}
	];
	let subpasses = [
		vk::SubpassDescription::builder()
			.color_attachments(&color_attachment_refs)
			.depth_stencil_attachment(&depth_attachment_ref)
			.pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
			.build()
	];
	let render_pass_create_info = vk::RenderPassCreateInfo::builder()
		.attachments(&render_pass_attachments)
		.subpasses(&subpasses)
		.dependencies(&dependencies);
	
	let render_pass = device.create_render_pass(&render_pass_create_info, None)
		.map_err(|e| {
			log::error!("Error creating render pass: {}", e);
		})?;
	
	Ok(render_pass)
}

unsafe fn make_buffer<T: Copy>(device: &Device, device_memory_properties: vk::PhysicalDeviceMemoryProperties, data: &[T], usage: vk::BufferUsageFlags) -> Result<(vk::Buffer, vk::DeviceMemory), ()> {
	let buffer_create_info = vk::BufferCreateInfo::builder()
		.size((data.len() * std::mem::size_of::<T>()) as u64)
		.usage(usage)
		.sharing_mode(vk::SharingMode::EXCLUSIVE);
	
	let buffer = device.create_buffer(&buffer_create_info, None)
		.map_err(|e| {
			log::error!("Failed to create buffer: {}", e);
		})?;
	
	let buffer_memory_req = device.get_buffer_memory_requirements(buffer);
	let buffer_memory = allocate_memory(device, device_memory_properties, buffer_memory_req.size, vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT)?;
	
	let ptr = device.map_memory(buffer_memory, 0, buffer_memory_req.size, vk::MemoryMapFlags::empty())
		.map_err(|e| {
			log::error!("Failed to map memory to host: {}", e);
		})?;
	let mut ptr_align = ash::util::Align::new(ptr, std::mem::align_of::<T>() as u64, buffer_memory_req.size);
	ptr_align.copy_from_slice(data);
	device.unmap_memory(buffer_memory);
	
	device.bind_buffer_memory(buffer, buffer_memory, 0)
		.map_err(|e| {
			log::error!("Failed to bind buffer memory: {}", e);
		})?;
	
	Ok((buffer, buffer_memory))
}

fn compile_shader<R: std::io::Read>(mut source: R, kind: shaderc::ShaderKind) -> Result<shaderc::CompilationArtifact, ()> {
	let mut compiler = shaderc::Compiler::new().expect("Failed to create shader compiler");
	let mut buf = String::new();
	source.read_to_string(&mut buf).map_err(|e| {
		log::error!("Failed to read shader source: {}", e);
	})?;
	let artifact = compiler.compile_into_spirv(&buf, kind, "<unknown>", "main", None).map_err(|e| {
		log::error!("Failed to compile shader: {}", e);
	})?;
	Ok(artifact)
}

unsafe fn load_shader(device: &Device, spirv: &[u32]) -> Result<vk::ShaderModule, ()> {
	let shader_module_create_info = vk::ShaderModuleCreateInfo::builder()
		.code(spirv);
	
	let shader_module = device.create_shader_module(&shader_module_create_info, None)
		.map_err(|e| {
			log::error!("Failed to create shader module: {}", e);
		})?;
	
	Ok(shader_module)
}

unsafe fn create_shader<R: std::io::Read>(device: &Device, source: R, kind: shaderc::ShaderKind) -> Result<vk::ShaderModule, ()> {
	let artifact = compile_shader(source, kind)?;
	load_shader(device, artifact.as_binary())
}

pub(crate) unsafe fn create_viewport(width: f32, height: f32) -> vk::Viewport {
	let viewport =
		vk::Viewport {
			x: 0.0,
			y: 0.0,
			width,
			height,
			min_depth: 0.0,
			max_depth: 1.0
		}
	;
	
	viewport
}

pub(crate) unsafe fn create_scissor(width: u32, height: u32) -> vk::Rect2D {
	let scissor =
		vk::Rect2D {
			offset: vk::Offset2D {
				x: 0,
				y: 0,
			},
			extent: vk::Extent2D {
				width,
				height,
			},
		};
	
	scissor
}

pub(crate) unsafe fn create_pipeline(device: &Device, render_pass: vk::RenderPass, vs_module: vk::ShaderModule, fs_module: vk::ShaderModule, viewports: &[vk::Viewport], scissors: &[vk::Rect2D]) -> Result<vk::Pipeline, ()> {
	let pipeline_layout_create_info = vk::PipelineLayoutCreateInfo::builder();
	let pipeline_layout = device.create_pipeline_layout(&pipeline_layout_create_info, None)
		.map_err(|e| {
			log::error!("Failed to create pipeline layout: {}", e);
		})?;
	
	let shader_entry_name = std::ffi::CString::new("main").unwrap();
	let shader_stage_create_infos = [
		vk::PipelineShaderStageCreateInfo {
			stage: vk::ShaderStageFlags::VERTEX,
			module: vs_module,
			p_name: shader_entry_name.as_ptr(),
			..Default::default()
		},
		vk::PipelineShaderStageCreateInfo {
			stage: vk::ShaderStageFlags::FRAGMENT,
			module: fs_module,
			p_name: shader_entry_name.as_ptr(),
			..Default::default()
		},
	];
	let vertex_input_binding_descriptions = [
		vk::VertexInputBindingDescription {
			binding: 0,
			stride: std::mem::size_of::<Vertex>() as u32,
			input_rate: vk::VertexInputRate::VERTEX,
		},
	];
	let vertex_input_attribute_descriptions = [
		vk::VertexInputAttributeDescription {
			location: 0,
			binding: 0,
			format: vk::Format::R32G32B32A32_SFLOAT,
			offset: 0,
		},
		vk::VertexInputAttributeDescription {
			location: 1,
			binding: 0,
			format: vk::Format::R32G32B32A32_SFLOAT,
			offset: 4 * 4,
		},
	];
	
	let vertex_input_state_info = vk::PipelineVertexInputStateCreateInfo {
		vertex_binding_description_count: vertex_input_binding_descriptions.len() as u32,
		p_vertex_binding_descriptions: vertex_input_binding_descriptions.as_ptr(),
		vertex_attribute_description_count: vertex_input_attribute_descriptions.len() as u32,
		p_vertex_attribute_descriptions: vertex_input_attribute_descriptions.as_ptr(),
		..Default::default()
	};
	let vertex_input_assembly_state_info = vk::PipelineInputAssemblyStateCreateInfo {
		topology: vk::PrimitiveTopology::TRIANGLE_LIST,
		primitive_restart_enable: vk::FALSE,
		..Default::default()
	};
	
	let viewport_state_info = vk::PipelineViewportStateCreateInfo::builder()
		.scissors(scissors)
		.viewports(viewports);
	
	let rasterization_info = vk::PipelineRasterizationStateCreateInfo {
		polygon_mode: vk::PolygonMode::FILL,
		front_face: vk::FrontFace::COUNTER_CLOCKWISE,
		line_width: 1.0,
		..Default::default()
	};
	let multisample_state_info = vk::PipelineMultisampleStateCreateInfo {
		rasterization_samples: vk::SampleCountFlags::TYPE_1,
		..Default::default()
	};
	let noop_stencil_state = vk::StencilOpState {
		fail_op: vk::StencilOp::KEEP,
		pass_op: vk::StencilOp::KEEP,
		depth_fail_op: vk::StencilOp::KEEP,
		compare_op: vk::CompareOp::ALWAYS,
		..Default::default()
	};
	let depth_state_info = vk::PipelineDepthStencilStateCreateInfo {
		depth_test_enable: vk::TRUE,
		depth_write_enable: vk::TRUE,
		depth_compare_op: vk::CompareOp::LESS_OR_EQUAL,
		front: noop_stencil_state,
		back: noop_stencil_state,
		min_depth_bounds: 0.0,
		max_depth_bounds: 1.0,
		..Default::default()
	};
	let color_blend_attachment_states = [
		vk::PipelineColorBlendAttachmentState {
			blend_enable: 0,
			src_color_blend_factor: vk::BlendFactor::SRC_COLOR,
			dst_color_blend_factor: vk::BlendFactor::ONE_MINUS_DST_COLOR,
			color_blend_op: vk::BlendOp::ADD,
			src_alpha_blend_factor: vk::BlendFactor::ZERO,
			dst_alpha_blend_factor: vk::BlendFactor::ZERO,
			alpha_blend_op: vk::BlendOp::ADD,
			color_write_mask: vk::ColorComponentFlags::all(),
		}
	];
	let color_blend_state = vk::PipelineColorBlendStateCreateInfo::builder()
		.logic_op(vk::LogicOp::CLEAR)
		.attachments(&color_blend_attachment_states);
	let dynamic_state = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
	let dynamic_state_info = vk::PipelineDynamicStateCreateInfo::builder()
		.dynamic_states(&dynamic_state);
	
	let graphic_pipeline_info = vk::GraphicsPipelineCreateInfo::builder()
		.stages(&shader_stage_create_infos)
		.vertex_input_state(&vertex_input_state_info)
		.input_assembly_state(&vertex_input_assembly_state_info)
		.viewport_state(&viewport_state_info)
		.rasterization_state(&rasterization_info)
		.multisample_state(&multisample_state_info)
		.depth_stencil_state(&depth_state_info)
		.color_blend_state(&color_blend_state)
		.dynamic_state(&dynamic_state_info)
		.layout(pipeline_layout)
		.render_pass(render_pass);
	
	let graphics_pipelines = device.create_graphics_pipelines(vk::PipelineCache::null(), &[graphic_pipeline_info.build()], None)
		.map_err(|e| {
			log::error!("Failed to create graphics pipeline: {}", e.1);
		})?;
	
	let graphic_pipeline = graphics_pipelines[0];
	
	Ok(graphic_pipeline)
}

pub(crate) unsafe fn create_semaphore(device: &Device) -> Result<vk::Semaphore, ()> {
	let semaphore_create_info = vk::SemaphoreCreateInfo::default();
	
	device.create_semaphore(&semaphore_create_info, None)
		.map_err(|e| {
			log::error!("Failed to create semaphore: {}", e);
		})
}

pub(crate) unsafe fn create_fence(device: &Device) -> Result<vk::Fence, ()> {
	let fence_create_info = vk::FenceCreateInfo::default();
	
	device.create_fence(&fence_create_info, None)
		.map_err(|e| {
			log::error!("Failed to create semaphore: {}", e);
		})
}

unsafe fn record_submit_command_buffer<F: FnOnce(vk::CommandBuffer) -> Result<(), ()>>(
	device: &Device,
	queue: vk::Queue,
	command_buffer: vk::CommandBuffer,
	fence: vk::Fence,
	wait_mask: &[vk::PipelineStageFlags],
	wait_semaphores: &[vk::Semaphore],
	signal_semaphores: &[vk::Semaphore],
	f: F
) -> Result<(), ()> {
	device.reset_command_buffer(command_buffer, vk::CommandBufferResetFlags::RELEASE_RESOURCES)
		.map_err(|e| log::error!("Failed to reset command buffer: {}", e))?;
	
	let command_buffer_begin_info = vk::CommandBufferBeginInfo::builder()
		.flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
	device.begin_command_buffer(command_buffer, &command_buffer_begin_info)
		.map_err(|e| log::error!("Failed to begin command buffer: {}", e))?;
	
	f(command_buffer)?;
	
	device.end_command_buffer(command_buffer)
		.map_err(|e| log::error!("Failed to end command buffer: {}", e))?;
	
	let command_buffers = vec![command_buffer];
	let submit_info = vk::SubmitInfo::builder()
		.wait_semaphores(wait_semaphores)
		.wait_dst_stage_mask(wait_mask)
		.command_buffers(&command_buffers)
		.signal_semaphores(signal_semaphores);
	
	device.queue_submit(queue, &[submit_info.build()], fence)
		.map_err(|e| log::error!("Failed to submit to queue: {}", e))?;
	
	Ok(())
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Vertex {
	pos: [f32; 4],
	col: [f32; 4],
}

impl Vertex {
	pub const fn new(pos: [f32; 3], col: [f32; 3]) -> Self {
		Vertex {
			pos: [pos[0], pos[1], pos[2], 1.0],
			col: [col[0], col[1], col[2], 1.0],
		}
	}
}