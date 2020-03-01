use std::time::Instant;

use ash::{
	version::DeviceV1_0,
	vk::{self},
	Device, Entry, Instance,
};

use crate::renderer;

pub mod drm;
pub mod vk_display;
pub mod winit;

pub trait PresentBackend: Sized {
	type CreateArgs;
	type ReturnVal;

	unsafe fn create(
		entry: &Entry,
		instance: &Instance,
		physical_device: vk::PhysicalDevice,
		queue_family_index: u32,
		device: &Device,
		queue: vk::Queue,
		command_pool: vk::CommandPool,
		device_memory_properties: vk::PhysicalDeviceMemoryProperties,
		create_args: Self::CreateArgs,
	) -> Result<(PresentBackendSetup<Self>, Self::ReturnVal), ()>;

	unsafe fn get_current_size(&self) -> (u32, u32);

	unsafe fn present(&mut self, base: &mut renderer::Renderer) -> Result<(), ()>;
}

pub struct PresentBackendSetup<P: PresentBackend> {
	pub(crate) present_backend: P,
}

pub trait SurfaceCreator: Sized {
	type CreateArgs;
	type ReturnVal;
	type SurfaceOwner: AsRef<vk::SurfaceKHR>;

	unsafe fn create(
		entry: &Entry,
		instance: &Instance,
		create_args: Self::CreateArgs,
	) -> Result<(Self, Self::ReturnVal), ()>;

	unsafe fn create_surface(
		&mut self,
		physical_device: vk::PhysicalDevice,
		device: &Device,
	) -> Result<Self::SurfaceOwner, ()>;

	unsafe fn get_size(&self, surface_owner: &Self::SurfaceOwner) -> (u32, u32);
}

const MAX_FRAMES_IN_FLIGHT: usize = 1; // TODO somehow make 2 without uniform buffer sync problems

pub struct GenericPresentBackend<S: SurfaceCreator> {
	surface_creator: S,
	surface_owner: S::SurfaceOwner,
	surface_loader: ash::extensions::khr::Surface,
	swapchain_loader: ash::extensions::khr::Swapchain,
	surface: vk::SurfaceKHR,
	present_images: Vec<vk::Image>,
	present_image_views: Vec<vk::ImageView>,
	framebuffers: Vec<vk::Framebuffer>,
	command_buffers: Vec<vk::CommandBuffer>,
	surface_format: vk::SurfaceFormatKHR,
	swapchain: vk::SwapchainKHR,
	image_available_semaphores: Vec<vk::Semaphore>,
	rendering_complete_semaphores: Vec<vk::Semaphore>,
	in_flight_fences: Vec<vk::Fence>,
	images_in_flight: Vec<vk::Fence>,
	current_frame: usize,
}

impl<S: SurfaceCreator> GenericPresentBackend<S> {
	unsafe fn recreate_swapchain(&mut self, base: &mut renderer::Renderer) -> Result<(), ()> {
		let size = self.surface_creator.get_size(&self.surface_owner);
		let image_usage = vk::ImageUsageFlags::TRANSFER_DST;

		let swapchain_setup = setup_swapchain(
			&base.device,
			base.physical_device,
			&self.surface_loader,
			&self.swapchain_loader,
			base.queue,
			base.command_pool,
			self.surface,
			image_usage,
			base.device_memory_properties,
			Some(self.swapchain),
			size,
		)?;

		let SwapchainSetup {
			surface_format,
			present_images,
			present_image_views,
			framebuffers,
			swapchain,
			render_pass,
			depth_image,
			depth_image_view,
			depth_memory,
		} = swapchain_setup;

		self.surface_format = surface_format;
		self.present_images = present_images;
		self.present_image_views = present_image_views;
		self.framebuffers = framebuffers;
		self.swapchain = swapchain;

		base.render_pass = render_pass;
		base.depth_image = depth_image;
		base.depth_image_view = depth_image_view;
		base.depth_memory = depth_memory;
		base.viewports = [renderer::create_viewport(size.0 as f32, size.1 as f32)];
		base.scissors = [renderer::create_scissor(size.0, size.1)];
		let (pipeline, pipeline_layout) = renderer::create_pipeline(
			&base.device,
			render_pass,
			base.vertex_shader_module,
			base.fragment_shader_module,
			&[base.descriptor_set_layout],
			&base.viewports,
			&base.scissors,
		)?;
		base.pipeline = pipeline;
		base.pipeline_layout = pipeline_layout;

		Ok(())
	}
}

impl<S: SurfaceCreator> PresentBackend for GenericPresentBackend<S> {
	type CreateArgs = S::CreateArgs;
	type ReturnVal = S::ReturnVal;

	unsafe fn create(
		entry: &Entry,
		instance: &Instance,
		physical_device: vk::PhysicalDevice,
		queue_family_index: u32,
		device: &Device,
		queue: vk::Queue,
		command_pool: vk::CommandPool,
		device_memory_properties: vk::PhysicalDeviceMemoryProperties,
		create_args: Self::CreateArgs,
	) -> Result<(PresentBackendSetup<Self>, Self::ReturnVal), ()> {
		let (mut surface_creator, return_val) = S::create(entry, instance, create_args)?;
		let surface_owner = surface_creator.create_surface(physical_device, device)?;

		let surface = *surface_owner.as_ref();

		let surface_loader = ash::extensions::khr::Surface::new(entry, instance);

		let physical_device_surface_support = surface_loader
			.get_physical_device_surface_support(physical_device, queue_family_index, surface)
			.map_err(|e| log::error!("Failed to get surface present support: {}", e))?;
		if !physical_device_surface_support {
			log::error!("Surface does not support presentation");
			return Err(());
		}

		let swapchain_loader = ash::extensions::khr::Swapchain::new(instance, device);

		let size = surface_creator.get_size(&surface_owner);
		let image_usage = vk::ImageUsageFlags::TRANSFER_DST;

		let SwapchainSetup {
			surface_format,
			present_images,
			present_image_views,
			framebuffers,
			swapchain,
			render_pass,
			depth_image,
			depth_image_view,
			depth_memory,
		} = setup_swapchain(
			device,
			physical_device,
			&surface_loader,
			&swapchain_loader,
			queue,
			command_pool,
			surface,
			image_usage,
			device_memory_properties,
			None,
			size,
		)?;

		let command_buffers = renderer::allocate_command_buffers(device, command_pool, framebuffers.len() as u32)?;

		let image_available_semaphores = (0..MAX_FRAMES_IN_FLIGHT)
			.into_iter()
			.map(|_| renderer::create_semaphore(device))
			.collect::<Result<Vec<_>, ()>>()?;
		let rendering_complete_semaphores = (0..MAX_FRAMES_IN_FLIGHT)
			.into_iter()
			.map(|_| renderer::create_semaphore(device))
			.collect::<Result<Vec<_>, ()>>()?;

		let in_flight_fences = (0..MAX_FRAMES_IN_FLIGHT)
			.into_iter()
			.map(|_| {
				let fence_create_info = vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED);
				device
					.create_fence(&fence_create_info, None)
					.map_err(|e| log::error!("Failed to create fence: {}", e))
			})
			.collect::<Result<Vec<_>, ()>>()?;
		let images_in_flight = (0..present_images.len())
			.into_iter()
			.map(|_| vk::Fence::null())
			.collect::<Vec<_>>();

		let winit_present_backend = Self {
			surface_creator,
			surface_owner,
			surface_loader,
			swapchain_loader,
			surface,
			present_images,
			present_image_views,
			framebuffers,
			command_buffers,
			surface_format,
			swapchain,
			image_available_semaphores,
			rendering_complete_semaphores,
			in_flight_fences,
			images_in_flight,
			current_frame: 0,
		};

		Ok((
			PresentBackendSetup {
				present_backend: winit_present_backend,
			},
			return_val,
		))
	}

	unsafe fn get_current_size(&self) -> (u32, u32) {
		self.surface_creator.get_size(&self.surface_owner)
	}

	unsafe fn present(&mut self, base: &mut renderer::Renderer) -> Result<(), ()> {
		let initial_wait_start = Instant::now();
		base.device
			.wait_for_fences(&[self.in_flight_fences[self.current_frame]], true, std::u64::MAX)
			.map_err(|e| log::error!("Error waiting for fence: {}", e))?;
		println!(
			"Waited for frame fence for {} ms",
			initial_wait_start.elapsed().as_secs_f64() * 1000.0
		);
		let acquire_image_start = Instant::now();
		let image_index = match self.swapchain_loader.acquire_next_image(
			self.swapchain,
			std::u64::MAX,
			self.image_available_semaphores[self.current_frame],
			vk::Fence::null(),
		) {
			Ok((image_index, _is_suboptimal)) => image_index,
			Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
				log::trace!("Swapchain out of date, recreating");
				self.recreate_swapchain(base)?;
				return Ok(());
			}
			Err(e) => {
				log::error!("Failed to acquire a swapchain image: {}", e);
				return Err(());
			}
		} as usize;
		println!(
			"Acquired next swapchain image in {} ms",
			acquire_image_start.elapsed().as_secs_f64() * 1000.0
		);

		if self.images_in_flight[image_index] != vk::Fence::null() {
			let second_wait_start = Instant::now();
			base.device
				.wait_for_fences(&[self.images_in_flight[image_index]], true, std::u64::MAX)
				.map_err(|e| {
					log::error!("Error while waiting for image in flight fence: {}", e);
				})?;
			println!(
				"Waited for current image fence for {} ms",
				second_wait_start.elapsed().as_secs_f64() * 1000.0
			);
		}
		self.images_in_flight[image_index] = self.in_flight_fences[self.current_frame];

		base.device
			.reset_fences(&[self.in_flight_fences[self.current_frame]])
			.unwrap();

		let size = self.surface_creator.get_size(&self.surface_owner);

		let wait_mask = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
		let wait_semaphores = [self.image_available_semaphores[self.current_frame]];
		let signal_semaphores = [self.rendering_complete_semaphores[self.current_frame]];

		renderer::transition_image_layout(
			&base.device,
			base.queue,
			base.command_pool,
			self.present_images[image_index],
			vk::ImageLayout::PRESENT_SRC_KHR,
			vk::ImageLayout::TRANSFER_DST_OPTIMAL,
		)?;
		copy_present_image(
			&base.device,
			base.queue,
			self.command_buffers[image_index],
			base.front_render_target.image,
			vk::ImageLayout::GENERAL,
			self.present_images[image_index],
			vk::ImageLayout::TRANSFER_DST_OPTIMAL,
			size.0,
			size.1,
			&wait_mask,
			&wait_semaphores,
			&signal_semaphores,
			self.in_flight_fences[self.current_frame],
		)?;
		renderer::transition_image_layout(
			&base.device,
			base.queue,
			base.command_pool,
			self.present_images[image_index],
			vk::ImageLayout::TRANSFER_DST_OPTIMAL,
			vk::ImageLayout::PRESENT_SRC_KHR,
		)?;

		let wait_semaphores = [self.rendering_complete_semaphores[self.current_frame]];
		let swapchains = [self.swapchain];
		let image_indices = [image_index as u32];

		let present_info = vk::PresentInfoKHR::builder()
			.wait_semaphores(&wait_semaphores)
			.swapchains(&swapchains)
			.image_indices(&image_indices);

		let queue_present_start = Instant::now();
		match self.swapchain_loader.queue_present(base.queue, &present_info) {
			Ok(_is_suboptimal) => {}
			Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
				log::trace!("Swapchain out of date, recreating");
				self.recreate_swapchain(base)?;
				return Ok(());
			}
			Err(e) => {
				log::error!("Failed to acquire a swapchain image: {}", e);
			}
		}
		println!(
			"Queue present took {} ms",
			queue_present_start.elapsed().as_secs_f64() * 1000.0
		);

		self.current_frame = (self.current_frame + 1) % MAX_FRAMES_IN_FLIGHT;

		Ok(())
	}
}

struct SwapchainSetup {
	pub(crate) surface_format: vk::SurfaceFormatKHR,
	pub(crate) present_images: Vec<vk::Image>,
	pub(crate) present_image_views: Vec<vk::ImageView>,
	pub(crate) framebuffers: Vec<vk::Framebuffer>,
	pub(crate) swapchain: vk::SwapchainKHR,
	pub(crate) render_pass: vk::RenderPass,
	pub(crate) depth_image: vk::Image,
	pub(crate) depth_image_view: vk::ImageView,
	pub(crate) depth_memory: vk::DeviceMemory,
}

unsafe fn setup_swapchain(
	device: &Device,
	physical_device: vk::PhysicalDevice,
	surface_loader: &ash::extensions::khr::Surface,
	swapchain_loader: &ash::extensions::khr::Swapchain,
	queue: vk::Queue,
	command_pool: vk::CommandPool,
	surface: vk::SurfaceKHR,
	image_usage: vk::ImageUsageFlags,
	device_memory_properties: vk::PhysicalDeviceMemoryProperties,
	old_swapchain: Option<vk::SwapchainKHR>,
	size: (u32, u32),
) -> Result<SwapchainSetup, ()> {
	let new_surface_format = get_surface_format(physical_device, &surface_loader, surface)?;

	let new_swapchain = create_swapchain(
		physical_device,
		&surface_loader,
		&swapchain_loader,
		surface,
		size.0,
		size.1,
		image_usage,
		new_surface_format,
		old_swapchain,
	)?;

	let new_present_images = swapchain_loader
		.get_swapchain_images(new_swapchain)
		.map_err(|e| log::error!("Failed to get swapchain images: {}", e))?;

	for image in &new_present_images {
		renderer::transition_image_layout(
			device,
			queue,
			command_pool,
			*image,
			vk::ImageLayout::UNDEFINED,
			vk::ImageLayout::PRESENT_SRC_KHR,
		)?;
	}
	let new_present_image_views: Vec<vk::ImageView> = new_present_images
		.iter()
		.map(|&image| {
			renderer::create_image_view(device, image, new_surface_format.format, vk::ImageAspectFlags::COLOR)
		})
		.collect::<Result<Vec<_>, ()>>()?;
	let new_render_pass = renderer::create_render_pass(device, new_surface_format.format)?;
	let (depth_image, depth_image_view, depth_memory) =
		renderer::create_depth_image(device, device_memory_properties, size.0, size.1)?;
	let new_framebuffers: Vec<vk::Framebuffer> = new_present_image_views
		.iter()
		.map(|&present_image_view| {
			renderer::create_fb(
				device,
				new_render_pass,
				present_image_view,
				depth_image_view,
				size.0,
				size.1,
			)
		})
		.collect::<Result<Vec<_>, ()>>()?;

	Ok(SwapchainSetup {
		surface_format: new_surface_format,
		present_images: new_present_images,
		present_image_views: new_present_image_views,
		framebuffers: new_framebuffers,
		swapchain: new_swapchain,
		render_pass: new_render_pass,
		depth_image,
		depth_image_view,
		depth_memory,
	})
}

unsafe fn get_surface_format(
	physical_device: vk::PhysicalDevice,
	surface_loader: &ash::extensions::khr::Surface,
	surface: vk::SurfaceKHR,
) -> Result<vk::SurfaceFormatKHR, ()> {
	let surface_formats = surface_loader
		.get_physical_device_surface_formats(physical_device, surface)
		.map_err(|e| log::error!("Failed to get surface formats: {}", e))?;
	log::info!("Got supported surface formats: {:?}", surface_formats);
	let surface_format = surface_formats
		.iter()
		.map(|&surface_format| match surface_format.format {
			vk::Format::UNDEFINED => vk::SurfaceFormatKHR {
				format: vk::Format::B8G8R8A8_UNORM,
				color_space: surface_format.color_space,
			},
			_ => surface_format,
		})
		.nth(0)
		.expect("Failed to find a suitable surface format");

	Ok(surface_format)
}

unsafe fn create_swapchain(
	physical_device: vk::PhysicalDevice,
	surface_loader: &ash::extensions::khr::Surface,
	swapchain_loader: &ash::extensions::khr::Swapchain,
	surface: vk::SurfaceKHR,
	width: u32,
	height: u32,
	image_usage: vk::ImageUsageFlags,
	surface_format: vk::SurfaceFormatKHR,
	old_swapchain: Option<vk::SwapchainKHR>,
) -> Result<vk::SwapchainKHR, ()> {
	let surface_capabilities = surface_loader
		.get_physical_device_surface_capabilities(physical_device, surface)
		.map_err(|e| log::error!("Failed to get surface capabilities: {}", e))?;
	let desired_image_count = surface_capabilities.min_image_count + 1;
	let surface_resolution = match surface_capabilities.current_extent.width {
		std::u32::MAX => vk::Extent2D { width, height },
		_ => surface_capabilities.current_extent,
	};
	let pre_transform = if surface_capabilities
		.supported_transforms
		.contains(vk::SurfaceTransformFlagsKHR::IDENTITY)
	{
		vk::SurfaceTransformFlagsKHR::IDENTITY
	} else {
		surface_capabilities.current_transform
	};
	let present_modes = surface_loader
		.get_physical_device_surface_present_modes(physical_device, surface)
		.map_err(|e| log::error!("Failed to get present modes: {}", e))?;
	let present_mode = present_modes
		.iter()
		.cloned()
		.find(|&mode| mode == vk::PresentModeKHR::MAILBOX)
		.unwrap_or_else(|| {
			log::warn!("Present mode MAILBOX was unavailable, falling back to FIFO");
			vk::PresentModeKHR::FIFO
		});

	let swapchain_create_info = vk::SwapchainCreateInfoKHR::builder()
		.surface(surface)
		.min_image_count(desired_image_count)
		.image_color_space(surface_format.color_space)
		.image_format(surface_format.format)
		.image_extent(surface_resolution)
		.image_usage(image_usage)
		.image_sharing_mode(vk::SharingMode::EXCLUSIVE)
		.pre_transform(pre_transform)
		.composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
		.present_mode(present_mode)
		.clipped(true)
		.image_array_layers(1);

	let swapchain_create_info = if let Some(old_swapchain) = old_swapchain {
		swapchain_create_info.old_swapchain(old_swapchain)
	} else {
		swapchain_create_info
	};

	let swapchain = swapchain_loader
		.create_swapchain(&swapchain_create_info, None)
		.map_err(|e| log::error!("Failed to create swapchain: {}", e))?;

	Ok(swapchain)
}

pub(crate) unsafe fn copy_present_image(
	device: &Device,
	queue: vk::Queue,
	command_buffer: vk::CommandBuffer,
	src: vk::Image,
	src_layout: vk::ImageLayout,
	dst: vk::Image,
	dst_layout: vk::ImageLayout,
	width: u32,
	height: u32,
	wait_mask: &[vk::PipelineStageFlags],
	wait_semaphores: &[vk::Semaphore],
	signal_semaphores: &[vk::Semaphore],
	fence: vk::Fence,
) -> Result<(), ()> {
	renderer::record_submit_command_buffer(
		device,
		queue,
		command_buffer,
		fence,
		wait_mask,
		wait_semaphores,
		signal_semaphores,
		|cmd_buf| {
			device.cmd_copy_image(
				cmd_buf,
				src,
				src_layout,
				dst,
				dst_layout,
				&[vk::ImageCopy {
					src_subresource: vk::ImageSubresourceLayers {
						aspect_mask: vk::ImageAspectFlags::COLOR,
						mip_level: 0,
						base_array_layer: 0,
						layer_count: 1,
					},
					src_offset: vk::Offset3D { x: 0, y: 0, z: 0 },
					dst_subresource: vk::ImageSubresourceLayers {
						aspect_mask: vk::ImageAspectFlags::COLOR,
						mip_level: 0,
						base_array_layer: 0,
						layer_count: 1,
					},
					dst_offset: vk::Offset3D { x: 0, y: 0, z: 0 },
					extent: vk::Extent3D {
						width,
						height,
						depth: 1,
					},
				}],
			);

			Ok(())
		},
	)?;

	Ok(())
}
