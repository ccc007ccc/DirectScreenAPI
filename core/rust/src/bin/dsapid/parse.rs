use directscreen_core::api::Status;

fn parse_u32(token: &str) -> Result<u32, Status> {
    token.parse::<u32>().map_err(|_| Status::InvalidArgument)
}

fn parse_u64(token: &str) -> Result<u64, Status> {
    token.parse::<u64>().map_err(|_| Status::InvalidArgument)
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
