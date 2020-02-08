use std::{
	ffi::CStr,
	os::raw::{c_char, c_void},
};

use ash::{
	version::{DeviceV1_0, EntryV1_0, InstanceV1_0},
	vk::{self},
	Device, Entry, Instance,
};

use crate::{math::*, renderer::present::PresentBackend};

pub mod present;

pub struct TextureData {
	pub image: vk::Image,
	pub image_view: vk::ImageView,
	pub image_memory: vk::DeviceMemory,
}

impl TextureData {
	pub unsafe fn create_from_source<S: TextureSource>(
		device: &Device,
		queue: vk::Queue,
		command_pool: vk::CommandPool,
		device_memory_properties: vk::PhysicalDeviceMemoryProperties,
		texture_source: S,
	) -> Result<Self, ()> {
		texture_source.create_texture(device, queue, command_pool, device_memory_properties)
	}

	pub unsafe fn destroy(&mut self, device: &Device) {
		device.destroy_image_view(self.image_view, None);
		device.destroy_image(self.image, None);
		device.free_memory(self.image_memory, None);
	}
}

pub struct Renderer {
	pub(crate) entry: Entry,
	pub(crate) instance: Instance,
	pub(crate) physical_device: vk::PhysicalDevice,
	pub(crate) device: Device,
	pub(crate) queue_family_index: u32,
	pub(crate) queue: vk::Queue,
	pub(crate) command_pool: vk::CommandPool,
	pub(crate) device_memory_properties: vk::PhysicalDeviceMemoryProperties,
	pub(crate) depth_image: vk::Image,
	pub(crate) depth_image_view: vk::ImageView,
	pub(crate) depth_memory: vk::DeviceMemory,
	pub(crate) vertex_shader_module: vk::ShaderModule,
	pub(crate) fragment_shader_module: vk::ShaderModule,
	pub(crate) descriptor_pool: vk::DescriptorPool,
	pub(crate) descriptor_set_layout: vk::DescriptorSetLayout,
	pub(crate) sampler: vk::Sampler,
	pub(crate) viewports: [vk::Viewport; 1],
	pub(crate) scissors: [vk::Rect2D; 1],
	pub(crate) pipeline: vk::Pipeline,
	pub(crate) pipeline_layout: vk::PipelineLayout,
	pub(crate) render_pass: vk::RenderPass,
	pub(crate) present_complete_semaphore: vk::Semaphore,
	pub(crate) rendering_complete_semaphore: vk::Semaphore,
	pub(crate) objects: Vec<Object>,
	current_object_handle: u64,
}

impl Renderer {
	pub fn new<P: PresentBackend>(create_args: P::CreateArgs) -> Result<(Self, P, P::ReturnVal), ()> {
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
			let vertex_shader_module = create_shader(
				&device,
				std::io::Cursor::new(vertex_shader_source),
				shaderc::ShaderKind::Vertex,
			)?;
			let fragment_shader_module = create_shader(
				&device,
				std::io::Cursor::new(fragment_shader_source),
				shaderc::ShaderKind::Fragment,
			)?;
			let sampler = create_texture_sampler(&device)?;

			let device_memory_properties = get_physical_device_memory_properties(&instance, physical_device);

			let command_pool = create_command_pool(&device, queue_family_index)?;

			let (
				present::PresentBackendSetup {
					present_backend,
					render_pass,
					depth_image,
					depth_image_view,
					depth_memory,
				},
				return_val,
			) = P::create(
				&entry,
				&instance,
				physical_device,
				&device,
				command_pool,
				device_memory_properties,
				create_args,
			)?;
			let size = present_backend.get_current_size();
			let viewports = [create_viewport(size.0 as f32, size.1 as f32)];
			let scissors = [create_scissor(size.0, size.1)];
			let descriptor_pool = create_descriptor_pool(&device)?;
			let descriptor_set_layout = create_descriptor_set_layout(&device)?;
			let (pipeline, pipeline_layout) = create_pipeline(
				&device,
				render_pass,
				vertex_shader_module,
				fragment_shader_module,
				&[descriptor_set_layout],
				&viewports,
				&scissors,
			)?;

			let present_complete_semaphore = create_semaphore(&device)?;
			let rendering_complete_semaphore = create_semaphore(&device)?;

			let setup_command_buffers = allocate_command_buffers(&device, command_pool, 1)?;
			let setup_command_buffer = setup_command_buffers[0];

			let fence = create_fence(&device, false)?;
			record_submit_command_buffer(&device, queue, setup_command_buffer, fence, &[], &[], &[], |cmd_buf| {
				let layout_transition_barriers = vk::ImageMemoryBarrier::builder()
					.image(depth_image)
					.dst_access_mask(
						vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ
							| vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
					)
					.new_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
					.old_layout(vk::ImageLayout::UNDEFINED)
					.subresource_range(
						vk::ImageSubresourceRange::builder()
							.aspect_mask(vk::ImageAspectFlags::DEPTH)
							.layer_count(1)
							.level_count(1)
							.build(),
					);

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

			let objects = Vec::new();
			let current_object_handle = 1;

			let mut renderer = Self {
				entry,
				instance,
				physical_device,
				queue_family_index,
				device,
				queue,
				command_pool,
				device_memory_properties,
				depth_image,
				depth_image_view,
				depth_memory,
				vertex_shader_module,
				fragment_shader_module,
				descriptor_pool,
				descriptor_set_layout,
				sampler,
				viewports,
				scissors,
				pipeline,
				pipeline_layout,
				render_pass,
				present_complete_semaphore,
				rendering_complete_semaphore,
				objects,
				current_object_handle,
			};

			Ok((renderer, present_backend, return_val))
		}
	}

	pub unsafe fn render<P: PresentBackend>(&mut self, present_backend: &mut P) -> Result<(), ()> {
		present_backend.present(
			self,
			|base, cmd_buf, fence, render_pass_begin_info, wait_mask, wait_semaphores, signal_semaphores| {
				record_submit_command_buffer(
					&base.device,
					base.queue,
					cmd_buf,
					fence,
					wait_mask,
					wait_semaphores,
					signal_semaphores,
					|cmd_buf| {
						base.device.cmd_begin_render_pass(
							cmd_buf,
							&render_pass_begin_info,
							vk::SubpassContents::INLINE,
						);
						base.device
							.cmd_bind_pipeline(cmd_buf, vk::PipelineBindPoint::GRAPHICS, base.pipeline);
						base.device.cmd_set_viewport(cmd_buf, 0, &base.viewports);
						base.device.cmd_set_scissor(cmd_buf, 0, &base.scissors);
						for object in &base.objects {
							if !object.draw {
								continue;
							}
							base.device
								.cmd_bind_vertex_buffers(cmd_buf, 0, &[object.vertex_buffer], &[0]);
							base.device
								.cmd_bind_index_buffer(cmd_buf, object.index_buffer, 0, vk::IndexType::UINT32);
							base.device.cmd_bind_descriptor_sets(
								cmd_buf,
								vk::PipelineBindPoint::GRAPHICS,
								base.pipeline_layout,
								0,
								&[object.descriptor_set],
								&[],
							);
							base.device.cmd_draw_indexed(cmd_buf, 6, 1, 0, 0, 0);
						}
						base.device.cmd_end_render_pass(cmd_buf);
						Ok(())
					},
				)?;

				Ok(())
			},
		)?;

		Ok(())
	}

	pub unsafe fn create_object(&mut self) -> Result<ObjectHandle, ()> {
		self.create_object_with(|_| {})
	}

	pub unsafe fn create_object_with<F: FnOnce(&mut Object)>(&mut self, f: F) -> Result<ObjectHandle, ()> {
		let texture = TextureData::create_from_source(
			&self.device,
			self.queue,
			self.command_pool,
			self.device_memory_properties,
			BlankTextureSource,
		)?;
		let vertices = [
			Vertex::new(
				Point3::new(0.0, 0.0, 0.0),
				Vec4::new(1.0, 0.0, 0.0, 1.0),
				Point2::new(0.0, 0.0),
			),
			Vertex::new(
				Point3::new(1.0, 0.0, 0.0),
				Vec4::new(0.0, 1.0, 0.0, 1.0),
				Point2::new(1.0, 0.0),
			),
			Vertex::new(
				Point3::new(0.0, 1.0, 0.0),
				Vec4::new(0.0, 0.0, 1.0, 1.0),
				Point2::new(0.0, 1.0),
			),
			Vertex::new(
				Point3::new(1.0, 1.0, 0.0),
				Vec4::new(1.0, 1.0, 1.0, 1.0),
				Point2::new(1.0, 1.0),
			),
		];
		let indices = [0, 1, 2, 1, 2, 3];
		let (vertex_buffer, vertex_buffer_memory) = make_buffer(
			&self.device,
			self.device_memory_properties,
			&vertices,
			vk::BufferUsageFlags::VERTEX_BUFFER,
		)?;
		let (index_buffer, index_buffer_memory) = make_buffer(
			&self.device,
			self.device_memory_properties,
			&indices,
			vk::BufferUsageFlags::INDEX_BUFFER,
		)?;
		let (uniform_buffer, uniform_buffer_memory) = make_buffer(
			&self.device,
			self.device_memory_properties,
			&[Mvp::no_transform()],
			vk::BufferUsageFlags::UNIFORM_BUFFER,
		)?;
		let descriptor_set = create_descriptor_set(
			&self.device,
			self.descriptor_pool,
			self.descriptor_set_layout,
			uniform_buffer,
			texture.image_view,
			self.sampler,
		)?;
		let position = Point2::new(0.0, 0.0);
		let id = ObjectHandle(self.current_object_handle);
		self.current_object_handle += 1;
		let mut object = Object {
			id,
			draw: false,
			vertex_buffer,
			vertex_buffer_memory,
			index_buffer,
			index_buffer_memory,
			texture,
			uniform_buffer,
			uniform_buffer_memory,
			descriptor_set,
			position,
		};
		f(&mut object);
		self.objects.push(object);
		Ok(id)
	}

	pub unsafe fn create_object_with_texture(&mut self, texture: TextureData) -> Result<ObjectHandle, ()> {
		let device = &self.device.clone();
		let sampler = self.sampler;
		self.create_object_with(|object| {
			object.replace_texture(device, sampler, texture).unwrap();
		})
	}

	pub fn get_object(&self, handle: ObjectHandle) -> Option<&Object> {
		self.objects.iter().find(|object| object.id == handle)
	}

	pub fn get_object_mut(&mut self, handle: ObjectHandle) -> Option<&mut Object> {
		self.objects.iter_mut().find(|object| object.id == handle)
	}

	pub unsafe fn move_object(&mut self, handle: ObjectHandle, delta: Vec2) {
		if let Some(object) = self.get_object(handle) {
			let mem = self
				.device
				.map_memory(
					object.uniform_buffer_memory,
					0,
					std::mem::size_of::<Mvp>() as u64,
					vk::MemoryMapFlags::empty(),
				)
				.unwrap();
			let mut mvp_slice = ash::util::Align::new(
				mem,
				std::mem::align_of::<Mvp>() as u64,
				std::mem::size_of::<Mvp>() as u64,
			);
			let new_mvp = Mvp::model_translate(Vec3::new(delta.x, delta.y, 0.0));
			mvp_slice.copy_from_slice(&[new_mvp]);
			self.device.unmap_memory(object.uniform_buffer_memory);
		}
	}

	pub unsafe fn delete_object(&mut self, handle: ObjectHandle) {
		if let Some(index) = self
			.objects
			.iter()
			.enumerate()
			.find_map(|(i, object)| if object.id == handle { Some(i) } else { None })
		{
			let mut object = self.objects.remove(index);
			self.device.destroy_buffer(object.vertex_buffer, None);
			self.device.free_memory(object.vertex_buffer_memory, None);
			self.device.destroy_buffer(object.index_buffer, None);
			self.device.free_memory(object.index_buffer_memory, None);
			object.texture.destroy(&self.device);
			self.device.destroy_buffer(object.uniform_buffer, None);
			self.device.free_memory(object.uniform_buffer_memory, None);
			self.device
				.free_descriptor_sets(self.descriptor_pool, &[object.descriptor_set]);
		}
	}
}

pub struct Object {
	pub(crate) id: ObjectHandle,
	pub(crate) draw: bool,
	pub(crate) vertex_buffer: vk::Buffer,
	pub(crate) vertex_buffer_memory: vk::DeviceMemory,
	pub(crate) index_buffer: vk::Buffer,
	pub(crate) index_buffer_memory: vk::DeviceMemory,
	pub(crate) texture: TextureData,
	pub(crate) uniform_buffer: vk::Buffer,
	pub(crate) uniform_buffer_memory: vk::DeviceMemory,
	pub(crate) descriptor_set: vk::DescriptorSet,
	pub(crate) position: Point2,
}

impl Object {
	pub unsafe fn replace_texture(
		&mut self,
		device: &Device,
		sampler: vk::Sampler,
		texture: TextureData,
	) -> Result<(), ()> {
		update_texture_descriptor_set(device, self.descriptor_set, texture.image_view, sampler)?;
		self.texture.destroy(device);
		self.texture = texture;
		Ok(())
	}

	pub unsafe fn update_mvp(&mut self, device: &Device, mvp: Mvp) -> Result<(), ()> {
		let mem = device
			.map_memory(
				self.uniform_buffer_memory,
				0,
				std::mem::size_of::<Mvp>() as u64,
				vk::MemoryMapFlags::empty(),
			)
			.unwrap();
		let mut device_slice = ash::util::Align::new(
			mem,
			std::mem::align_of::<Mvp>() as u64,
			std::mem::size_of::<Mvp>() as u64,
		);
		device_slice.copy_from_slice(&[mvp]);
		device.unmap_memory(self.uniform_buffer_memory);
		Ok(())
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectHandle(u64);

unsafe fn create_entry() -> Result<Entry, ()> {
	let entry = Entry::new().map_err(|e| {
		log::error!("Failed to create Vulkan entry point: {}", e);
	})?;

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

	let layer_names: &[*const c_char] = &[CStr::from_bytes_with_nul(b"VK_LAYER_LUNARG_standard_validation\0")
		.unwrap()
		.as_ptr()];
	let extension_names: &[*const c_char] = &[
		CStr::from_bytes_with_nul(b"VK_EXT_debug_utils\0").unwrap().as_ptr(),
		CStr::from_bytes_with_nul(b"VK_KHR_display\0").unwrap().as_ptr(),
		CStr::from_bytes_with_nul(b"VK_KHR_surface\0").unwrap().as_ptr(),
		CStr::from_bytes_with_nul(b"VK_KHR_xlib_surface\0").unwrap().as_ptr(),
		CStr::from_bytes_with_nul(b"VK_KHR_wayland_surface\0").unwrap().as_ptr(),
		CStr::from_bytes_with_nul(b"VK_KHR_external_memory_capabilities\0")
			.unwrap()
			.as_ptr(),
		CStr::from_bytes_with_nul(b"VK_KHR_get_physical_device_properties2\0")
			.unwrap()
			.as_ptr(),
	];

	let instance_create_info = vk::InstanceCreateInfo::builder()
		.application_info(&app_info)
		.enabled_layer_names(layer_names)
		.enabled_extension_names(extension_names);

	let instance = entry.create_instance(&instance_create_info, None).map_err(|e| {
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
	let backtrace_severities =
		vk::DebugUtilsMessageSeverityFlagsEXT::WARNING | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR;
	if message_severity & backtrace_severities != vk::DebugUtilsMessageSeverityFlagsEXT::empty() {
		let bt = backtrace::Backtrace::new();
		println!("{:?}", bt);
	}
	vk::FALSE
}

unsafe fn create_debug_report_callback(entry: &Entry, instance: &Instance) -> Result<vk::DebugUtilsMessengerEXT, ()> {
	let debug_utils_messenger_create_info = vk::DebugUtilsMessengerCreateInfoEXT::builder()
		.message_severity(
			vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
				| vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
				| vk::DebugUtilsMessageSeverityFlagsEXT::INFO
				| vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE,
		)
		.message_type(
			vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
				| vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE
				| vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION,
		)
		.pfn_user_callback(Some(debug_callback));

	let debug_messenger = ash::extensions::ext::DebugUtils::new(entry, instance);
	let debug_utils_messenger = debug_messenger
		.create_debug_utils_messenger(&debug_utils_messenger_create_info, None)
		.map_err(|e| {
			log::error!("Failed to create debug utils messenger: {}", e);
		})?;

	Ok(debug_utils_messenger)
}

unsafe fn get_physical_device(instance: &Instance) -> Result<vk::PhysicalDevice, ()> {
	let mut physical_devices = instance.enumerate_physical_devices().map_err(|_| {
		log::error!("Failed to enumerate physical devices");
	})?;
	let physical_device = physical_devices.remove(0);

	Ok(physical_device)
}

unsafe fn get_queue_family_index(instance: &Instance, physical_device: vk::PhysicalDevice) -> Result<u32, ()> {
	instance
		.get_physical_device_queue_family_properties(physical_device)
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

unsafe fn create_device(
	instance: &Instance,
	physical_device: vk::PhysicalDevice,
	queue_family_index: u32,
) -> Result<Device, ()> {
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

	let features = vk::PhysicalDeviceFeatures::builder().shader_clip_distance(true).build();

	let device_info = vk::DeviceCreateInfo::builder()
		.queue_create_infos(&queue_infos)
		.enabled_extension_names(extension_names)
		.enabled_features(&features);

	let device = instance
		.create_device(physical_device, &device_info, None)
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

	let command_pool = device.create_command_pool(&pool_create_info, None).map_err(|e| {
		log::error!("Failed to create command pool: {}", e);
	})?;

	Ok(command_pool)
}

pub(crate) unsafe fn allocate_command_buffers(
	device: &Device,
	command_pool: vk::CommandPool,
	amount: u32,
) -> Result<Vec<vk::CommandBuffer>, ()> {
	let command_buffer_allocate_info = vk::CommandBufferAllocateInfo::builder()
		.command_pool(command_pool)
		.level(vk::CommandBufferLevel::PRIMARY)
		.command_buffer_count(amount);

	let command_buffers = device
		.allocate_command_buffers(&command_buffer_allocate_info)
		.map_err(|e| {
			log::error!("Failed to allocate command buffers: {}", e);
		})?;

	Ok(command_buffers)
}

pub(crate) unsafe fn create_image(
	device: &Device,
	width: u32,
	height: u32,
	format: vk::Format,
	tiling: vk::ImageTiling,
	usage: vk::ImageUsageFlags,
) -> Result<vk::Image, ()> {
	let image_extents = vk::Extent3D::builder().width(width).height(height).depth(1);

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

	log::debug!("Creating image");
	let image = device.create_image(&image_create_info, None).map_err(|e| {
		log::error!("Failed to create image: {}", e);
	})?;

	Ok(image)
}

/*
pub(crate) unsafe fn create_fb_image(device: &Device, device_memory_properties: vk::PhysicalDeviceMemoryProperties, dma_buf_fd: RawFd, width: u32, height: u32, format: vk::Format) -> Result<(vk::Image, vk::ImageView, vk::DeviceMemory), ()> {
	let image = create_image(device, width, height, format, vk::ImageTiling::LINEAR, vk::ImageUsageFlags::COLOR_ATTACHMENT)?;
	let image_memory_requirements = device.get_image_memory_requirements(image);
	let memory = import_dma_buf_fd(device, device_memory_properties, dma_buf_fd, image_memory_requirements.size)?;
	bind_image_memory(device, image, memory)?;
	let view = create_image_view(device, image, format, vk::ImageAspectFlags::COLOR)?;
	Ok((image, view, memory))
}
*/

unsafe fn create_depth_image(
	device: &Device,
	device_memory_properties: vk::PhysicalDeviceMemoryProperties,
	width: u32,
	height: u32,
) -> Result<(vk::Image, vk::ImageView, vk::DeviceMemory), ()> {
	let format = vk::Format::D32_SFLOAT;
	let image = create_image(
		device,
		width,
		height,
		format,
		vk::ImageTiling::OPTIMAL,
		vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
	)?;
	let image_memory_requirements = device.get_image_memory_requirements(image);
	let memory = allocate_memory(
		device,
		device_memory_properties,
		image_memory_requirements.size,
		vk::MemoryPropertyFlags::DEVICE_LOCAL,
	)?;
	bind_image_memory(device, image, memory)?;
	let view = create_image_view(device, image, format, vk::ImageAspectFlags::DEPTH)?;
	Ok((image, view, memory))
}

pub(crate) unsafe fn create_fb(
	device: &Device,
	render_pass: vk::RenderPass,
	present_image_view: vk::ImageView,
	depth_image_view: vk::ImageView,
	width: u32,
	height: u32,
) -> Result<vk::Framebuffer, ()> {
	let fb_attachments = [present_image_view, depth_image_view];
	let framebuffer_create_info = vk::FramebufferCreateInfo::builder()
		.render_pass(render_pass)
		.attachments(&fb_attachments)
		.width(width)
		.height(height)
		.layers(1);

	device.create_framebuffer(&framebuffer_create_info, None).map_err(|e| {
		log::error!("Failed to create framebuffer: {}", e);
	})
}

pub(crate) unsafe fn bind_image_memory(device: &Device, image: vk::Image, memory: vk::DeviceMemory) -> Result<(), ()> {
	device.bind_image_memory(image, memory, 0).map_err(|e| {
		log::error!("Failed to bind device memory: {}", e);
	})
}

pub(crate) unsafe fn create_image_view(
	device: &Device,
	image: vk::Image,
	format: vk::Format,
	aspect_flags: vk::ImageAspectFlags,
) -> Result<vk::ImageView, ()> {
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

	let image_view = device.create_image_view(&create_view_info, None).map_err(|e| {
		log::error!("Failed to create image view: {}", e);
	})?;

	Ok(image_view)
}

unsafe fn get_physical_device_memory_properties(
	instance: &Instance,
	physical_device: vk::PhysicalDevice,
) -> vk::PhysicalDeviceMemoryProperties {
	instance.get_physical_device_memory_properties(physical_device)
}

unsafe fn get_memory_and_heap_index(
	device_memory_properties: vk::PhysicalDeviceMemoryProperties,
	memory_properties: vk::MemoryPropertyFlags,
) -> Result<(u32, u32), ()> {
	device_memory_properties
		.memory_types
		.iter()
		.enumerate()
		.find(|(idx, mem_type)| (mem_type.property_flags & memory_properties) == memory_properties)
		.map(|(idx, mem_type)| (idx as u32, mem_type.heap_index))
		.ok_or_else(|| {
			log::error!("Failed to find a suitable memory type");
		})
}

/*
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
*/

pub(crate) unsafe fn allocate_memory(
	device: &Device,
	device_memory_properties: vk::PhysicalDeviceMemoryProperties,
	size: vk::DeviceSize,
	memory_properties: vk::MemoryPropertyFlags,
) -> Result<vk::DeviceMemory, ()> {
	let (mem_type_index, heap_index) = get_memory_and_heap_index(device_memory_properties, memory_properties)?;

	let allocate_memory_info = vk::MemoryAllocateInfo::builder()
		.allocation_size(size)
		.memory_type_index(mem_type_index);

	let memory = device.allocate_memory(&allocate_memory_info, None).map_err(|e| {
		log::error!("Failed to allocate memory: {}", e);
	})?;

	Ok(memory)
}

pub(crate) unsafe fn transition_image_layout(
	device: &Device,
	queue: vk::Queue,
	command_pool: vk::CommandPool,
	image: vk::Image,
	format: vk::Format,
	old_layout: vk::ImageLayout,
	new_layout: vk::ImageLayout,
) -> Result<(), ()> {
	let (src_access_mask, dst_access_mask, source_stage, destination_stage) = match (old_layout, new_layout) {
		(vk::ImageLayout::UNDEFINED, vk::ImageLayout::TRANSFER_DST_OPTIMAL) => (
			vk::AccessFlags::empty(),
			vk::AccessFlags::TRANSFER_WRITE,
			vk::PipelineStageFlags::TOP_OF_PIPE,
			vk::PipelineStageFlags::TRANSFER,
		),
		(vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL) => (
			vk::AccessFlags::TRANSFER_WRITE,
			vk::AccessFlags::SHADER_READ,
			vk::PipelineStageFlags::TRANSFER,
			vk::PipelineStageFlags::FRAGMENT_SHADER,
		),
		_ => {
			log::error!(
				"Unsupported image layout transition: {:?} -> {:?}",
				old_layout,
				new_layout
			);
			return Err(());
		}
	};

	let image_memory_barrier = vk::ImageMemoryBarrier::builder()
		.old_layout(old_layout)
		.new_layout(new_layout)
		.src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
		.dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
		.image(image)
		.subresource_range(
			vk::ImageSubresourceRange::builder()
				.aspect_mask(vk::ImageAspectFlags::COLOR)
				.base_mip_level(0)
				.level_count(1)
				.base_array_layer(0)
				.layer_count(1)
				.build(),
		)
		.src_access_mask(src_access_mask)
		.dst_access_mask(dst_access_mask);

	record_submit_one_time_commands(device, queue, command_pool, |cmd_buf| {
		device.cmd_pipeline_barrier(
			cmd_buf,
			source_stage,
			destination_stage,
			vk::DependencyFlags::empty(),
			&[],
			&[],
			&[image_memory_barrier.build()],
		);

		Ok(())
	})?;

	Ok(())
}

pub trait TextureSource {
	unsafe fn create_texture(
		self,
		device: &Device,
		queue: vk::Queue,
		command_pool: vk::CommandPool,
		device_memory_properties: vk::PhysicalDeviceMemoryProperties,
	) -> Result<TextureData, ()>;
}

pub struct VecTextureSource {
	width: u32,
	height: u32,
	data: Vec<u8>,
	format: vk::Format,
}

impl VecTextureSource {
	pub fn new(width: u32, height: u32, data: Vec<u8>, format: vk::Format) -> Self {
		assert_eq!(width * height * 4, data.len() as u32);
		Self {
			width,
			height,
			data,
			format,
		}
	}
}

impl TextureSource for VecTextureSource {
	unsafe fn create_texture(
		self,
		device: &Device,
		queue: vk::Queue,
		command_pool: vk::CommandPool,
		device_memory_properties: vk::PhysicalDeviceMemoryProperties,
	) -> Result<TextureData, ()> {
		let (staging_buffer, staging_buffer_memory) = make_buffer(
			device,
			device_memory_properties,
			self.data.as_slice(),
			vk::BufferUsageFlags::TRANSFER_SRC,
		)?;
		let image = create_image(
			device,
			self.width,
			self.height,
			self.format,
			vk::ImageTiling::OPTIMAL,
			vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
		)?;
		let image_memory_requirements = device.get_image_memory_requirements(image);
		let image_memory = allocate_memory(
			device,
			device_memory_properties,
			image_memory_requirements.size,
			vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
		)?;
		bind_image_memory(device, image, image_memory)?;
		transition_image_layout(
			device,
			queue,
			command_pool,
			image,
			self.format,
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
				width: self.width,
				height: self.height,
				depth: 1,
			},
		};
		record_submit_one_time_commands(device, queue, command_pool, |cmd_buf| {
			device.cmd_copy_buffer_to_image(
				cmd_buf,
				staging_buffer,
				image,
				vk::ImageLayout::TRANSFER_DST_OPTIMAL,
				&[buffer_image_copy],
			);
			Ok(())
		})?;
		transition_image_layout(
			device,
			queue,
			command_pool,
			image,
			self.format,
			vk::ImageLayout::TRANSFER_DST_OPTIMAL,
			vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
		)?;

		device.destroy_buffer(staging_buffer, None);
		device.free_memory(staging_buffer_memory, None);

		let image_view = create_image_view(device, image, self.format, vk::ImageAspectFlags::COLOR)?;

		Ok(TextureData {
			image,
			image_view,
			image_memory,
		})
	}
}

pub struct BlankTextureSource;

impl TextureSource for BlankTextureSource {
	unsafe fn create_texture(
		self,
		device: &Device,
		queue: vk::Queue,
		command_pool: vk::CommandPool,
		device_memory_properties: vk::PhysicalDeviceMemoryProperties,
	) -> Result<TextureData, ()> {
		let format = vk::Format::R8G8B8A8_UNORM;
		let (staging_buffer, staging_buffer_memory) = make_buffer(
			device,
			device_memory_properties,
			&[127, 127, 127, 127],
			vk::BufferUsageFlags::TRANSFER_SRC,
		)?;
		let image = create_image(
			device,
			1,
			1,
			format,
			vk::ImageTiling::OPTIMAL,
			vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
		)?;
		let image_memory_requirements = device.get_image_memory_requirements(image);
		let image_memory = allocate_memory(
			device,
			device_memory_properties,
			image_memory_requirements.size,
			vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
		)?;
		bind_image_memory(device, image, image_memory)?;
		transition_image_layout(
			device,
			queue,
			command_pool,
			image,
			format,
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
				width: 1,
				height: 1,
				depth: 1,
			},
		};
		record_submit_one_time_commands(device, queue, command_pool, |cmd_buf| {
			device.cmd_copy_buffer_to_image(
				cmd_buf,
				staging_buffer,
				image,
				vk::ImageLayout::TRANSFER_DST_OPTIMAL,
				&[buffer_image_copy],
			);
			Ok(())
		})?;
		transition_image_layout(
			device,
			queue,
			command_pool,
			image,
			format,
			vk::ImageLayout::TRANSFER_DST_OPTIMAL,
			vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
		)?;

		device.destroy_buffer(staging_buffer, None);
		device.free_memory(staging_buffer_memory, None);

		let image_view = create_image_view(device, image, format, vk::ImageAspectFlags::COLOR)?;

		Ok(TextureData {
			image,
			image_view,
			image_memory,
		})
	}
}

unsafe fn create_texture_sampler(device: &Device) -> Result<vk::Sampler, ()> {
	let sampler_create_info = vk::SamplerCreateInfo::builder()
		.mag_filter(vk::Filter::LINEAR)
		.min_filter(vk::Filter::LINEAR)
		.address_mode_u(vk::SamplerAddressMode::REPEAT)
		.address_mode_v(vk::SamplerAddressMode::REPEAT)
		.address_mode_w(vk::SamplerAddressMode::REPEAT)
		.anisotropy_enable(true)
		.max_anisotropy(16.0)
		.border_color(vk::BorderColor::INT_OPAQUE_BLACK)
		.unnormalized_coordinates(false)
		.compare_enable(false)
		.compare_op(vk::CompareOp::ALWAYS)
		.mipmap_mode(vk::SamplerMipmapMode::LINEAR)
		.mip_lod_bias(0.0)
		.min_lod(0.0)
		.max_lod(0.0);

	let sampler = device
		.create_sampler(&sampler_create_info, None)
		.map_err(|e| log::error!("Failed to create sampler: {}", e))?;

	Ok(sampler)
}

/*
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
*/

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
	let color_attachment_refs = [vk::AttachmentReference {
		attachment: 0,
		layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
	}];
	let depth_attachment_ref = vk::AttachmentReference {
		attachment: 1,
		layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
	};
	let dependencies = [vk::SubpassDependency {
		src_subpass: vk::SUBPASS_EXTERNAL,
		dst_subpass: 0,
		src_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
		src_access_mask: vk::AccessFlags::empty(),
		dst_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
		dst_access_mask: vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
		dependency_flags: vk::DependencyFlags::empty(),
	}];
	let subpasses = [vk::SubpassDescription::builder()
		.color_attachments(&color_attachment_refs)
		.depth_stencil_attachment(&depth_attachment_ref)
		.pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
		.build()];
	let render_pass_create_info = vk::RenderPassCreateInfo::builder()
		.attachments(&render_pass_attachments)
		.subpasses(&subpasses)
		.dependencies(&dependencies);

	let render_pass = device.create_render_pass(&render_pass_create_info, None).map_err(|e| {
		log::error!("Error creating render pass: {}", e);
	})?;

	Ok(render_pass)
}

// T cannot be Drop I think
pub(crate) unsafe fn make_buffer<T: Copy>(
	device: &Device,
	device_memory_properties: vk::PhysicalDeviceMemoryProperties,
	data: &[T],
	usage: vk::BufferUsageFlags,
) -> Result<(vk::Buffer, vk::DeviceMemory), ()> {
	let buffer_create_info = vk::BufferCreateInfo::builder()
		.size((data.len() * std::mem::size_of::<T>()) as u64)
		.usage(usage)
		.sharing_mode(vk::SharingMode::EXCLUSIVE);

	let buffer = device.create_buffer(&buffer_create_info, None).map_err(|e| {
		log::error!("Failed to create buffer: {}", e);
	})?;

	let buffer_memory_req = device.get_buffer_memory_requirements(buffer);
	let buffer_memory = allocate_memory(
		device,
		device_memory_properties,
		buffer_memory_req.size,
		vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
	)?;

	let ptr = device
		.map_memory(buffer_memory, 0, buffer_memory_req.size, vk::MemoryMapFlags::empty())
		.map_err(|e| {
			log::error!("Failed to map memory to host: {}", e);
		})?;
	log::debug!("Mapped memory");
	let mut ptr_align = ash::util::Align::new(ptr, std::mem::align_of::<T>() as u64, buffer_memory_req.size);
	log::debug!("Copying from slice");
	ptr_align.copy_from_slice(data);
	device.unmap_memory(buffer_memory);

	device.bind_buffer_memory(buffer, buffer_memory, 0).map_err(|e| {
		log::error!("Failed to bind buffer memory: {}", e);
	})?;

	Ok((buffer, buffer_memory))
}

fn compile_shader<R: std::io::Read>(
	mut source: R,
	kind: shaderc::ShaderKind,
) -> Result<shaderc::CompilationArtifact, ()> {
	let mut compiler = shaderc::Compiler::new().expect("Failed to create shader compiler");
	let mut buf = String::new();
	source.read_to_string(&mut buf).map_err(|e| {
		log::error!("Failed to read shader source: {}", e);
	})?;
	let artifact = compiler
		.compile_into_spirv(&buf, kind, "<unknown>", "main", None)
		.map_err(|e| {
			log::error!("Failed to compile shader: {}", e);
		})?;
	Ok(artifact)
}

unsafe fn load_shader(device: &Device, spirv: &[u32]) -> Result<vk::ShaderModule, ()> {
	let shader_module_create_info = vk::ShaderModuleCreateInfo::builder().code(spirv);

	let shader_module = device
		.create_shader_module(&shader_module_create_info, None)
		.map_err(|e| {
			log::error!("Failed to create shader module: {}", e);
		})?;

	Ok(shader_module)
}

unsafe fn create_shader<R: std::io::Read>(
	device: &Device,
	source: R,
	kind: shaderc::ShaderKind,
) -> Result<vk::ShaderModule, ()> {
	let artifact = compile_shader(source, kind)?;
	load_shader(device, artifact.as_binary())
}

pub(crate) unsafe fn create_viewport(width: f32, height: f32) -> vk::Viewport {
	let viewport = vk::Viewport {
		x: 0.0,
		y: 0.0,
		width,
		height,
		min_depth: 0.0,
		max_depth: 1.0,
	};

	viewport
}

pub(crate) unsafe fn create_scissor(width: u32, height: u32) -> vk::Rect2D {
	let scissor = vk::Rect2D {
		offset: vk::Offset2D { x: 0, y: 0 },
		extent: vk::Extent2D { width, height },
	};

	scissor
}

pub(crate) unsafe fn create_pipeline(
	device: &Device,
	render_pass: vk::RenderPass,
	vs_module: vk::ShaderModule,
	fs_module: vk::ShaderModule,
	descriptor_set_layouts: &[vk::DescriptorSetLayout],
	viewports: &[vk::Viewport],
	scissors: &[vk::Rect2D],
) -> Result<(vk::Pipeline, vk::PipelineLayout), ()> {
	let pipeline_layout_create_info = vk::PipelineLayoutCreateInfo::builder().set_layouts(descriptor_set_layouts);
	let pipeline_layout = device
		.create_pipeline_layout(&pipeline_layout_create_info, None)
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
	let vertex_input_binding_descriptions = [vk::VertexInputBindingDescription {
		binding: 0,
		stride: std::mem::size_of::<Vertex>() as u32,
		input_rate: vk::VertexInputRate::VERTEX,
	}];
	let vertex_input_attribute_descriptions = [
		vk::VertexInputAttributeDescription {
			location: 0,
			binding: 0,
			format: vk::Format::R32G32B32_SFLOAT,
			offset: 0,
		},
		vk::VertexInputAttributeDescription {
			location: 1,
			binding: 0,
			format: vk::Format::R32G32B32A32_SFLOAT,
			offset: 4 * 3,
		},
		vk::VertexInputAttributeDescription {
			location: 2,
			binding: 0,
			format: vk::Format::R32G32_SFLOAT,
			offset: 4 * 3 + 4 * 4,
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
	let color_blend_attachment_states = [vk::PipelineColorBlendAttachmentState {
		blend_enable: 0,
		src_color_blend_factor: vk::BlendFactor::SRC_COLOR,
		dst_color_blend_factor: vk::BlendFactor::ONE_MINUS_DST_COLOR,
		color_blend_op: vk::BlendOp::ADD,
		src_alpha_blend_factor: vk::BlendFactor::ZERO,
		dst_alpha_blend_factor: vk::BlendFactor::ZERO,
		alpha_blend_op: vk::BlendOp::ADD,
		color_write_mask: vk::ColorComponentFlags::all(),
	}];
	let color_blend_state = vk::PipelineColorBlendStateCreateInfo::builder()
		.logic_op(vk::LogicOp::CLEAR)
		.attachments(&color_blend_attachment_states);
	let dynamic_state = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
	let dynamic_state_info = vk::PipelineDynamicStateCreateInfo::builder().dynamic_states(&dynamic_state);

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

	let graphics_pipelines = device
		.create_graphics_pipelines(vk::PipelineCache::null(), &[graphic_pipeline_info.build()], None)
		.map_err(|e| {
			log::error!("Failed to create graphics pipeline: {}", e.1);
		})?;

	let graphic_pipeline = graphics_pipelines[0];

	Ok((graphic_pipeline, pipeline_layout))
}

unsafe fn create_descriptor_pool(device: &Device) -> Result<vk::DescriptorPool, ()> {
	let descriptor_sizes = [
		vk::DescriptorPoolSize {
			ty: vk::DescriptorType::UNIFORM_BUFFER,
			descriptor_count: 1000,
		},
		vk::DescriptorPoolSize {
			ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
			descriptor_count: 1000,
		},
	];

	let descriptor_pool_create_info = vk::DescriptorPoolCreateInfo::builder()
		.pool_sizes(&descriptor_sizes)
		.max_sets(1);
	let descriptor_pool = device
		.create_descriptor_pool(&descriptor_pool_create_info, None)
		.map_err(|e| log::error!("Failed to create descriptor pool: {}", e))?;

	Ok(descriptor_pool)
}

unsafe fn create_descriptor_set_layout(device: &Device) -> Result<vk::DescriptorSetLayout, ()> {
	let desc_set_layout_bindings = [
		vk::DescriptorSetLayoutBinding {
			binding: 0,
			descriptor_type: vk::DescriptorType::UNIFORM_BUFFER,
			descriptor_count: 1,
			stage_flags: vk::ShaderStageFlags::VERTEX,
			..Default::default()
		},
		vk::DescriptorSetLayoutBinding {
			binding: 1,
			descriptor_type: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
			descriptor_count: 1,
			stage_flags: vk::ShaderStageFlags::FRAGMENT,
			..Default::default()
		},
	];
	let descriptor_info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&desc_set_layout_bindings);

	let descriptor_set_layouts = device.create_descriptor_set_layout(&descriptor_info, None).unwrap();

	Ok(descriptor_set_layouts)
}

unsafe fn create_descriptor_set(
	device: &Device,
	descriptor_pool: vk::DescriptorPool,
	descriptor_set_layout: vk::DescriptorSetLayout,
	buffer: vk::Buffer,
	texture_view: vk::ImageView,
	sampler: vk::Sampler,
) -> Result<vk::DescriptorSet, ()> {
	let descriptor_set_layouts = [descriptor_set_layout];
	let descriptor_set_allocate_info = vk::DescriptorSetAllocateInfo::builder()
		.descriptor_pool(descriptor_pool)
		.set_layouts(&descriptor_set_layouts);
	let descriptor_sets = device
		.allocate_descriptor_sets(&descriptor_set_allocate_info)
		.map_err(|e| log::error!("Failed to allocate descriptor sets: {}", e))?;
	let descriptor_set = descriptor_sets[0];

	let buffer_descriptor_info = [vk::DescriptorBufferInfo::builder()
		.buffer(buffer)
		.offset(0)
		.range(std::mem::size_of::<Mvp>() as u64)
		.build()];
	let image_descriptor_info = [vk::DescriptorImageInfo::builder()
		.image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
		.image_view(texture_view)
		.sampler(sampler)
		.build()];

	let descriptor_set_writes = [
		vk::WriteDescriptorSet::builder()
			.dst_set(descriptor_set)
			.dst_binding(0)
			.dst_array_element(0)
			.descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
			.buffer_info(&buffer_descriptor_info)
			.build(),
		vk::WriteDescriptorSet::builder()
			.dst_set(descriptor_set)
			.dst_binding(1)
			.dst_array_element(0)
			.descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
			.image_info(&image_descriptor_info)
			.build(),
	];

	device.update_descriptor_sets(&descriptor_set_writes, &[]);

	Ok(descriptor_set)
}

pub(crate) unsafe fn update_texture_descriptor_set(
	device: &Device,
	descriptor_set: vk::DescriptorSet,
	texture_view: vk::ImageView,
	sampler: vk::Sampler,
) -> Result<(), ()> {
	let image_descriptor_info = [vk::DescriptorImageInfo::builder()
		.image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
		.image_view(texture_view)
		.sampler(sampler)
		.build()];

	let descriptor_set_writes = [vk::WriteDescriptorSet::builder()
		.dst_set(descriptor_set)
		.dst_binding(1)
		.dst_array_element(0)
		.descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
		.image_info(&image_descriptor_info)
		.build()];

	device.update_descriptor_sets(&descriptor_set_writes, &[]);

	Ok(())
}

pub(crate) unsafe fn create_semaphore(device: &Device) -> Result<vk::Semaphore, ()> {
	let semaphore_create_info = vk::SemaphoreCreateInfo::default();

	device.create_semaphore(&semaphore_create_info, None).map_err(|e| {
		log::error!("Failed to create semaphore: {}", e);
	})
}

pub(crate) unsafe fn create_fence(device: &Device, signaled: bool) -> Result<vk::Fence, ()> {
	let mut fence_create_info = vk::FenceCreateInfo::default();
	if signaled {
		fence_create_info.flags = vk::FenceCreateFlags::SIGNALED;
	}

	device.create_fence(&fence_create_info, None).map_err(|e| {
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
	f: F,
) -> Result<(), ()> {
	device
		.reset_command_buffer(command_buffer, vk::CommandBufferResetFlags::RELEASE_RESOURCES)
		.map_err(|e| log::error!("Failed to reset command buffer: {}", e))?;

	let command_buffer_begin_info =
		vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
	device
		.begin_command_buffer(command_buffer, &command_buffer_begin_info)
		.map_err(|e| log::error!("Failed to begin command buffer: {}", e))?;

	f(command_buffer)?;

	device
		.end_command_buffer(command_buffer)
		.map_err(|e| log::error!("Failed to end command buffer: {}", e))?;

	let command_buffers = vec![command_buffer];
	let submit_info = vk::SubmitInfo::builder()
		.wait_semaphores(wait_semaphores)
		.wait_dst_stage_mask(wait_mask)
		.command_buffers(&command_buffers)
		.signal_semaphores(signal_semaphores);

	device
		.queue_submit(queue, &[submit_info.build()], fence)
		.map_err(|e| log::error!("Failed to submit to queue: {}", e))?;

	Ok(())
}

pub(crate) unsafe fn record_submit_one_time_commands<F: FnOnce(vk::CommandBuffer) -> Result<(), ()>>(
	device: &Device,
	queue: vk::Queue,
	command_pool: vk::CommandPool,
	f: F,
) -> Result<(), ()> {
	let fence = create_fence(device, false)?;
	let command_buffer_allocate_info = vk::CommandBufferAllocateInfo::builder()
		.level(vk::CommandBufferLevel::PRIMARY)
		.command_pool(command_pool)
		.command_buffer_count(1);

	let command_buffers = device
		.allocate_command_buffers(&command_buffer_allocate_info)
		.map_err(|e| log::error!("Failed to allocate one time use command buffer: {}", e))?;
	let command_buffer = command_buffers[0];

	let command_buffer_begin_info =
		vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
	device
		.begin_command_buffer(command_buffer, &command_buffer_begin_info)
		.map_err(|e| log::error!("Failed to begin one time command buffer: {}", e))?;

	f(command_buffer)?;

	device
		.end_command_buffer(command_buffer)
		.map_err(|e| log::error!("Failed to end command buffer: {}", e))?;

	let command_buffers_ref = [command_buffer];
	let submit_info = vk::SubmitInfo::builder().command_buffers(&command_buffers_ref);

	device
		.queue_submit(queue, &[submit_info.build()], fence)
		.map_err(|e| log::error!("Failed to submit one time command buffer: {}", e))?;
	device
		.wait_for_fences(&[fence], true, std::u64::MAX)
		.map_err(|e| log::error!("Error while waiting for fences after submitting one time command buffer"))?;

	Ok(())
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Mvp {
	model: Mat4,
	view: Mat4,
	projection: Mat4,
}

impl Mvp {
	pub fn no_transform() -> Self {
		Mvp {
			model: Mat4::identity(),
			view: Mat4::identity(),
			projection: Mat4::identity(),
		}
	}

	pub fn model_translate(delta: Vec3) -> Self {
		Mvp {
			model: Mat4::new_translation(&delta),
			view: Mat4::identity(),
			projection: Mat4::identity(),
		}
	}

	pub fn new_surface(pos: (i32, i32), size: (u32, u32), view_size: (u32, u32)) -> Self {
		/*let pos = Vec2::new(pos.0 as f32, pos.1 as f32);
		let size = Vec2::new(size.0 as f32, size.1 as f32);
		let view_size = Vec2::new(view_size.0 as f32, view_size.1 as f32);

		let model = nalgebra::Isometry3::new(Vec3::new(pos.x, pos.y, 0.0), Vec3::new(0.0, 0.0, 0.0));

		let eye = Point3::new(0.0, 0.0, 0.0);
		let target = Point3::new(0.0, 0.0, 1.0);
		let view = nalgebra::Isometry3::look_at_rh(&eye, &target, &Vec3::y());

		let projection = nalgebra::Orthographic3::new(0.0, view_size.x, 0.0, view_size.y, -1.0, 1.0);

		Self {
			model: model.to_homogeneous(),
			view: view.to_homogeneous(),
			projection: *projection.as_matrix(),
		}*/
		let pos = Vec2::new(pos.0 as f32, pos.1 as f32);
		let size = Vec2::new(size.0 as f32, size.1 as f32);
		let view_size = Vec2::new(view_size.0 as f32, view_size.1 as f32);

		let normalized_pos = Vec2::new(
			(pos.x - view_size.x / 2.0) / (view_size.x / 2.0),
			(pos.y - view_size.y / 2.0) / (view_size.y / 2.0),
		);
		let normalized_size = Vec2::new(
			(size.x / (view_size.x / 2.0)),
			(size.y / (view_size.y / 2.0)),
		);

		Self {
			model: Mat4::new_translation(&Vec3::new(normalized_pos.x, normalized_pos.y, 0.0)).append_nonuniform_scaling(&Vec3::new(normalized_size.x, normalized_size.y, 1.0)),
			view: Mat4::identity(),
			projection: Mat4::identity(),
		}
	}
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Vertex {
	pos: Point3,
	col: Vec4,
	tex: Point2,
}

impl Vertex {
	pub const fn new(pos: Point3, col: Vec4, tex: Point2) -> Self {
		Vertex { pos, col, tex }
	}
}
