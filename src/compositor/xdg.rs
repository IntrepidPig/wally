use std::{
	fmt,
};

use wl_protocols::xdg_shell::*;

use crate::{
	backend::{GraphicsBackend, InputBackend},
	compositor::{prelude::*, surface::SurfaceData, Compositor},
};
use super::output::OutputData;

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

	pub fn request_resize(&self, size: Size) {
		if let Some(ref xdg_surface_role) = self.xdg_surface_role {
			xdg_surface_role.request_resize(size)
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

impl XdgSurfaceRole {
	pub fn request_resize(&self, size: Size) {
		match *self {
			XdgSurfaceRole::XdgToplevel(ref xdg_toplevel) => {
				let configure_event = xdg_toplevel::ConfigureEvent {
					width: size.width as i32,
					height: size.height as i32,
					states: Vec::new(), // TODO: investigate,
				};
				xdg_toplevel.send_event(XdgToplevelEvent::Configure(configure_event));
			}
		}
	}
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
	pub title: Option<String>,
}

impl XdgToplevelData {
	pub fn new() -> Self {
		Self { title: None }
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
		let xdg_toplevel_data = XdgToplevelData::new();
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

		// Set the role of the parent XdgSurface
		let xdg_surface_data: Ref<RefCell<XdgSurfaceData>> = this.get_user_data();
		xdg_surface_data.borrow_mut().xdg_surface_role = Some(XdgSurfaceRole::XdgToplevel(xdg_toplevel.clone()));

		self.inner.window_manager.manager_impl.add_surface(xdg_surface_data.borrow().parent.clone());
		
		// Send a wl_surface::enter event for every output this surface intersects with. TODO (should this be in the surface module?)
		let surface_data = xdg_surface_data.borrow();
		let surface_data: Ref<RefCell<SurfaceData<G>>> = surface_data.parent.get_user_data();

		let client = this.client();
		let client = client.get().unwrap();
		let client_state = client.state::<RefCell<ClientState>>();
	
		for output in &client_state.borrow().outputs {
			let output_data: Ref<OutputData<G>> = output.get_user_data();
			if let Some(surface_geometry) = surface_data.borrow().try_get_surface_geometry() {
				if surface_geometry.intersects(output_data.output.viewport) {
					xdg_surface_data.borrow().parent.send_event(WlSurfaceEvent::Enter(wl_surface::EnterEvent {
						output: output.clone(),
					}));
				}
			}
		}
	}

	pub fn handle_xdg_surface_set_window_geometry(&mut self, this: Resource<XdgSurface>, request: xdg_surface::SetWindowGeometryRequest) {
		let solid_window_geometry = Rect::new(request.x, request.y, request.width as u32, request.height as u32);
		
		let xdg_surface_data: Ref<RefCell<XdgSurfaceData>> = this.get_user_data();
		xdg_surface_data.borrow_mut().solid_window_geometry = Some(solid_window_geometry);
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
}

impl_user_data!(XdgSurface, RefCell<XdgSurfaceData>);
impl_user_data!(XdgToplevel, RefCell<XdgToplevelData>);