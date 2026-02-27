use std::collections::HashMap;

use crate::api::{
    Decision, DisplayState, RectRegion, RouteResult, Status, TouchEvent, TOUCH_MAX_POINTERS,
};

#[derive(Debug, Clone, Copy)]
struct ActiveTouch {
    x: f32,
    y: f32,
    routed: RouteResult,
}

#[derive(Debug)]
pub struct RuntimeState {
    pub display: DisplayState,
    pub default_decision: Decision,
    pub regions: Vec<RectRegion>,
    active_touches: HashMap<i32, ActiveTouch>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            display: DisplayState::default(),
            default_decision: Decision::Pass,
            regions: Vec::new(),
            active_touches: HashMap::new(),
        }
    }
}

impl RuntimeState {
    pub fn set_display(&mut self, display: DisplayState) -> Result<(), Status> {
        display.validate()?;
        self.display = display;
        Ok(())
    }

    pub fn clear_regions(&mut self) {
        self.regions.clear();
    }

    pub fn clear_touches(&mut self) {
        self.active_touches.clear();
    }

    pub fn add_rect_region(&mut self, region: RectRegion) -> Result<(), Status> {
        region.validate()?;
        self.regions.push(region);
        Ok(())
    }

    pub fn route_point(&self, x: f32, y: f32) -> RouteResult {
        for region in self.regions.iter().rev() {
            if region.contains(x, y) {
                return RouteResult {
                    decision: region.decision,
                    region_id: region.region_id,
                };
            }
        }

        RouteResult {
            decision: self.default_decision,
            region_id: -1,
        }
    }

    pub fn touch_down(&mut self, event: TouchEvent) -> Result<RouteResult, Status> {
        event.validate()?;

        if self.active_touches.contains_key(&event.pointer_id) {
            return Err(Status::InvalidArgument);
        }
        if self.active_touches.len() >= TOUCH_MAX_POINTERS {
            return Err(Status::OutOfRange);
        }

        let routed = self.route_point(event.x, event.y);
        self.active_touches.insert(
            event.pointer_id,
            ActiveTouch {
                x: event.x,
                y: event.y,
                routed,
            },
        );
        Ok(routed)
    }

    pub fn touch_move(&mut self, event: TouchEvent) -> Result<RouteResult, Status> {
        event.validate()?;
        let touch = self
            .active_touches
            .get_mut(&event.pointer_id)
            .ok_or(Status::OutOfRange)?;

        touch.x = event.x;
        touch.y = event.y;
        Ok(touch.routed)
    }

    pub fn touch_up(&mut self, event: TouchEvent) -> Result<RouteResult, Status> {
        event.validate()?;
        let touch = self
            .active_touches
            .remove(&event.pointer_id)
            .ok_or(Status::OutOfRange)?;
        Ok(touch.routed)
    }

    pub fn touch_cancel(&mut self, pointer_id: i32) -> Result<RouteResult, Status> {
        if pointer_id < 0 {
            return Err(Status::OutOfRange);
        }
        let touch = self
            .active_touches
            .remove(&pointer_id)
            .ok_or(Status::OutOfRange)?;
        Ok(touch.routed)
    }

    pub fn active_touch_count(&self) -> usize {
        self.active_touches.len()
    }
}
