use std::{
	fmt,
	sync::{Arc, Mutex},
};

use wayland_protocols::xdg_shell::server::{xdg_popup, xdg_positioner, xdg_surface, xdg_toplevel, xdg_wm_base};
use wayland_server::{Filter, Main};

use crate::{
	backend::{GraphicsBackend, InputBackend},
	compositor::{prelude::*, role::Role, surface::SurfaceData, Compositor},
	renderer::Output,
};

#[derive(Debug, Default, Clone)]
pub struct XdgSurfacePendingState {
	solid_window_geometry: Option<Rect>,
}

// This object serves as the Role for a WlSurface, and so it is owned by the WlSurface. As such, it
// should not contain a strong reference to the WlSurface or a reference cycle would be created.
#[derive(Debug, Clone)]
pub struct XdgSurfaceData {
	pub pending_state: XdgSurfacePendingState,
	pub solid_window_geometry: Option<Rect>,
	pub xdg_surface_role: Option<XdgSurfaceRole>,
}

#[derive(Clone)]
pub enum XdgSurfaceRole {
	XdgToplevel(xdg_toplevel::XdgToplevel),
}

impl XdgSurfaceRole {
	pub fn resize_window(&self, size: Size) {
		match *self {
			XdgSurfaceRole::XdgToplevel(ref xdg_toplevel) => {
				xdg_toplevel.configure(size.width as i32, size.height as i32, Vec::new());
			}
		}
	}
}

impl XdgSurfaceData {
	pub fn new() -> Self {
		Self {
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

	pub fn resize_window(&mut self, size: Size) {
		self.xdg_surface_role
			.as_mut()
			.map(|xdg_surface_role| xdg_surface_role.resize_window(size));
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

impl<I: InputBackend + 'static, G: GraphicsBackend + 'static> Compositor<I, G> {
	pub(crate) fn setup_xdg_wm_base_global(&mut self) {
		let inner = Arc::clone(&self.inner);
		let xdg_wm_base_filter = Filter::new(
			move |(main, _num): (Main<xdg_wm_base::XdgWmBase>, u32), _filter, _dispatch_data| {
				let inner = Arc::clone(&inner);
				main.quick_assign(move |_main, request: xdg_wm_base::Request, _| {
					let inner = Arc::clone(&inner);
					match request {
						xdg_wm_base::Request::Destroy => {}
						xdg_wm_base::Request::CreatePositioner { id } => {
							id.quick_assign(
								|_main: Main<xdg_positioner::XdgPositioner>, request: xdg_positioner::Request, _| {
									match request {
										xdg_positioner::Request::Destroy => {}
										xdg_positioner::Request::SetSize { .. } => {}
										xdg_positioner::Request::SetAnchorRect { .. } => {}
										xdg_positioner::Request::SetAnchor { .. } => {}
										xdg_positioner::Request::SetGravity { .. } => {}
										xdg_positioner::Request::SetConstraintAdjustment { .. } => {}
										xdg_positioner::Request::SetOffset { .. } => {}
										_ => {
											log::warn!("Got unknown request for xdg_positioner");
										}
									}
								},
							);
						}
						xdg_wm_base::Request::GetXdgSurface {
							id: xdg_surface_id,
							surface,
						} => {
							log::trace!("Creating xdg_surface");
							let xdg_surface = (*xdg_surface_id).clone();
							let xdg_surface_data = Arc::new(Mutex::new(XdgSurfaceData::new()));
							let xdg_surface_data_clone = Arc::clone(&xdg_surface_data);
							xdg_surface
								.as_ref()
								.user_data()
								.set_threadsafe(move || xdg_surface_data_clone);
							xdg_surface_id.quick_assign(
								move |_main: Main<xdg_surface::XdgSurface>, request: xdg_surface::Request, _| {
									let inner = Arc::clone(&inner);
									match request {
										xdg_surface::Request::GetToplevel { id: xdg_toplevel_id } => {
											// Set the xdg toplevel data
											let xdg_toplevel = (*xdg_toplevel_id).clone();
											let xdg_toplevel_data = Arc::new(Mutex::new(XdgToplevelData::new()));
											let xdg_toplevel_data_clone = Arc::clone(&xdg_toplevel_data);
											xdg_toplevel
												.as_ref()
												.user_data()
												.set_threadsafe(move || xdg_toplevel_data_clone);

											// Now that the surface has been assigned as a toplevel we assign the role to the wl_surface and the xdg_surface
											let surface_data = surface.get_synced::<SurfaceData<G>>();
											let mut surface_data_lock = surface_data.lock().unwrap();
											surface_data_lock.role = Some(Role::XdgSurface(xdg_surface.clone()));
											drop(surface_data_lock);
											let mut xdg_surface_data_lock = xdg_surface_data.lock().unwrap();
											xdg_surface_data_lock.xdg_surface_role =
												Some(XdgSurfaceRole::XdgToplevel(xdg_toplevel.clone()));
											drop(xdg_surface_data_lock);

											let mut inner_lock = inner.lock().unwrap();
											inner_lock.window_manager.manager_impl.add_surface(surface.clone());

											// Send output enter events for every output viewport this surface intersects
											// TODO: handle surface moves and possibly output viewport changes
											let surface_data = surface.get_synced::<SurfaceData<G>>();
											let surface_data_lock = surface_data.lock().unwrap();
											let client_info = inner_lock
												.client_manager
												.get_client_info(xdg_toplevel.as_ref().client().unwrap());
											let client_info_lock = client_info.lock().unwrap();
											for output in &client_info_lock.outputs {
												let output_data = output.get::<Output<G>>();
												if let Some(surface_geometry) =
													surface_data_lock.try_get_surface_geometry()
												{
													if surface_geometry.intersects(output_data.viewport) {
														surface.enter(output);
													}
												}
											}

											xdg_toplevel_id.quick_assign(
												move |_main, request: xdg_toplevel::Request, _| {
													let toplevel_data = Arc::clone(&xdg_toplevel_data);
													match request {
														xdg_toplevel::Request::SetParent { .. } => {}
														xdg_toplevel::Request::SetTitle { title } => {
															let mut toplevel_data_lock = toplevel_data.lock().unwrap();
															toplevel_data_lock.title = Some(title);
														}
														xdg_toplevel::Request::SetAppId { .. } => {}
														xdg_toplevel::Request::ShowWindowMenu { .. } => {}
														xdg_toplevel::Request::Move {
															seat: _seat,
															serial: _serial,
														} => {}
														xdg_toplevel::Request::Resize {
															seat: _seat,
															serial: _serail,
															edges: _edges,
														} => {}
														xdg_toplevel::Request::SetMaxSize { .. } => {}
														xdg_toplevel::Request::SetMinSize { .. } => {}
														xdg_toplevel::Request::SetMaximized => {}
														xdg_toplevel::Request::UnsetMaximized => {}
														xdg_toplevel::Request::SetFullscreen { .. } => {}
														xdg_toplevel::Request::UnsetFullscreen => {}
														xdg_toplevel::Request::SetMinimized => {}
														_ => {
															log::warn!("Got unknown request for xdg_toplevel");
														}
													}
												},
											);
										}
										xdg_surface::Request::GetPopup {
											id,
											parent: _parent,
											positioner: _positioner,
										} => id.quick_assign(
											move |_main, request: xdg_popup::Request, _| match request {
												xdg_popup::Request::Destroy => {}
												xdg_popup::Request::Grab { .. } => {}
												xdg_popup::Request::Reposition { .. } => {}
												_ => log::warn!("Got unknown request for xdg_popup"),
											},
										),
										xdg_surface::Request::SetWindowGeometry { x, y, width, height } => {
											let solid_window_geometry = Rect::new(x, y, width as u32, height as u32);
											let mut xdg_surface_data_lock = xdg_surface_data.lock().unwrap();
											xdg_surface_data_lock.solid_window_geometry = Some(solid_window_geometry);
										}
										xdg_surface::Request::AckConfigure { .. } => {}
										_ => log::warn!("Got unknown request for xdg_surface"),
									}
								},
							);
						}
						xdg_wm_base::Request::Pong { .. } => {}
						_ => {
							log::warn!("Got unknown request for xdg_wm_base");
						}
					}
				});
			},
		);
		self.display
			.create_global::<xdg_wm_base::XdgWmBase, _>(2, xdg_wm_base_filter);
	}
}
