use std::os::unix::io::{AsRawFd, RawFd};

use ash::{
	version::DeviceV1_0,
	vk::{self},
	Device, Entry, Instance,
};
use drm::control::{connector, crtc, framebuffer, Device as DrmControlDevice, ResourceInfo};

use crate::renderer::{
	self,
	present::{PresentBackend, PresentBackendSetup},
};

pub struct DrmBuffer {
	fb: DrmFb,
	image: vk::Image,
	image_view: vk::ImageView,
	image_memory: vk::DeviceMemory,
}

pub struct DrmPresentBackend {
	drm_info: DrmInfo,
	drm_fb: DrmFb,
	present_image: vk::Image,
	present_image_view: vk::ImageView,
	present_image_memory: vk::DeviceMemory,
	command_buffer: vk::CommandBuffer,
	present_fence: vk::Fence,
}

impl PresentBackend for DrmPresentBackend {
	type CreateArgs = ();
	type ReturnVal = ();

	unsafe fn create(
		_entry: &Entry,
		_instance: &Instance,
		_physical_device: vk::PhysicalDevice,
		_queue_family_index: u32,
		device: &Device,
		queue: vk::Queue,
		command_pool: vk::CommandPool,
		device_memory_properties: vk::PhysicalDeviceMemoryProperties,
		_create_args: Self::CreateArgs,
	) -> Result<(PresentBackendSetup<Self>, Self::ReturnVal), ()> {
		log::trace!("Initializing DRM present backend");

		let drm_info = DrmInfo::new().unwrap();

		let size = drm_info.size;
		let format = vk::Format::B8G8R8A8_UNORM;
		let drm_fb = drm_info.create_fb().unwrap();
		drm_info.set_crtc_fb(&drm_fb);

		let (fb_image, fb_image_view, fb_memory) = renderer::import_fb_image(
			device,
			device_memory_properties,
			queue,
			command_pool,
			drm_fb.fd,
			size.0,
			size.1,
			format,
		)?;

		let command_buffers = renderer::allocate_command_buffers(device, command_pool, 1)?;
		let present_fence = renderer::create_fence(device, false)?;

		let drm_present_backend = Self {
			drm_info,
			drm_fb,
			present_image: fb_image,
			present_image_view: fb_image_view,
			present_image_memory: fb_memory,
			command_buffer: command_buffers[0],
			present_fence,
		};

		Ok((
			PresentBackendSetup {
				present_backend: drm_present_backend,
			},
			(),
		))
	}

	unsafe fn get_current_size(&self) -> (u32, u32) {
		self.drm_info.size
	}

	unsafe fn present(&mut self, base: &mut renderer::Renderer) -> Result<(), ()> {
		let size = self.get_current_size();
		renderer::transition_image_layout(
			&base.device,
			base.queue,
			base.command_pool,
			base.front_render_target.image,
			vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
			vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
		)?;
		renderer::present::copy_present_image(
			&base.device,
			base.queue,
			self.command_buffer,
			base.front_render_target.image,
			vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
			self.present_image,
			vk::ImageLayout::TRANSFER_DST_OPTIMAL,
			size.0,
			size.1,
			&[],
			&[],
			&[],
			self.present_fence,
		)?;
		renderer::transition_image_layout(
			&base.device,
			base.queue,
			base.command_pool,
			base.front_render_target.image,
			vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
			vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
		)?;
		base.device
			.wait_for_fences(&[self.present_fence], true, std::u64::MAX)
			.map_err(|e| log::error!("Error waiting for fences: {:?}", e))?;
		base.device
			.reset_fences(&[self.present_fence])
			.map_err(|e| log::error!("Error while resetting fences: {}", e))?;

		Ok(())
	}
}

pub struct DrmInfo {
	gbm_device: gbm::Device<Card>,
	crtc: crtc::Handle,
	con: connector::Handle,
	con_mode: drm::control::Mode,
	size: (u32, u32),
}

impl DrmInfo {
	pub unsafe fn new() -> Result<Self, ()> {
		let gbm_device = gbm::Device::new(Card::new().unwrap()).unwrap();

		let res_handles = gbm_device.resource_handles().unwrap();

		let mut crtc = None;
		for test in res_handles.crtcs() {
			let info = gbm_device.resource_info::<drm::control::crtc::Info>(*test).unwrap();
			if info.mode().is_some() {
				crtc = Some(*test);
				break;
			}
		}
		let crtc = crtc.ok_or_else(|| {
			log::error!("Failed to find a suitable CRTC");
		})?;

		let mut con = None;
		let mut con_mode = None;
		for test in res_handles.connectors() {
			let info = gbm_device
				.resource_info::<drm::control::connector::Info>(*test)
				.unwrap();
			if info.connection_state() == drm::control::connector::State::Connected {
				con = Some(*test);
				for mode in info.modes() {
					if mode.size() == (1920, 1080) && mode.vrefresh() == 75 {
						con_mode = Some(mode.clone());
					}
				}
				break;
			}
		}
		let con = con.ok_or_else(|| {
			log::error!("Failed to find a suitable connector");
		})?;
		let con_mode = con_mode.ok_or_else(|| {
			log::error!("Failed to find a suitable connector mode");
		})?;

		let size = con_mode.size();
		let size = (size.0 as u32, size.1 as u32);

		Ok(Self {
			gbm_device,
			crtc,
			con,
			con_mode,
			size,
		})
	}

	pub unsafe fn create_fb(&self) -> Result<DrmFb, ()> {
		let format = gbm::Format::ARGB8888;
		let buffer_object_flags =
			gbm::BufferObjectFlags::SCANOUT | gbm::BufferObjectFlags::RENDERING | gbm::BufferObjectFlags::LINEAR;
		assert!(self.gbm_device.is_format_supported(format, buffer_object_flags));
		let gbm_buffer = self
			.gbm_device
			.create_buffer_object(self.size.0, self.size.1, format, buffer_object_flags)
			.unwrap();
		let fd = gbm_buffer.as_raw_fd();
		let fb_info =
			drm::control::framebuffer::create(&self.gbm_device, &gbm_buffer).expect("Failed to create framebuffer");
		let fb_handle = fb_info.handle();
		Ok(DrmFb {
			_gbm_buffer: gbm_buffer,
			fb_handle,
			fd,
		})
	}

	pub unsafe fn set_crtc_fb(&self, fb: &DrmFb) {
		drm::control::crtc::set(
			&self.gbm_device,
			self.crtc,
			fb.fb_handle,
			&[self.con],
			(0, 0),
			Some(self.con_mode),
		)
		.expect("Failed to set device framebuffer");
	}
}

pub struct DrmFb {
	_gbm_buffer: gbm::BufferObject<()>,
	fb_handle: framebuffer::Handle,
	fd: RawFd,
}

pub struct Card(std::fs::File);

impl AsRawFd for Card {
	fn as_raw_fd(&self) -> RawFd {
		self.0.as_raw_fd()
	}
}

impl Card {
	pub fn new() -> Result<Self, std::io::Error> {
		use std::fs;
		let mut options = fs::OpenOptions::new();
		options.read(true);
		options.write(true);
		let file = options.open("/dev/dri/card0")?;
		Ok(Self(file))
	}
}

impl drm::Device for Card {}

impl drm::control::Device for Card {}
