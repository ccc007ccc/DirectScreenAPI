use directscreen_core::api::{Status, RENDER_MAX_FRAME_BYTES};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RawFrameSubmitRequest {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) byte_len: usize,
}

fn parse_u32(token: &str) -> Result<u32, Status> {
    token.parse::<u32>().map_err(|_| Status::InvalidArgument)
}

fn parse_u64(token: &str) -> Result<u64, Status> {
    token.parse::<u64>().map_err(|_| Status::InvalidArgument)
}

pub(super) fn parse_raw_frame_submit_request(
    line: &str,
    max_frame_bytes: usize,
) -> Result<Option<RawFrameSubmitRequest>, Status> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let mut tokens = trimmed.split_whitespace();
    let Some(cmd) = tokens.next() else {
        return Ok(None);
    };

    if !cmd.eq_ignore_ascii_case("RENDER_FRAME_SUBMIT_RGBA_RAW") {
        return Ok(None);
    }

    let width = parse_u32(tokens.next().ok_or(Status::InvalidArgument)?)?;
    let height = parse_u32(tokens.next().ok_or(Status::InvalidArgument)?)?;
    let byte_len_u32 = parse_u32(tokens.next().ok_or(Status::InvalidArgument)?)?;
    if tokens.next().is_some() {
        return Err(Status::InvalidArgument);
    }
    if width == 0 || height == 0 {
        return Err(Status::InvalidArgument);
    }

    let byte_len = usize::try_from(byte_len_u32).map_err(|_| Status::OutOfRange)?;
    let expected_len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|v| v.checked_mul(4usize))
        .ok_or(Status::OutOfRange)?;
    if expected_len != byte_len {
        return Err(Status::InvalidArgument);
    }
    let max_len = max_frame_bytes.min(RENDER_MAX_FRAME_BYTES);
    if expected_len > max_len {
        return Err(Status::OutOfRange);
    }

    Ok(Some(RawFrameSubmitRequest {
        width,
        height,
        byte_len,
    }))
}

pub(super) fn parse_frame_get_raw_request(line: &str) -> Result<bool, Status> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }

    let mut tokens = trimmed.split_whitespace();
    let Some(cmd) = tokens.next() else {
        return Ok(false);
    };
    if !cmd.eq_ignore_ascii_case("RENDER_FRAME_GET_RAW") {
        return Ok(false);
    }
    if tokens.next().is_some() {
        return Err(Status::InvalidArgument);
    }
    Ok(true)
}

pub(super) fn parse_frame_get_fd_request(line: &str) -> Result<bool, Status> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }

    let mut tokens = trimmed.split_whitespace();
    let Some(cmd) = tokens.next() else {
        return Ok(false);
    };
    if !cmd.eq_ignore_ascii_case("RENDER_FRAME_GET_FD") {
        return Ok(false);
    }
    if tokens.next().is_some() {
        return Err(Status::InvalidArgument);
    }
    Ok(true)
}

pub(super) fn parse_frame_bind_fd_request(line: &str) -> Result<bool, Status> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }

    let mut tokens = trimmed.split_whitespace();
    let Some(cmd) = tokens.next() else {
        return Ok(false);
    };
    if !cmd.eq_ignore_ascii_case("RENDER_FRAME_BIND_FD") {
        return Ok(false);
    }
    if tokens.next().is_some() {
        return Err(Status::InvalidArgument);
    }
    Ok(true)
}

pub(super) fn parse_frame_get_bound_request(line: &str) -> Result<bool, Status> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }

    let mut tokens = trimmed.split_whitespace();
    let Some(cmd) = tokens.next() else {
        return Ok(false);
    };
    if !cmd.eq_ignore_ascii_case("RENDER_FRAME_GET_BOUND") {
        return Ok(false);
    }
    if tokens.next().is_some() {
        return Err(Status::InvalidArgument);
    }
    Ok(true)
}

pub(super) fn parse_frame_wait_bound_present_request(
    line: &str,
) -> Result<Option<(u64, u32)>, Status> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let mut tokens = trimmed.split_whitespace();
    let Some(cmd) = tokens.next() else {
        return Ok(None);
    };
    if !cmd.eq_ignore_ascii_case("RENDER_FRAME_WAIT_BOUND_PRESENT") {
        return Ok(None);
    }
    let last_seq = parse_u64(tokens.next().ok_or(Status::InvalidArgument)?)?;
    let timeout_ms = parse_u32(tokens.next().ok_or(Status::InvalidArgument)?)?;
    if tokens.next().is_some() {
        return Err(Status::InvalidArgument);
    }
    Ok(Some((last_seq, timeout_ms)))
}

pub(super) fn parse_display_stream_v1_request(line: &str) -> Result<bool, Status> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }

    let mut tokens = trimmed.split_whitespace();
    let Some(cmd) = tokens.next() else {
        return Ok(false);
    };
    if !cmd.eq_ignore_ascii_case("STREAM_DISPLAY_V1") {
        return Ok(false);
    }
    if tokens.next().is_some() {
        return Err(Status::InvalidArgument);
    }
    Ok(true)
}
