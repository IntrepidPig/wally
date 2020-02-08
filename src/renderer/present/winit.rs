use std::sync::Arc;

use ::winit::window::Window;
use ash::{
	vk::{self},
	Device, Entry, Instance,
};

use crate::renderer::present::SurfaceCreator;

pub struct WinitSurfaceCreator {
	xlib_surface_loader: ash::extensions::khr::XlibSurface,
	window: Arc<Window>,
}

#[derive(Debug)]
pub struct WinitSurface {
	window: Arc<Window>,
	surface: vk::SurfaceKHR,
}

impl AsRef<vk::SurfaceKHR> for WinitSurface {
	fn as_ref(&self) -> &vk::SurfaceKHR {
		&self.surface
	}
}

impl SurfaceCreator for WinitSurfaceCreator {
	type CreateArgs = Window;
	type ReturnVal = Arc<Window>;
	type SurfaceOwner = WinitSurface;

	unsafe fn create(
		entry: &Entry,
		instance: &Instance,
		create_args: Self::CreateArgs,
	) -> Result<(Self, Self::ReturnVal), ()> {
		let xlib_surface_loader = ash::extensions::khr::XlibSurface::new(entry, instance);
		let window = Arc::new(create_args);
		let return_window = Arc::clone(&window);

		Ok((
			Self {
				xlib_surface_loader,
				window,
			},
			return_window,
		))
	}

	unsafe fn create_surface(
		&mut self,
		physical_device: vk::PhysicalDevice,
		device: &Device,
	) -> Result<Self::SurfaceOwner, ()> {
		use winit::platform::unix::WindowExtUnix;

		let xlib_display = self.window.xlib_display().unwrap();
		let xlib_window = self.window.xlib_window().unwrap();
		let xlib_surface_create_info = vk::XlibSurfaceCreateInfoKHR::builder()
			.window(xlib_window)
			.dpy(xlib_display as *mut vk::Display);

		let surface = self
			.xlib_surface_loader
			.create_xlib_surface(&xlib_surface_create_info, None)
			.map_err(|e| log::error!("Failed to create xlib surface: {}", e))?;

		Ok(Self::SurfaceOwner {
			window: Arc::clone(&self.window),
			surface,
		})
	}

	unsafe fn get_size(&self, surface_owner: &Self::SurfaceOwner) -> (u32, u32) {
		let size = surface_owner.window.inner_size();
		(size.width, size.height)
	}
}
