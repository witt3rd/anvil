//! Pass a tty file descriptor over the unix socket (SCM_RIGHTS).
//! The client donates stdout; the daemon paints it.

use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;

pub fn send_fd(stream: &UnixStream, fd: RawFd) -> io::Result<()> {
    let mut dummy: u8 = 0;
    unsafe { send_fd_msg(stream.as_raw_fd(), fd, &mut dummy) }
}

pub fn recv_fd(stream: &UnixStream) -> io::Result<RawFd> {
    let mut dummy: u8 = 0;
    unsafe { recv_fd_msg(stream.as_raw_fd(), &mut dummy) }
}

unsafe fn send_fd_msg(sock: RawFd, fd: RawFd, dummy: &mut u8) -> io::Result<()> {
    let mut iov = libc::iovec {
        iov_base: dummy as *mut u8 as *mut libc::c_void,
        iov_len: 1,
    };
    let mut buf = [0u8; 256];
    let msg = libc::msghdr {
        msg_name: std::ptr::null_mut(),
        msg_namelen: 0,
        msg_iov: &mut iov,
        msg_iovlen: 1,
        msg_control: buf.as_mut_ptr() as *mut libc::c_void,
        msg_controllen: libc::CMSG_SPACE(std::mem::size_of::<RawFd>() as u32) as _,
        msg_flags: 0,
    };
    let cmsg = libc::CMSG_FIRSTHDR(&msg);
    if cmsg.is_null() {
        return Err(io::Error::other("no cmsg space"));
    }
    (*cmsg).cmsg_level = libc::SOL_SOCKET;
    (*cmsg).cmsg_type = libc::SCM_RIGHTS;
    (*cmsg).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<RawFd>() as u32) as _;
    std::ptr::copy_nonoverlapping(&fd as *const RawFd, libc::CMSG_DATA(cmsg) as *mut RawFd, 1);
    if libc::sendmsg(sock, &msg, 0) < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

unsafe fn recv_fd_msg(sock: RawFd, dummy: &mut u8) -> io::Result<RawFd> {
    let mut iov = libc::iovec {
        iov_base: dummy as *mut u8 as *mut libc::c_void,
        iov_len: 1,
    };
    let mut buf = [0u8; 256];
    let mut msg = libc::msghdr {
        msg_name: std::ptr::null_mut(),
        msg_namelen: 0,
        msg_iov: &mut iov,
        msg_iovlen: 1,
        msg_control: buf.as_mut_ptr() as *mut libc::c_void,
        msg_controllen: libc::CMSG_SPACE(std::mem::size_of::<RawFd>() as u32) as _,
        msg_flags: 0,
    };
    if libc::recvmsg(sock, &mut msg, 0) < 0 {
        return Err(io::Error::last_os_error());
    }
    let cmsg = libc::CMSG_FIRSTHDR(&msg);
    if cmsg.is_null()
        || (*cmsg).cmsg_level != libc::SOL_SOCKET
        || (*cmsg).cmsg_type != libc::SCM_RIGHTS
    {
        return Err(io::Error::other("the client sent no tty"));
    }
    let mut fd: RawFd = -1;
    std::ptr::copy_nonoverlapping(libc::CMSG_DATA(cmsg) as *const RawFd, &mut fd, 1);
    if fd < 0 {
        return Err(io::Error::other("the client sent no tty"));
    }
    Ok(fd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::fd::{FromRawFd, IntoRawFd};
    use std::os::unix::net::UnixStream;

    #[test]
    fn a_fd_crosses_the_socket() {
        let (a, b) = UnixStream::pair().unwrap();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(b"hello fd").unwrap();
        tmp.flush().unwrap();
        let path = tmp.path().to_path_buf();
        let file = std::fs::File::open(&path).unwrap();
        let raw = file.into_raw_fd();
        send_fd(&a, raw).unwrap();
        unsafe {
            libc::close(raw);
        }
        let got = recv_fd(&b).unwrap();
        let mut got = unsafe { std::fs::File::from_raw_fd(got) };
        let mut buf = String::new();
        got.read_to_string(&mut buf).unwrap();
        assert_eq!(buf, "hello fd");
    }
}
