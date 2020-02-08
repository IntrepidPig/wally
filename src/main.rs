//#![allow(warnings)]
//#![allow(unused)]

use calloop::EventLoop;
/*use dbus::{
	arg::{RefArg},
};*/
use structopt::StructOpt;

use crate::{
	backend::{vulkan::VulkanRenderBackend, winit::WinitInputBackend},
	renderer::present::{
		vk_display::DisplaySurfaceCreator,
		winit::WinitSurfaceCreator,
		//drm::{DrmPresentBackend},
		GenericPresentBackend,
	},
};

pub mod backend;
pub mod compositor;
pub mod logind;
pub mod math;
pub mod renderer;
//pub mod behavior;
//pub mod ffi;

#[derive(StructOpt)]
#[structopt(name = "wally", about = "A wayland compositor")]
pub struct Opts {
	#[structopt(short, long, help = "Select the backend. Can be either \"winit\" or \"vk_display\"")]
	backend: String,
}

fn main() {
	setup_logging().expect("Failed to setup logging dispatch");

	let mut event_loop = EventLoop::<()>::new().expect("Failed to create event loop");
	let opts = Opts::from_args();
	match opts.backend.as_str() {
		"winit" => {
			start_winit_compositor(event_loop);
		}
		"vk_display" => {
			start_vk_display_compositor(event_loop);
		}
		u => {
			eprintln!("Unknown backend '{}'", u);
			return;
		}
	}

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
}

#[allow(unused)]
fn start_winit_compositor(event_loop: calloop::EventLoop<()>) {
	let winit_event_loop = winit::event_loop::EventLoop::new();
	let window = winit::window::Window::new(&winit_event_loop).unwrap();
	let (mut renderer, mut present_backend, window) =
		renderer::Renderer::new::<GenericPresentBackend<WinitSurfaceCreator>>(window)
			.expect("Failed to initialize renderer");

	let render_backend = VulkanRenderBackend::new(renderer, present_backend);

	let (tx, rx) = std::sync::mpsc::channel();
	std::thread::spawn(move || {
		let input_backend = WinitInputBackend::new();
		let sender = input_backend.get_sender();
		tx.send(sender);
		let mut backend = backend::create_backend(input_backend, render_backend);
		let mut event_loop = calloop::EventLoop::new().expect("Failed to create event loop");
		let handle = event_loop.handle();
		let mut compositor = compositor::Compositor::new(backend, handle).expect("Failed to initialize compositor");
		compositor.init();
		compositor.start(&mut event_loop);
	});
	let sender = rx.recv().unwrap();
	WinitInputBackend::start(sender, winit_event_loop);
}

#[allow(unused)]
fn start_vk_display_compositor(event_loop: calloop::EventLoop<()>) {
	let (mut renderer, mut present_backend, window) =
		renderer::Renderer::new::<GenericPresentBackend<DisplaySurfaceCreator>>(())
			.expect("Failed to initialize renderer");
	let mut event_loop = calloop::EventLoop::new().expect("Failed to create event loop");
	let render_backend = VulkanRenderBackend::new(renderer, present_backend);
	let input_backend = backend::libinput::LibinputInputBackend::new(event_loop.handle()).expect("Failed to create libinput backend");
	let mut backend = backend::create_backend(input_backend, render_backend);
	let mut compositor = compositor::Compositor::new(backend, event_loop.handle()).expect("Failed to initialize compositor");
	compositor.init();
	compositor.start(&mut event_loop);
}

fn setup_logging() -> Result<(), ()> {
	fern::Dispatch::new()
		.format(|out, message, record| {
			out.finish(format_args!("[{}][{}] {}", record.target(), record.level(), message))
		})
		.level(log::LevelFilter::Trace)
		.chain(std::io::stderr())
		.apply()
		.map_err(|_| ())
}
