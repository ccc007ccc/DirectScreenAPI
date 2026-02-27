#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Status {
    Ok = 0,
    NullPointer = 1,
    InvalidArgument = 2,
    OutOfRange = 3,
    InternalError = 4,
}

pub const TOUCH_MAX_POINTERS: usize = 16;
pub const RENDER_MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RenderStats {
    pub frame_seq: u64,
    pub draw_calls: u32,
    pub frost_passes: u32,
    pub text_calls: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RenderFrameInfo {
    pub frame_seq: u64,
    pub width: u32,
    pub height: u32,
    pub byte_len: u32,
    pub checksum_fnv1a32: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Decision {
    Pass = 0,
    Block = 1,
}

impl Decision {
    pub fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Pass),
            1 => Some(Self::Block),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayState {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: f32,
    pub density_dpi: u32,
    pub rotation: u32,
}

impl Default for DisplayState {
    fn default() -> Self {
        Self {
            width: 1080,
            height: 2400,
            refresh_hz: 60.0,
            density_dpi: 420,
            rotation: 0,
        }
    }
}

impl DisplayState {
    pub fn validate(&self) -> Result<(), Status> {
        if self.width == 0 || self.height == 0 {
            return Err(Status::InvalidArgument);
        }
        if !self.refresh_hz.is_finite() || self.refresh_hz <= 0.0 {
            return Err(Status::OutOfRange);
        }
        if self.density_dpi == 0 {
            return Err(Status::OutOfRange);
        }
        if self.rotation > 3 {
            return Err(Status::OutOfRange);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectRegion {
    pub region_id: i32,
    pub decision: Decision,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl RectRegion {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        let right = self.x + self.w;
        let bottom = self.y + self.h;
        px >= self.x && px <= right && py >= self.y && py <= bottom
    }

    pub fn validate(&self) -> Result<(), Status> {
        if !self.x.is_finite() || !self.y.is_finite() || !self.w.is_finite() || !self.h.is_finite()
        {
            return Err(Status::InvalidArgument);
        }
        if self.w <= 0.0 || self.h <= 0.0 {
            return Err(Status::OutOfRange);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteResult {
    pub decision: Decision,
    pub region_id: i32,
}

impl RouteResult {
    pub fn passthrough() -> Self {
        Self {
            decision: Decision::Pass,
            region_id: -1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TouchEvent {
    pub pointer_id: i32,
    pub x: f32,
    pub y: f32,
}

impl TouchEvent {
    pub fn validate(&self) -> Result<(), Status> {
        if self.pointer_id < 0 {
            return Err(Status::OutOfRange);
        }
        if !self.x.is_finite() || !self.y.is_finite() {
            return Err(Status::InvalidArgument);
        }
        Ok(())
    }
}
