use wayland_server::{protocol::*, Client};

pub struct ClientInfo {
	pub(crate) client: Client,
	pub(crate) keyboards: Vec<wl_keyboard::WlKeyboard>,
	pub(crate) pointers: Vec<wl_pointer::WlPointer>,
	pub(crate) outputs: Vec<wl_output::WlOutput>,
}
