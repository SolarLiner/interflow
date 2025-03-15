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
    pub fn is_triggered(&self) -> Result<bool, nix::Error> {
        let mut value = 0u64;
        let size = size_of_val(&value);
        let out = std::ptr::from_mut(&mut value).cast();
        let ret = unsafe { libc::read(self.0, out, size) };
        match (ret, value) {
            (8, 1) => Ok(true),
            (0, _) => Ok(false),
            _ => Err(nix::Error::last()),
        }
    }

    pub fn as_pollfd(&self) -> libc::pollfd {
        libc::pollfd {
            fd: self.0,
            events: libc::POLLIN,
            revents: 0,
        }
    }

    pub fn alsa_poll(&self, timeout: i32) -> Result<bool, alsa::Error> {
        let mut fds = [self.as_pollfd()];
        let res = alsa::poll::poll(&mut fds, timeout)?;
        Ok(res > 0)
    }
}
