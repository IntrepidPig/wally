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
use ::winit::{
	window::{Window, WindowBuilder},
	event_loop::{EventLoop, ControlFlow},
};

use crate::{
	renderer::{
		present::{SurfaceCreator},
	}
};

pub struct WinitSurfaceCreator<'a, T: 'static> {
	xlib_surface_loader: ash::extensions::khr::XlibSurface,
	event_loop: &'a EventLoop<T>,
}

#[derive(Debug)]
pub struct WinitSurface {
	window: Window,
	surface: vk::SurfaceKHR,
}

impl AsRef<vk::SurfaceKHR> for WinitSurface {
	fn as_ref(&self) -> &vk::SurfaceKHR {
		&self.surface
	}
}

impl<'a, T: 'static> SurfaceCreator for WinitSurfaceCreator<'a, T> {
	type CreateArgs = &'a EventLoop<T>;
	type SurfaceOwner = WinitSurface;
	
	unsafe fn create(entry: &Entry, instance: &Instance, create_args: Self::CreateArgs) -> Result<Self, ()> {
		let xlib_surface_loader = ash::extensions::khr::XlibSurface::new(entry, instance);
		let event_loop = create_args;
		
		Ok(Self {
			xlib_surface_loader,
			event_loop,
		})
	}
	
	unsafe fn create_surface(&mut self, physical_device: vk::PhysicalDevice, device: &Device) -> Result<Self::SurfaceOwner, ()> {
		use winit::platform::unix::WindowExtUnix;
		
		let window = WindowBuilder::new()
			.build(self.event_loop)
			.expect("Failed to create winit window");
		
		let xlib_display = window.xlib_display().unwrap();
		let xlib_window = window.xlib_window().unwrap();
		let xlib_surface_create_info = vk::XlibSurfaceCreateInfoKHR::builder()
			.window(xlib_window)
			.dpy(xlib_display as *mut vk::Display);
		
		let surface = self.xlib_surface_loader.create_xlib_surface(&xlib_surface_create_info, None)
			.map_err(|e| log::error!("Failed to create xlib surface: {}", e))?;
		
		Ok(Self::SurfaceOwner {
			window,
			surface,
		})
	}
	
	unsafe fn get_size(&self, surface_owner: &Self::SurfaceOwner) -> (u32, u32) {
		let size = surface_owner.window.inner_size();
		(size.width, size.height)
	}
}