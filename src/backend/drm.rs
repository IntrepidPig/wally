use crate::backend::{AsRawFd, RawFd, InputBackend, BackendEvent, RenderBackend};
use drm::control::{ResourceInfo, Device};

pub struct DrmRenderBackend {
	pub(crate) device: gbm::Device<Card>,
	pub(crate) buffer_object: gbm::BufferObject<()>,
	pub(crate) framebuffer_dma_buf_fd: RawFd,
	pub(crate) size: (u32, u32),
}

impl DrmRenderBackend {
	pub fn new(device: gbm::Device<Card>) -> Result<Self, ()> {
		let card = device;
		
		let res_handles = card.resource_handles().unwrap();
		
		let mut crtc = None;
		for test in res_handles.crtcs() {
			let info = card.resource_info::<drm::control::crtc::Info>(*test).unwrap();
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
			let info = card.resource_info::<drm::control::connector::Info>(*test).unwrap();
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
		
/*		let fd = {
			use nix::sys::memfd;
			memfd::memfd_create(
				&std::ffi::CStr::from_bytes_with_nul(b"wally-drm-fb\0").unwrap(),
				memfd::MemFdCreateFlag::MFD_CLOEXEC | memfd::MemFdCreateFlag::MFD_ALLOW_SEALING)
				.expect("Failed to create DMA file descriptor")
		};*/
		
		let size = con_mode.size();
		/*let mut gbm_buf: gbm::BufferObject<()> = gbm_device.import_buffer_object_from_dma_buf(fb_fd, 1920, 1080, 1920 * 4, gbm::Format::ARGB8888, gbm::BufferObjectFlags::SCANOUT | gbm::BufferObjectFlags::RENDERING)
			.expect("Failed to create GBM buffer");*/
		let mut buffer: gbm::BufferObject<()> = card.create_buffer_object(size.0 as u32, size.1 as u32, gbm::Format::ARGB8888, gbm::BufferObjectFlags::SCANOUT | gbm::BufferObjectFlags::RENDERING | gbm::BufferObjectFlags::LINEAR)
			.expect("Failed to create GBM buffer");
		std::thread::sleep(std::time::Duration::from_millis(200)); // TODO remove
		//let mut buffer = drm::control::dumbbuffer::DumbBuffer::create_from_device(&card, (size.0 as u32, size.1 as u32), drm::buffer::PixelFormat::ARGB8888).expect("Failed to create dumbbuffer");
		
		buffer.map_mut(&card, 0, 0, size.0 as u32, size.1 as u32, |map| {
			let buffer_mut = map.buffer_mut();
			let mut i = 0;
			for p in buffer_mut.chunks_exact_mut(4) {
				p[0] = (i % 255) as u8;
				p[1] = (i % 255) as u8;
				p[2] = (i % 255) as u8;
				i += 1;
			}
		}).unwrap().unwrap();
		/*let pixels = {
			let mut buffer = Vec::new();
			for i in 0..(1920 * 1080 * 4) {
				buffer.push((i % 255) as u8);
				buffer.push((i % 255) as u8);
				buffer.push((i % 255) as u8);
				buffer.push(255u8);
			}
			buffer
		};
		buffer.write(&pixels).unwrap();
		log::debug!("Wrote pixels");*/
		/*let mut map = buffer.map(&card).unwrap();
		let mut i = 0;
		for p in map.as_mut().chunks_exact_mut(4) {
			p[0] = (i % 255) as u8;
			p[1] = (i % 255) as u8;
			p[2] = (i % 255) as u8;
			i += 1;
		}
		drop(map);*/
		
		let fb_info = drm::control::framebuffer::create(&card, &buffer).expect("Failed to create framebuffer");
		let fb_handle = fb_info.handle();
		
		let buf_fd = 0;//buffer.as_raw_fd();
		
		drm::control::crtc::set(&card, crtc, fb_handle, &[con], (0, 0), Some(con_mode)).expect("Failed to set device framebuffer");
		
		loop {}
		
		/*Ok(Self {
			device: card,
			buffer_object: buffer,
			framebuffer_dma_buf_fd: buf_fd,
			size: (size.0 as u32, size.1 as u32),
		})*/
	}
	
	pub fn framebuffer_dma_buf_fd(&self) -> RawFd {
		self.framebuffer_dma_buf_fd
	}
}

struct Inner {
	pub card: Card,
}

impl RenderBackend for DrmRenderBackend {
	type Error = ();
	type ShmPool = ();
	
	fn update(&mut self) -> Result<(), Self::Error> {
		Ok(())
	}
	
	fn create_shm_pool(&mut self, fd: RawFd, size: usize) -> Result<Self::ShmPool, Self::Error> {
		
		Ok(())
	}
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

impl drm::Device for Card { }

impl drm::control::Device for Card { }