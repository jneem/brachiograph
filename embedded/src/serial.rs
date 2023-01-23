use arrayvec::ArrayVec;
use brachiograph::{Op, Resp};
use postcard::accumulator::{CobsAccumulator, FeedResult};
use stm32f1xx_hal::usb::UsbBusType;
use usb_device::prelude::*;
use usbd_serial::SerialPort;

// TODO: the calibrationdata variant is pretty big, which forces this to be big also
const BUF_SIZE: usize = 128;

pub struct UsbSerial {
    dev: UsbDevice<'static, UsbBusType>,
    serial: SerialPort<'static, UsbBusType>,
    acc: CobsAccumulator<BUF_SIZE>,
    read_buf: ArrayVec<u8, BUF_SIZE>,
    write_buf: ArrayVec<u8, BUF_SIZE>,
}

impl UsbSerial {
    pub fn new(
        dev: UsbDevice<'static, UsbBusType>,
        serial: SerialPort<'static, UsbBusType>,
    ) -> Self {
        UsbSerial {
            dev,
            serial,
            acc: CobsAccumulator::new(),
            read_buf: ArrayVec::new(),
            write_buf: ArrayVec::new(),
        }
    }

    pub fn poll(&mut self) -> bool {
        self.dev.poll(&mut [&mut self.serial])
    }

    fn read_into_buf(&mut self) -> Result<(), UsbError> {
        let remaining = self.read_buf.remaining_capacity();
        if remaining > 0 {
            let len = self.read_buf.len();
            unsafe {
                self.read_buf.set_len(self.read_buf.capacity());
                match self.serial.read(&mut self.read_buf[len..]) {
                    Ok(count) => {
                        self.read_buf.set_len(len + count);
                    }
                    Err(e) => {
                        self.read_buf.set_len(len);
                        return Err(e);
                    }
                }
            }
        }
        Ok(())
    }

    fn drain_read_buf_except(&mut self, remaining: usize) {
        let until = self.read_buf.len().saturating_sub(remaining);
        self.read_buf.drain(..until);
    }

    /// Tries to read a message from the serial port, returning it if possible.
    ///
    /// This should be called often, probably on an interrupt. If it returns `Some`,
    /// maybe call it again to help process the queue faster.
    pub fn read(&mut self) -> Option<Op> {
        loop {
            match self.read_into_buf() {
                Ok(()) => {
                    let mut window = &self.read_buf[..];
                    while !window.is_empty() {
                        window = match self.acc.feed::<Op>(&window) {
                            FeedResult::Consumed => &[],
                            FeedResult::OverFull(w) => w,
                            FeedResult::DeserError(w) => w,
                            FeedResult::Success { data, remaining } => {
                                self.drain_read_buf_except(remaining.len());
                                return Some(data);
                            }
                        };
                    }
                }
                Err(e) => {
                    if !matches!(e, UsbError::WouldBlock) {
                        defmt::println!("error: {}", e);
                    }
                    return None;
                }
            }
        }
    }

    /// Tries to push our write buffer out onto the port. This should be called often,
    /// probably on an interrupt.
    pub fn write(&mut self) {
        let mut idx = 0;
        while idx < self.write_buf.len() {
            match self.serial.write(&self.write_buf[idx..]) {
                Ok(0) | Err(UsbError::WouldBlock) => break,
                Ok(count) => {
                    defmt::println!("wrote '{:?}'", self.write_buf[idx..(idx + count)]);
                    idx += count;
                }
                Err(e) => {
                    defmt::println!("error: {}", e);
                    self.write_buf.clear();
                    return;
                }
            }
        }
        let _ = self.serial.flush();
        self.write_buf.drain(..idx);
    }

    /// Tries to send or queue a message. Returns the message if the queue was full.
    pub fn send(&mut self, msg: Resp) -> Result<(), Resp> {
        self.write();
        let len = self.write_buf.len();
        let ret = unsafe {
            self.write_buf.set_len(self.write_buf.capacity());
            match postcard::to_slice_cobs(&msg, &mut self.write_buf[len..]) {
                Ok(written) => {
                    let new_len = len + written.len();
                    self.write_buf.set_len(new_len);
                    Ok(())
                }
                Err(_) => {
                    self.write_buf.set_len(len);
                    Err(msg)
                }
            }
        };
        self.write();
        ret
    }
}
