use wl_server::{
	protocol::*,
};

use crate::{
	compositor::prelude::*,
};

#[derive(Debug)]
pub struct ClientState {
	pub(crate) keyboards: Vec<Resource<WlKeyboard>>,
	pub(crate) pointers: Vec<Resource<WlPointer>>,
	pub(crate) outputs: Vec<Resource<WlOutput>>,
}

impl ClientState {
	pub fn new() -> Self {
		Self {
			keyboards: Vec::new(),
			pointers: Vec::new(),
			outputs: Vec::new(),
		}
	}
}