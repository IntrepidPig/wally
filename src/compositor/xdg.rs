use std::sync::{Arc, Mutex};

use wayland_protocols::xdg_shell::server::{xdg_positioner, xdg_surface, xdg_toplevel, xdg_wm_base};
use wayland_server::{Filter, Main};

use crate::{
	backend::{InputBackend, RenderBackend},
	compositor::{role::Role, surface::SurfaceData, Compositor},
};

// This object serves as the Role for a WlSurface, and so it is owned by the WlSurface. As such, it
// should not contain a strong reference to the WlSurface or a reference cycle would be created.
#[derive(Debug, Clone)]
pub struct XdgSurfaceData {}

impl XdgSurfaceData {
	pub fn new() -> Self {
		Self {}
	}
}

#[derive(Debug, Clone)]
pub struct XdgToplevelData {
	pub title: Option<String>,
	pub pos: (i32, i32),
	pub size: (u32, u32),
}

impl XdgToplevelData {
	pub fn new() -> Self {
		Self {
			title: None,
			pos: (20, 20),
			size: (400, 300),
		}
	}
}

impl<I: InputBackend, R: RenderBackend> Compositor<I, R> {
	pub(crate) fn setup_xdg_wm_base_global(&mut self) {
		let xdg_wm_base_filter = Filter::new(
			|(main, _num): (Main<xdg_wm_base::XdgWmBase>, u32), _filter, _dispatch_data| {
				main.quick_assign(|_main, request: xdg_wm_base::Request, _| match request {
					xdg_wm_base::Request::Destroy => {
						log::debug!("Got xdg_wm_base destroy request");
					}
					xdg_wm_base::Request::CreatePositioner { id } => {
						log::debug!("Got xdg_wm_base create_positioner request");
						id.quick_assign(
							|_main: Main<xdg_positioner::XdgPositioner>, request: xdg_positioner::Request, _| {
								match request {
									xdg_positioner::Request::Destroy => {
										log::debug!("Got xdg_positioner destroy request");
									}
									xdg_positioner::Request::SetSize { width, height } => {
										log::debug!("Got xdg_positioner set_size request for {}x{}", width, height);
									}
									xdg_positioner::Request::SetAnchorRect { x, y, width, height } => {
										log::debug!(
											"Got xdg_positioner set_anchor_rect request for ({}, {}), {}x{}",
											x,
											y,
											width,
											height
										);
									}
									xdg_positioner::Request::SetAnchor { anchor } => {
										log::debug!("Got xdg_positioner set anchor request: {:?}", anchor);
									}
									xdg_positioner::Request::SetGravity { gravity } => {
										log::debug!("Got xdg_positioner set gravity request: {:?}", gravity);
									}
									xdg_positioner::Request::SetConstraintAdjustment { constraint_adjustment } => {
										log::debug!("Got xdg_positioner set constraint adjustment request: {:?}", constraint_adjustment);
									}
									xdg_positioner::Request::SetOffset { x, y } => {
										log::debug!("Got xdg_positioner set offset request: ({}, {})", x, y);
									}
									_ => {
										log::warn!("Got unknown request for xdg_positioner");
									}
								}
							},
						);
					}
					xdg_wm_base::Request::GetXdgSurface { id, surface } => {
						log::debug!("Got xdg_wm_base get_xdg_surface request");
						let xdg_surface_data = Arc::new(Mutex::new(XdgSurfaceData::new()));
						let xdg_surface_data_clone = Arc::new(Mutex::new(XdgSurfaceData::new()));
						let xdg_surface = (*id).clone();
						xdg_surface
							.as_ref()
							.user_data()
							.set_threadsafe(move || xdg_surface_data_clone);
						(*id).configure(42);
						id.quick_assign(
							move |_main: Main<xdg_surface::XdgSurface>, request: xdg_surface::Request, _| {
								let xdg_surface_data = Arc::clone(&xdg_surface_data);
								let surface = surface.clone();
								match request {
									xdg_surface::Request::Destroy => {
										log::debug!("Got xdg_surface destroy request");
									}
									xdg_surface::Request::GetToplevel { id } => {
										log::debug!("Got xdg_surface get_top_level request");
										let toplevel = (*id).clone();
										let toplevel_data = XdgToplevelData::new();
										toplevel.configure(
											toplevel_data.size.0 as i32,
											toplevel_data.size.1 as i32,
											Vec::new(),
										);
										let toplevel_data = Arc::new(Mutex::new(toplevel_data));
										let toplevel_data_clone = Arc::clone(&toplevel_data);
										toplevel
											.as_ref()
											.user_data()
											.set_threadsafe(move || toplevel_data_clone);
										let surface_data: &Arc<Mutex<SurfaceData<R::ObjectHandle>>> = surface
											.as_ref()
											.user_data()
											.get::<Arc<Mutex<SurfaceData<R::ObjectHandle>>>>()
											.unwrap();
										let mut surface_data_lock = surface_data.lock().unwrap();
										surface_data_lock.role = Some(Role::XdgToplevel(toplevel.clone()));
										id.quick_assign(move |_main, request: xdg_toplevel::Request, _| {
											let toplevel_data = Arc::clone(&toplevel_data);
											match request {
												xdg_toplevel::Request::Destroy => {
													log::debug!("Got xdg_toplevel destroy request");
												}
												xdg_toplevel::Request::SetParent { parent } => {
													log::debug!("Got xdg_toplevel set_parent request on {:?}", parent.map(|parent| parent.as_ref().id()));
												}
												xdg_toplevel::Request::SetTitle { title } => {
													log::debug!("Got xdg_toplevel set_title request");
													let mut toplevel_data_lock = toplevel_data.lock().unwrap();
													toplevel_data_lock.title = Some(title);
												}
												xdg_toplevel::Request::SetAppId { app_id } => {
													log::debug!("Got xdg_toplevel set_app_id request: '{}'", app_id);
												}
												xdg_toplevel::Request::ShowWindowMenu { .. } => {
													log::debug!("Got xdg_toplevel show_window_meny request");
												}
												xdg_toplevel::Request::Move { .. } => {
													log::debug!("Got xdg_toplevel move request");
												}
												xdg_toplevel::Request::Resize { .. } => {
													log::debug!("Got xdg_toplevel resize request");
												}
												xdg_toplevel::Request::SetMaxSize { .. } => {
													log::debug!("Got xdg_toplevel set_max_size request");
												}
												xdg_toplevel::Request::SetMinSize { .. } => {
													log::debug!("Got xdg_toplevel set_min_size request");
												}
												xdg_toplevel::Request::SetMaximized => {
													log::debug!("Got xdg_toplevel set_maximized request");
												}
												xdg_toplevel::Request::UnsetMaximized => {
													log::debug!("Got xdg_toplevel unset_maximized request");
												}
												xdg_toplevel::Request::SetFullscreen { .. } => {
													log::debug!("Got xdg_toplevel set_fullscreen request");
												}
												xdg_toplevel::Request::UnsetFullscreen => {
													log::debug!("Got xdg_toplevel unset_fullscreen request");
												}
												xdg_toplevel::Request::SetMinimized => {
													log::debug!("Got xdg_toplevel set_minimized request");
												}
												_ => {
													log::warn!("Got unknown request for xdg_toplevel");
												}
											}
										});
									}
									xdg_surface::Request::GetPopup { .. } => {
										log::debug!("Got xdg_surface get_popup request");
									}
									xdg_surface::Request::SetWindowGeometry { .. } => {
										log::debug!("Got xdg_surface set_window_geometry request");
									}
									xdg_surface::Request::AckConfigure { .. } => {
										log::debug!("Got xdg_surface ack_configure request");
									}
									_ => log::warn!("Got unknown request for xdg_surface"),
								}
							},
						);
					}
					xdg_wm_base::Request::Pong { serial } => {
						log::debug!("Got xdg_wm_base pong request with serial: {}", serial);
					}
					_ => {
						log::warn!("Got unknown request for xdg_wm_base");
					}
				});
			},
		);
		self.display
			.create_global::<xdg_wm_base::XdgWmBase, _>(2, xdg_wm_base_filter);
	}
}
