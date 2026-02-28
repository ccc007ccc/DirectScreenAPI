use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use super::core::{parse_ok_tokens, SocketConnection};
use super::error::{DsapiError, Result};

#[derive(Debug)]
pub struct RenderSession {
    connection: SocketConnection,
    display_width: u32,
    display_height: u32,
    logical_width: u32,
    logical_height: u32,
    rotation: u32,
    refresh_hz: f32,
    fps_cap: Option<f32>,
    pacing_enabled: bool,
    next_deadline: Option<Instant>,
    submit_ema_secs: f64,
    rotate_scratch: Vec<u8>,
    last_frame_seq: u64,
}

impl RenderSession {
    pub(crate) fn connect(
        socket_path: PathBuf,
        width: u32,
        height: u32,
        rotation: u32,
        refresh_hz: f32,
    ) -> Result<Self> {
        if width == 0 || height == 0 || rotation > 3 {
            return Err(DsapiError::ParseError(
                "render_session_invalid_display_size".to_string(),
            ));
        }
        let connection = SocketConnection::connect(socket_path.as_path())?;
        let (logical_width, logical_height) = logical_size(width, height, rotation);
        Ok(Self {
            connection,
            display_width: width,
            display_height: height,
            logical_width,
            logical_height,
            rotation,
            refresh_hz: sanitize_refresh_hz(refresh_hz),
            fps_cap: None,
            pacing_enabled: true,
            next_deadline: None,
            submit_ema_secs: 0.0,
            rotate_scratch: Vec::new(),
            last_frame_seq: 0,
        })
    }

    pub fn submit_frame(&mut self, rgba_buffer: &[u8]) -> Result<()> {
        let expected_len = expected_rgba_len(self.logical_width, self.logical_height)?;
        if rgba_buffer.len() != expected_len {
            return Err(DsapiError::ParseError(format!(
                "rgba_len_mismatch logical={}x{} expected={} actual={}",
                self.logical_width,
                self.logical_height,
                expected_len,
                rgba_buffer.len()
            )));
        }

        self.pace_before_submit();
        let submit_started = Instant::now();

        if self.rotation == 0 {
            self.submit_display_frame(rgba_buffer, self.display_width, self.display_height)?;
        } else {
            let expected_display_len = expected_rgba_len(self.display_width, self.display_height)?;
            let mut rotated = std::mem::take(&mut self.rotate_scratch);
            rotated.resize(expected_display_len, 0);
            rotate_logical_rgba_to_display(
                rgba_buffer,
                self.logical_width,
                self.logical_height,
                self.display_width,
                self.display_height,
                self.rotation,
                &mut rotated,
            )?;
            self.submit_display_frame(&rotated, self.display_width, self.display_height)?;
            self.rotate_scratch = rotated;
        }

        self.pace_after_submit(submit_started.elapsed());
        Ok(())
    }

    pub fn submit_frame_display_space(&mut self, rgba_buffer: &[u8]) -> Result<()> {
        let expected_len = expected_rgba_len(self.display_width, self.display_height)?;
        if rgba_buffer.len() != expected_len {
            return Err(DsapiError::ParseError(format!(
                "rgba_len_mismatch_display={}x{} expected={} actual={}",
                self.display_width,
                self.display_height,
                expected_len,
                rgba_buffer.len()
            )));
        }

        self.pace_before_submit();
        let submit_started = Instant::now();
        self.submit_display_frame(rgba_buffer, self.display_width, self.display_height)?;
        self.pace_after_submit(submit_started.elapsed());
        Ok(())
    }

    pub fn set_rotation(&mut self, rotation: u32) -> Result<()> {
        if rotation > 3 {
            return Err(DsapiError::ParseError(
                "rotation_must_be_0_3".to_string(),
            ));
        }
        self.rotation = rotation;
        let (logical_w, logical_h) =
            logical_size(self.display_width, self.display_height, self.rotation);
        self.logical_width = logical_w;
        self.logical_height = logical_h;
        Ok(())
    }

    pub fn reconfigure_display(
        &mut self,
        display_width: u32,
        display_height: u32,
        rotation: u32,
        refresh_hz: f32,
    ) -> Result<()> {
        if display_width == 0 || display_height == 0 || rotation > 3 {
            return Err(DsapiError::ParseError(
                "render_session_invalid_display_size".to_string(),
            ));
        }
        self.display_width = display_width;
        self.display_height = display_height;
        self.refresh_hz = sanitize_refresh_hz(refresh_hz);
        self.set_rotation(rotation)?;
        self.rotate_scratch.clear();
        self.next_deadline = None;
        Ok(())
    }

    pub fn set_fps_cap(&mut self, fps_cap: Option<f32>) -> Result<()> {
        if let Some(v) = fps_cap {
            if !v.is_finite() || v <= 0.0 {
                return Err(DsapiError::ParseError("fps_cap_invalid".to_string()));
            }
        }
        self.fps_cap = fps_cap;
        Ok(())
    }

    pub fn set_frame_pacing_enabled(&mut self, enabled: bool) {
        self.pacing_enabled = enabled;
        if !enabled {
            self.next_deadline = None;
        }
    }

    pub fn frame_size(&self) -> (u32, u32) {
        (self.logical_width, self.logical_height)
    }

    pub fn display_frame_size(&self) -> (u32, u32) {
        (self.display_width, self.display_height)
    }

    pub fn rotation(&self) -> u32 {
        self.rotation
    }

    pub fn refresh_hz(&self) -> f32 {
        self.refresh_hz
    }

    pub fn last_frame_seq(&self) -> u64 {
        self.last_frame_seq
    }

    fn submit_display_frame(&mut self, rgba_buffer: &[u8], width: u32, height: u32) -> Result<()> {
        let command = format!("RENDER_FRAME_SUBMIT_RGBA_RAW {} {} {}", width, height, rgba_buffer.len());
        let ready = self.connection.send_line(&command)?;
        if ready != "OK READY" {
            return Err(DsapiError::ProtocolError(format!(
                "RENDER_FRAME_SUBMIT_RGBA_RAW_bad_ready:{}",
                ready
            )));
        }

        self.connection.write_all(rgba_buffer)?;
        self.connection.flush()?;

        let response = self.connection.read_line()?;
        let tokens = parse_ok_tokens(&response)?;
        if tokens.len() != 6 {
            return Err(DsapiError::ProtocolError(format!(
                "RENDER_FRAME_SUBMIT_RGBA_RAW_bad_token_count:{} line={}",
                tokens.len(),
                response
            )));
        }
        if tokens[3] != "RGBA8888" {
            return Err(DsapiError::ProtocolError(format!(
                "RENDER_FRAME_SUBMIT_RGBA_RAW_bad_pixel_format:{}",
                response
            )));
        }

        let frame_seq = parse_u64(tokens[0], "frame_seq")?;
        let width = parse_u32(tokens[1], "width")?;
        let height = parse_u32(tokens[2], "height")?;
        let byte_len = parse_u32(tokens[4], "byte_len")?;
        let _checksum = parse_u32(tokens[5], "checksum_fnv1a32")?;

        if width != self.display_width || height != self.display_height {
            return Err(DsapiError::ProtocolError(format!(
                "submit_frame_response_size_mismatch line={}",
                response
            )));
        }
        if byte_len as usize != rgba_buffer.len() {
            return Err(DsapiError::ProtocolError(format!(
                "submit_frame_response_len_mismatch line={}",
                response
            )));
        }

        self.last_frame_seq = frame_seq;
        Ok(())
    }

    fn pace_before_submit(&mut self) {
        if !self.pacing_enabled {
            return;
        }

        let Some(deadline) = self.next_deadline else {
            return;
        };
        let now = Instant::now();
        if deadline > now {
            thread::sleep(deadline - now);
        }
    }

    fn pace_after_submit(&mut self, submit_cost: Duration) {
        if !self.pacing_enabled {
            return;
        }

        let cost_secs = submit_cost.as_secs_f64();
        if self.submit_ema_secs <= 0.0 {
            self.submit_ema_secs = cost_secs;
        } else {
            self.submit_ema_secs = self.submit_ema_secs * 0.8 + cost_secs * 0.2;
        }

        let base_interval = 1.0 / self.effective_fps();
        // 提交耗时越高，自动降帧，避免系统因忙提交而抖动。
        let adaptive_interval = base_interval.max(self.submit_ema_secs * 1.15);
        self.next_deadline = Some(Instant::now() + Duration::from_secs_f64(adaptive_interval));
    }

    fn effective_fps(&self) -> f64 {
        let mut fps = self.refresh_hz as f64;
        if let Some(cap) = self.fps_cap {
            fps = fps.min(cap as f64);
        }
        fps.clamp(1.0, 240.0)
    }
}

fn logical_size(display_width: u32, display_height: u32, rotation: u32) -> (u32, u32) {
    if rotation % 2 == 1 {
        (display_height, display_width)
    } else {
        (display_width, display_height)
    }
}

fn sanitize_refresh_hz(refresh_hz: f32) -> f32 {
    if refresh_hz.is_finite() && refresh_hz > 1.0 {
        refresh_hz
    } else {
        60.0
    }
}

fn rotate_logical_rgba_to_display(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
    rotation: u32,
    dst: &mut [u8],
) -> Result<()> {
    if rotation > 3 {
        return Err(DsapiError::ParseError("rotation_must_be_0_3".to_string()));
    }
    let src_len = expected_rgba_len(src_width, src_height)?;
    let dst_len = expected_rgba_len(dst_width, dst_height)?;
    if src.len() != src_len || dst.len() != dst_len {
        return Err(DsapiError::ParseError(
            "rotate_rgba_len_mismatch".to_string(),
        ));
    }

    let (expect_w, expect_h) = logical_size(dst_width, dst_height, rotation);
    if src_width != expect_w || src_height != expect_h {
        return Err(DsapiError::ParseError(format!(
            "rotate_rgba_size_mismatch expected={}x{} actual={}x{}",
            expect_w, expect_h, src_width, src_height
        )));
    }

    for y in 0..src_height {
        for x in 0..src_width {
            let (dx, dy) = match rotation {
                0 => (x, y),
                1 => (dst_width - 1 - y, x),
                2 => (dst_width - 1 - x, dst_height - 1 - y),
                3 => (y, dst_height - 1 - x),
                _ => unreachable!(),
            };

            let s_idx = ((y as usize) * (src_width as usize) + (x as usize)) * 4;
            let d_idx = ((dy as usize) * (dst_width as usize) + (dx as usize)) * 4;
            dst[d_idx..d_idx + 4].copy_from_slice(&src[s_idx..s_idx + 4]);
        }
    }
    Ok(())
}

fn expected_rgba_len(width: u32, height: u32) -> Result<usize> {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|v| v.checked_mul(4))
        .ok_or_else(|| DsapiError::ParseError("frame_size_overflow".to_string()))
}

fn parse_u32(token: &str, field: &str) -> Result<u32> {
    token
        .parse::<u32>()
        .map_err(|_| DsapiError::ParseError(format!("invalid_{}:{}", field, token)))
}

fn parse_u64(token: &str, field: &str) -> Result<u64> {
    token
        .parse::<u64>()
        .map_err(|_| DsapiError::ParseError(format!("invalid_{}:{}", field, token)))
}

#[cfg(test)]
mod tests {
    use super::{expected_rgba_len, logical_size, rotate_logical_rgba_to_display};

    #[test]
    fn expected_rgba_len_computes_bytes() {
        let len = expected_rgba_len(2, 3).expect("len");
        assert_eq!(len, 24);
    }

    #[test]
    fn expected_rgba_len_rejects_overflow() {
        let err = expected_rgba_len(u32::MAX, u32::MAX).expect_err("must overflow");
        assert_eq!(err.to_string(), "parse_error: frame_size_overflow");
    }

    #[test]
    fn logical_size_swaps_on_quarter_turn() {
        assert_eq!(logical_size(1080, 2400, 0), (1080, 2400));
        assert_eq!(logical_size(1080, 2400, 1), (2400, 1080));
        assert_eq!(logical_size(1080, 2400, 2), (1080, 2400));
        assert_eq!(logical_size(1080, 2400, 3), (2400, 1080));
    }

    #[test]
    fn rotate_logical_to_display_rotation_1() {
        // display = 2x3, rotation=1 => logical = 3x2
        let src_w = 3u32;
        let src_h = 2u32;
        let dst_w = 2u32;
        let dst_h = 3u32;

        let mut src = vec![0u8; (src_w * src_h * 4) as usize];
        for y in 0..src_h {
            for x in 0..src_w {
                let idx = ((y * src_w + x) * 4) as usize;
                src[idx] = (y * src_w + x) as u8;
                src[idx + 3] = 255;
            }
        }

        let mut dst = vec![0u8; (dst_w * dst_h * 4) as usize];
        rotate_logical_rgba_to_display(&src, src_w, src_h, dst_w, dst_h, 1, &mut dst)
            .expect("rotate");

        let mut got = Vec::new();
        for y in 0..dst_h {
            for x in 0..dst_w {
                let idx = ((y * dst_w + x) * 4) as usize;
                got.push(dst[idx]);
            }
        }
        assert_eq!(got, vec![3, 0, 4, 1, 5, 2]);
    }
}
