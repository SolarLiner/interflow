pub fn trigger() -> Result<(Sender, Receiver), nix::Error> {
    let mut fds = [0; 2];
    let ret = unsafe { libc::pipe(fds.as_mut_ptr()) };
    nix::Error::result(ret)?;
    let [read, write] = fds;
    Ok((Sender(read), Receiver(write)))
}

#[derive(Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Sender(libc::c_int);

unsafe impl Send for Sender {}
unsafe impl Sync for Sender {}

impl Drop for Sender {
    fn drop(&mut self) {
        unsafe { libc::close(self.0) };
    }
}

impl Sender {
    pub fn trigger(&self) -> Result<(), nix::Error> {
        let buf = 1u64;
        let size = size_of_val(&buf);
        let buf = std::ptr::from_ref(&buf).cast();
        let ret = unsafe { libc::write(self.0, buf, size) };
        match ret {
            8 => Ok(()),
            _ => Err(nix::Error::last()),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Receiver(libc::c_int);

unsafe impl Send for Receiver {}
unsafe impl Sync for Receiver {}

impl Drop for Receiver {
    fn drop(&mut self) {
        unsafe { libc::close(self.0) };
    }
}

impl Receiver {
    pub fn as_pollfd(&self) -> libc::pollfd {
        libc::pollfd {
            fd: self.0,
            events: libc::POLLIN,
            revents: 0,
        }
    }
}
