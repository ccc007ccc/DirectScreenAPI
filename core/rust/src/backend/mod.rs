use crate::api::{DisplayState, RouteResult, Status};

pub trait RenderBackend: Send {
    fn on_display_changed(&mut self, _display: DisplayState) -> Result<(), Status> {
        Ok(())
    }
}

pub trait InputBackend: Send {
    fn on_route_result(&mut self, _result: RouteResult) -> Result<(), Status> {
        Ok(())
    }
}

#[derive(Default)]
pub struct NullBackend;

impl RenderBackend for NullBackend {}
impl InputBackend for NullBackend {}
