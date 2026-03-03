use directscreen_core::api::{Status, RENDER_MAX_FRAME_BYTES};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ShmFrameSubmitRequest {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) byte_len: usize,
    pub(super) offset: usize,
    pub(super) origin_x: i32,
    pub(super) origin_y: i32,
}

fn parse_u32(token: &str) -> Result<u32, Status> {
    token.parse::<u32>().map_err(|_| Status::InvalidArgument)
}

fn parse_u64(token: &str) -> Result<u64, Status> {
    token.parse::<u64>().map_err(|_| Status::InvalidArgument)
}

fn parse_i32(token: &str) -> Result<i32, Status> {
    token.parse::<i32>().map_err(|_| Status::InvalidArgument)
}

pub(super) fn parse_frame_bind_shm_request(line: &str) -> Result<bool, Status> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }

    let mut tokens = trimmed.split_whitespace();
    let Some(cmd) = tokens.next() else {
        return Ok(false);
    };
    if !cmd.eq_ignore_ascii_case("RENDER_FRAME_BIND_SHM") {
        return Ok(false);
    }
    if tokens.next().is_some() {
        return Err(Status::InvalidArgument);
    }
    Ok(true)
}

pub(super) fn parse_frame_wait_shm_present_request(
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
    if !cmd.eq_ignore_ascii_case("RENDER_FRAME_WAIT_SHM_PRESENT") {
        return Ok(None);
    }
    let last_seq = parse_u64(tokens.next().ok_or(Status::InvalidArgument)?)?;
    let timeout_ms = parse_u32(tokens.next().ok_or(Status::InvalidArgument)?)?;
    if tokens.next().is_some() {
        return Err(Status::InvalidArgument);
    }
    Ok(Some((last_seq, timeout_ms)))
}

pub(super) fn parse_frame_submit_shm_request(
    line: &str,
) -> Result<Option<ShmFrameSubmitRequest>, Status> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let mut tokens = trimmed.split_whitespace();
    let Some(cmd) = tokens.next() else {
        return Ok(None);
    };
    if !cmd.eq_ignore_ascii_case("RENDER_FRAME_SUBMIT_SHM") {
        return Ok(None);
    }

    let width = parse_u32(tokens.next().ok_or(Status::InvalidArgument)?)?;
    let height = parse_u32(tokens.next().ok_or(Status::InvalidArgument)?)?;
    let byte_len_u32 = parse_u32(tokens.next().ok_or(Status::InvalidArgument)?)?;
    let offset_u32 = parse_u32(tokens.next().ok_or(Status::InvalidArgument)?)?;
    let origin_x = match tokens.next() {
        Some(v) => parse_i32(v)?,
        None => 0,
    };
    let origin_y = match tokens.next() {
        Some(v) => parse_i32(v)?,
        None => 0,
    };
    if tokens.next().is_some() {
        return Err(Status::InvalidArgument);
    }

    if width == 0 || height == 0 {
        return Err(Status::InvalidArgument);
    }

    let byte_len = usize::try_from(byte_len_u32).map_err(|_| Status::OutOfRange)?;
    let offset = usize::try_from(offset_u32).map_err(|_| Status::OutOfRange)?;
    let expected_len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|v| v.checked_mul(4usize))
        .ok_or(Status::OutOfRange)?;
    if byte_len != expected_len {
        return Err(Status::InvalidArgument);
    }
    if byte_len > RENDER_MAX_FRAME_BYTES {
        return Err(Status::OutOfRange);
    }

    Ok(Some(ShmFrameSubmitRequest {
        width,
        height,
        byte_len,
        offset,
        origin_x,
        origin_y,
    }))
}
