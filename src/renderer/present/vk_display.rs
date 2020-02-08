use ash::{
	vk::{self},
	Device, Entry, Instance,
};

use crate::renderer::present::SurfaceCreator;

pub struct DisplaySurfaceCreator {
	display_loader: ash::extensions::khr::Display,
}

#[derive(Debug, Copy, Clone)]
pub struct DisplaySurface {
	display_properties: vk::DisplayPropertiesKHR,
	display_mode_properties: vk::DisplayModePropertiesKHR,
	display_plane_index: u32,
	surface: vk::SurfaceKHR,
}

impl AsRef<vk::SurfaceKHR> for DisplaySurface {
	fn as_ref(&self) -> &vk::SurfaceKHR {
		&self.surface
	}
}

impl SurfaceCreator for DisplaySurfaceCreator {
	type CreateArgs = (); // TODO modeset selection closure and other config
	type ReturnVal = ();
	type SurfaceOwner = DisplaySurface;

	unsafe fn create(
		entry: &Entry,
		instance: &Instance,
		create_args: Self::CreateArgs,
	) -> Result<(Self, Self::ReturnVal), ()> {
		let display_loader = ash::extensions::khr::Display::new(entry, instance);

		Ok((Self { display_loader }, ()))
	}

	unsafe fn create_surface(
		&mut self,
		physical_device: vk::PhysicalDevice,
		device: &Device,
	) -> Result<Self::SurfaceOwner, ()> {
		let displays_properties = self
			.display_loader
			.get_physical_device_display_properties(physical_device)
			.map_err(|e| log::error!("Failed to get display properties: {}", e))?;
		if displays_properties.is_empty() {
			log::error!("No displays available");
			return Err(());
		}
		let display_properties = displays_properties[0];
		let display_planes_properties = self
			.display_loader
			.get_physical_device_display_plane_properties(physical_device)
			.unwrap();
		let (display_plane_index, display_plane_properties) = display_planes_properties
			.iter()
			.enumerate()
			.find(|(index, properties)| {
				let supported_displays = self
					.display_loader
					.get_display_plane_supported_displays(physical_device, *index as u32)
					.unwrap();
				supported_displays
					.iter()
					.any(|display| *display == display_properties.display)
			})
			.map(|(index, properties)| (index as u32, properties))
			.unwrap();
		let display_modes_properties = self
			.display_loader
			.get_display_mode_properties(physical_device, display_properties.display)
			.unwrap();
		let display_mode_properties = display_modes_properties[0];
		let display_plane_capabilities = self
			.display_loader
			.get_display_plane_capabilities(
				physical_device,
				display_mode_properties.display_mode,
				display_plane_index,
			)
			.unwrap();
		let surface_create_info = vk::DisplaySurfaceCreateInfoKHR::builder()
			.display_mode(display_mode_properties.display_mode)
			.plane_index(display_plane_index)
			.plane_stack_index(0)
			.transform(vk::SurfaceTransformFlagsKHR::IDENTITY)
			.alpha_mode(vk::DisplayPlaneAlphaFlagsKHR::OPAQUE)
			.image_extent(display_mode_properties.parameters.visible_region);
		let surface = self
			.display_loader
			.create_display_plane_surface(&surface_create_info, None)
			.unwrap();

		let surface_owner = Self::SurfaceOwner {
			display_properties,
			display_mode_properties,
			display_plane_index,
			surface,
		};

		Ok(surface_owner)
	}

	unsafe fn get_size(&self, surface_owner: &Self::SurfaceOwner) -> (u32, u32) {
		(
			surface_owner.display_properties.physical_resolution.width,
			surface_owner.display_properties.physical_resolution.height,
		)
	}
}
