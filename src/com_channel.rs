use qt_core::{QBox, SignalOfBool};
use std::sync::mpsc as mpsc_std;

pub struct ComChannel<T> {
    ch: mpsc_std::SyncSender<T>,
    signal: force_send_sync::Send<QBox<SignalOfBool>>,
}

impl<T> ComChannel<T> {
    pub unsafe fn new(ch: mpsc_std::SyncSender<T>) -> Self {
        Self {
            ch,
            signal: force_send_sync::Send::new(SignalOfBool::new()),
        }
    }
    pub unsafe fn send(&self, t: T) -> Result<(), mpsc_std::SendError<T>> {
        self.ch.send(t)?;
        self.signal.emit(true);
        Ok(())
    }
    pub unsafe fn signal(&self) -> &QBox<SignalOfBool> {
        &self.signal
    }
}
