use std::{
	time::{Duration},
	fs::{self},
	env::{self},
	any::{Any},
};

use wayland_server::{
	Display,
	protocol::{
		*,
	}
};
use wayland_protocols::{
	xdg_shell::server::{
		xdg_wm_base, xdg_surface,
	},
};

use calloop::{
	EventLoop,
};
use dbus::{
	arg::{RefArg},
};
use std::path::Path;

use crate::{
	renderer::{
		present::{
			PresentBackend, GenericPresentBackend,
			vk_display::{DisplaySurfaceCreator},
			winit::{WinitSurfaceCreator},
			//drm::{DrmPresentBackend},
		}
	}
};
use winit::event_loop::ControlFlow;

pub mod xdg;
pub mod compositor;
pub mod backend;
pub mod renderer;
pub mod logind;
//pub mod ffi;

fn main() {
	setup_logging().expect("Failed to setup logging dispatch");
	
	
	let mut event_loop = EventLoop::<()>::new().expect("Failed to create event loop");
	let handle = event_loop.handle();
	
	/*let dbus_system: dbus::blocking::Connection = dbus::blocking::Connection::new_system().unwrap();
	let logind = dbus_system.with_proxy("org.freedesktop.login1", "/org/freedesktop/login1", Duration::from_secs(5));
	use logind::OrgFreedesktopLogin1Manager;
	let seats = logind.list_seats().unwrap();
	for seat in seats {
		println!("Found seat");
		println!("\tname: {}", seat.0);
	}
	println!("{:?}", logind.get_seat("seat0"));
	*/
	//let kb = libinput.path_add_device("/dev/input/by-id/ckb-Corsair_Gaming_K70_LUX_RGB_Keyboard_vKB_-event").unwrap();
	
	//let input_backend = backend::libinput::LibinputInputBackend::new(handle.clone()).expect("Failed to initialize libinput input backend");
	let input_backend = backend::headless::HeadlessInputBackend { };
	//let render_backend = backend::drm::DrmRenderBackend::new("/dev/dri/card0").expect("Failed to initialize DRM render backend");
	let render_backend = backend::headless::HeadlessRenderBackend { };
	//let renderer = renderer::Renderer::new(render_backend.framebuffer_dma_buf_fd()).expect("Failed to initialize renderer");
	//let event_loop = winit::event_loop::EventLoop::new();
	let (mut renderer, mut present_backend) = renderer::Renderer::new::<DisplaySurfaceCreator, GenericPresentBackend<_>>((), ()).expect("Failed to initialize renderer");
	/*let raw_card = crate::backend::drm::Card::new().map_err(|e| {
		log::error!("DRM backend failed to open device: {}", e);
	}).unwrap();*/
	//log::trace!("Creating gbm device");
	//let card = gbm::Device::new(raw_card).expect("Failed to create gbm_device");
	//backend::drm::DrmRenderBackend::new(card);
	
	/*log::trace!("Created gbm device successfully");
	let (mut renderer, mut drm_present_backend) = renderer::Renderer::new::<DrmPresentBackend>(card).expect("Failed to initialize renderer");
	//let render_backend = backend::drm::DrmRenderBackend::new("/dev/dri/card0", dbg!(renderer.memory_fd())).expect("Failed to initialize DRM render backend");*/
	
	/*event_loop.run(move |event, _window_target, control_flow| {
		unsafe { renderer.render(&mut present_backend) };
		*control_flow = match event {
			winit::event::Event::WindowEvent { event: winit::event::WindowEvent::CloseRequested, .. } => ControlFlow::Exit,
			_ => ControlFlow::Poll,
		};
	});*/
	
	let start = std::time::Instant::now();
	loop {
		unsafe { renderer.render(&mut present_backend).unwrap(); }
		std::thread::sleep(std::time::Duration::from_nanos(1_000_000_000 / 60));
		if start.elapsed() > std::time::Duration::from_secs(5) {
			break;
		}
	}
	/*let mut backend = backend::create_backend(input_backend, render_backend);
	
	let mut compositor = compositor::Compositor::new(backend, handle).expect("Failed to initialize compositor");
	compositor.init();
	compositor.start(&mut event_loop);*/
}

fn setup_logging() -> Result<(), ()> {
	fern::Dispatch::new()
		.format(|out, message, record| out.finish(format_args!("[{}][{}] {}", record.target(), record.level(), message)))
		.level(log::LevelFilter::Trace)
		.chain(std::io::stderr())
		.apply()
		.map_err(|_| ())
}