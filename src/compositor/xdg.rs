use std::{
	fmt,
};

use wl_protocols::xdg_shell::*;

use super::prelude::*;

// This object serves as the Role for a WlSurface, and so it is owned by the WlSurface. As such, it
// should not contain a strong reference to the WlSurface or a reference cycle would be created.
#[derive(Debug, Clone)]
pub struct XdgSurfaceData {
	pub parent: Resource<WlSurface>,
	pub pending_state: XdgSurfacePendingState,
	pub solid_window_geometry: Option<Rect>,
	pub xdg_surface_role: Option<XdgSurfaceRole>,
}

impl XdgSurfaceData {
	pub fn new(parent: Resource<WlSurface>) -> Self {
		Self {
			parent,
			pending_state: XdgSurfacePendingState::default(),
			solid_window_geometry: None,
			xdg_surface_role: None,
		}
	}

	pub fn commit_pending_state(&mut self) {
		if let Some(solid_window_geometry) = self.pending_state.solid_window_geometry.take() {
			self.solid_window_geometry = Some(solid_window_geometry);
		}
	}
}

#[derive(Debug, Default, Clone)]
pub struct XdgSurfacePendingState {
	solid_window_geometry: Option<Rect>,
}

#[derive(Clone)]
pub enum XdgSurfaceRole {
	XdgToplevel(Resource<XdgToplevel>),
}

impl fmt::Debug for XdgSurfaceRole {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match *self {
			XdgSurfaceRole::XdgToplevel(ref _xdg_toplevel) => f
				.debug_struct("XdgSurfaceRole::XdgToplevel")
				.field("XdgToplevel", &"<XdgToplevel>")
				.finish(),
		}
	}
}

#[derive(Debug, Clone)]
pub struct XdgToplevelData {
	pub parent: Resource<XdgSurface>,
	pub title: Option<String>,
	states: Vec<xdg_toplevel::State>,
}

impl XdgToplevelData {
	pub fn new(parent: Resource<XdgSurface>) -> Self {
		Self {
			parent,
			title: None,
			states: Vec::new(),
		}
	}

	fn set_state(&mut self, state: xdg_toplevel::State) {
		if !self.states.contains(&state) {
			self.states.push(state);
		}
	}

	fn unset_state(&mut self, state: xdg_toplevel::State) {
		self.states.retain(|test_state| *test_state != state);
	}
}

impl<I: InputBackend, G: GraphicsBackend> Compositor<I, G> {
	pub(crate) fn setup_xdg_wm_base_global(&mut self) {
		self.server.register_global(|new: NewResource<XdgWmBase>| {
			new.register_fn(
				(),
				|state, this, request| {
					let state = state.get_mut::<CompositorState<I, G>>();
					state.handle_xdg_wm_base_request(this, request);
				},
				|_state, _this| { },
			);
		});
	}
}

impl<I: InputBackend, G: GraphicsBackend> CompositorState<I, G> {
	pub fn handle_xdg_wm_base_request(&mut self, this: Resource<XdgWmBase>, request: XdgWmBaseRequest) {
		match request {
			XdgWmBaseRequest::Destroy => log::warn!("xdg_wm_base::destroy not implemented"),
			XdgWmBaseRequest::CreatePositioner(_request) => log::warn!("xdg_wm_base::create_positioner not implemented"),
			XdgWmBaseRequest::GetXdgSurface(request) => self.handle_xdg_wm_base_get_xdg_surface(this, request),
			XdgWmBaseRequest::Pong(_request) => log::warn!("xdg_wm_base::pong not implemented"),
		}
	}

	pub fn handle_xdg_wm_base_get_xdg_surface(&mut self, _this: Resource<XdgWmBase>, request: xdg_wm_base::GetXdgSurfaceRequest) {
		let xdg_surface_data = XdgSurfaceData::new(request.surface.clone());
		let xdg_surface = request.id.register_fn(
			RefCell::new(xdg_surface_data),
			|state, this, request| {
				let state = state.get_mut::<Self>();
				state.handle_xdg_surface_request(this, request);
			},
			|_state, _this| {
				log::warn!("xdg_surface destructor not implemented");
			},
		);

		let parent_surface_data: Ref<RefCell<SurfaceData<G>>> = request.surface.get_user_data();
		parent_surface_data.borrow_mut().role = Some(Role::XdgSurface(xdg_surface));
	}

	pub fn handle_xdg_surface_request(&mut self, this: Resource<XdgSurface>, request: XdgSurfaceRequest) {
		match request {
			XdgSurfaceRequest::Destroy => self.handle_xdg_surface_destroy(this),
			XdgSurfaceRequest::GetToplevel(request) => self.handle_xdg_surface_get_toplevel(this, request),
			XdgSurfaceRequest::GetPopup(_request) => log::warn!("xdg_surface::get_popup not implemented"),
			XdgSurfaceRequest::SetWindowGeometry(request) => self.handle_xdg_surface_set_window_geometry(this, request),
			XdgSurfaceRequest::AckConfigure(_request) => log::warn!("xdg_surface::ack_configure not implemented"),
		}
	}

	pub fn handle_xdg_surface_destroy(&mut self, this: Resource<XdgSurface>) {
		// TODO: should this be called automatically by the server implementation upon receiving a destruct message?
		this.destroy();
	}

	pub fn handle_xdg_surface_get_toplevel(&mut self, this: Resource<XdgSurface>, request: xdg_surface::GetToplevelRequest) {
		let xdg_toplevel_data = XdgToplevelData::new(this.clone());
		let xdg_toplevel = request.id.register_fn(
			RefCell::new(xdg_toplevel_data),
			|state, this, request| {
				let state = state.get_mut::<Self>();
				state.handle_xdg_toplevel_request(this, request);
			},
			|_state, _this| {
				log::warn!("xdg_toplevel destructor not implemented");
			},
		);

		let xdg_surface_data: Ref<RefCell<XdgSurfaceData>> = this.get_user_data();
		xdg_surface_data.borrow_mut().xdg_surface_role = Some(XdgSurfaceRole::XdgToplevel(xdg_toplevel.clone()));

		self.focus_surface_keyboard(xdg_surface_data.borrow().parent.clone());
		self.set_surface_active(xdg_surface_data.borrow().parent.clone());

		self.inner.window_manager.add_surface(xdg_surface_data.borrow().parent.clone());
	}

	pub fn handle_xdg_surface_set_window_geometry(&mut self, this: Resource<XdgSurface>, request: xdg_surface::SetWindowGeometryRequest) {
		let solid_window_geometry = Rect::new(request.x, request.y, request.width as u32, request.height as u32);
		
		let xdg_surface_data: Ref<RefCell<XdgSurfaceData>> = this.get_user_data();
		xdg_surface_data.borrow_mut().pending_state.solid_window_geometry = Some(solid_window_geometry);
	}

	pub fn handle_xdg_toplevel_request(&mut self, this: Resource<XdgToplevel>, request: XdgToplevelRequest) {
		match request {
			XdgToplevelRequest::Destroy => log::warn!("xdg_toplevel::destroy not implemented"),
			XdgToplevelRequest::SetParent(_request) => log::warn!("xdg_toplevel::set_parent not implemented"),
			XdgToplevelRequest::SetTitle(request) => self.handle_xdg_toplevel_set_title(this, request),
			XdgToplevelRequest::SetAppId(_request) => log::warn!("xdg_toplevel::set_app_id not implemented"),
			XdgToplevelRequest::ShowWindowMenu(_request) => log::warn!("xdg_toplevel::show_window_menu not implemented"),
			XdgToplevelRequest::Move(_request) => log::warn!("xdg_toplevel::move not implemented"),
			XdgToplevelRequest::Resize(_request) => log::warn!("xdg_toplevel::resize not implemented"),
			XdgToplevelRequest::SetMaxSize(_request) => log::warn!("xdg_toplevel::set_max_size not implemented"),
			XdgToplevelRequest::SetMinSize(_request) => log::warn!("xdg_toplevel::set_min_size not implemented"),
			XdgToplevelRequest::SetMaximized => log::warn!("xdg_toplevel::set_maximize not implemented"),
			XdgToplevelRequest::UnsetMaximized => log::warn!("xdg_toplevel::unset_maximized not implemented"),
			XdgToplevelRequest::SetFullscreen(_request) => log::warn!("xdg_toplevel::set_fullscreen not implemented"),
			XdgToplevelRequest::UnsetFullscreen => log::warn!("xdg_toplevel::unset_fullscreen not implemented"),
			XdgToplevelRequest::SetMinimized => log::warn!("xdg_toplevel::set_minimized not implemented"),
		}
	}

	pub fn handle_xdg_toplevel_set_title(&mut self, this: Resource<XdgToplevel>, request: xdg_toplevel::SetTitleRequest) {
		let title = String::from_utf8_lossy(&request.title).into_owned();
		this.get_user_data().borrow_mut().title = Some(title);
	}

	pub fn set_xdg_surface_active(&mut self, xdg_surface: Resource<XdgSurface>) {
		let xdg_surface_data = xdg_surface.get_user_data();
		let xdg_surface_role = xdg_surface_data.borrow().xdg_surface_role.clone();
		if let Some(role) = xdg_surface_role {
			match role {
				XdgSurfaceRole::XdgToplevel(xdg_toplevel) => self.set_xdg_toplevel_active(xdg_toplevel),
			}
		}
	}

	pub fn unset_xdg_surface_active(&mut self, xdg_surface: Resource<XdgSurface>) {
		let xdg_surface_data = xdg_surface.get_user_data();
		let xdg_surface_role = xdg_surface_data.borrow().xdg_surface_role.clone();
		if let Some(role) = xdg_surface_role {
			match role {
				XdgSurfaceRole::XdgToplevel(xdg_toplevel) => self.unset_xdg_toplevel_active(xdg_toplevel),
			}
		}
	}

	pub fn set_xdg_toplevel_active(&mut self, xdg_toplevel: Resource<XdgToplevel>) {
		let xdg_toplevel_data = xdg_toplevel.get_user_data();
		xdg_toplevel_data.borrow_mut().set_state(xdg_toplevel::State::Activated);
		self.request_xdg_toplevel_configure(xdg_toplevel.clone(), None, None);
	}

	pub fn unset_xdg_toplevel_active(&mut self, xdg_toplevel: Resource<XdgToplevel>) {
		let xdg_toplevel_data = xdg_toplevel.get_user_data();
		xdg_toplevel_data.borrow_mut().unset_state(xdg_toplevel::State::Activated);
		self.request_xdg_toplevel_configure(xdg_toplevel.clone(), None, None);
	}

	pub fn finish_xdg_surface_configure(&mut self, this: Resource<XdgSurface>) {
		let configure_event = xdg_surface::ConfigureEvent {
			serial: get_input_serial(),
		};
		this.send_event(XdgSurfaceEvent::Configure(configure_event));
	}

	pub fn request_xdg_toplevel_configure(&mut self, this: Resource<XdgToplevel>, size: Option<Size>, states: Option<&[xdg_toplevel::State]>) {
		let xdg_toplevel_data: Ref<RefCell<XdgToplevelData>> = this.get_user_data();
		let xdg_toplevel_data = xdg_toplevel_data.borrow();
		let size = size.unwrap_or(Size::new(0, 0));
		let states = states.unwrap_or_else(|| &xdg_toplevel_data.states);

		let states = states.iter().map(|state| *state as u8).collect();
		let configure_event = xdg_toplevel::ConfigureEvent {
			width: size.width as i32,
			height: size.height as i32,
			states,
		};
		this.send_event(XdgToplevelEvent::Configure(configure_event));
		self.finish_xdg_surface_configure(xdg_toplevel_data.parent.clone());
	}
}

impl_user_data!(XdgSurface, RefCell<XdgSurfaceData>);
impl_user_data!(XdgToplevel, RefCell<XdgToplevelData>);