use std::io;
use std::os::fd::RawFd;

pub(super) fn set_fd_nonblocking(fd: RawFd) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }

    let ret = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
    if ret < 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(())
}

pub(super) fn make_wake_pipe() -> io::Result<(RawFd, RawFd)> {
    let mut fds = [0i32; 2];
    let ret = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) };
    if ret < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((fds[0], fds[1]))
}

pub(super) fn wait_ready(device_fd: RawFd, wake_read_fd: RawFd) -> io::Result<bool> {
    let mut fds = [
        libc::pollfd {
            fd: device_fd,
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: wake_read_fd,
            events: libc::POLLIN,
            revents: 0,
        },
    ];

    loop {
        let ret = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
        if ret < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        break;
    }

    if (fds[1].revents & libc::POLLIN) != 0 {
        drain_fd(wake_read_fd);
        return Ok(false);
    }
    if (fds[1].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL)) != 0 {
        return Ok(false);
    }
    if (fds[0].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL)) != 0 {
        return Err(io::Error::other("device_poll_error_or_hup"));
    }
    Ok((fds[0].revents & libc::POLLIN) != 0)
}

fn drain_fd(fd: RawFd) {
    let mut buf = [0u8; 64];
    loop {
        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n > 0 {
            if (n as usize) < buf.len() {
                break;
            }
            continue;
        }
        if n == 0 {
            break;
        }
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::WouldBlock || err.kind() == io::ErrorKind::Interrupted {
            break;
        }
        break;
    }
}

pub(super) fn close_fd(fd: &mut RawFd) {
    if *fd >= 0 {
        unsafe {
            libc::close(*fd);
        }
        *fd = -1;
    }
}

pub(super) fn close_fd_value(fd: RawFd) {
    if fd >= 0 {
        unsafe {
            libc::close(fd);
        }
    }
}
