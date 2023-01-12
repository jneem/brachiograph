use arrayvec::ArrayVec;
use brachiograph::OpParseErr;
use stm32f1xx_hal::usb::UsbBusType;
use usb_device::prelude::*;
use usbd_serial::SerialPort;

const READ_SIZE: usize = 32;
const WRITE_SIZE: usize = 32;
const BUF_SIZE: usize = 512;

/// A `WriteMsg` is something of bounded length that can be written to a buffer.
pub trait WriteMsg {
    /// Must write at most `WRITE_SIZE` bytes to the buffer.
    fn write(&self, buf: &mut [u8]) -> usize;
    fn error() -> Self;
}

pub trait ParseMsg: Sized {
    type Error: defmt::Format;

    fn parse(buf: &[u8]) -> Result<Self, Self::Error>;
}

impl ParseMsg for brachiograph::Op {
    type Error = brachiograph::OpParseErr;

    fn parse(buf: &[u8]) -> Result<Self, Self::Error> {
        let s = core::str::from_utf8(buf).map_err(|_| OpParseErr::UnknownOp)?;
        s.parse()
    }
}

pub enum Status {
    QueueFull,
    Ack,
    Nack,
}

impl WriteMsg for Status {
    fn write(&self, buf: &mut [u8]) -> usize {
        let s = match self {
            Status::QueueFull => &b"queue full"[..],
            Status::Ack => &b"ack"[..],
            Status::Nack => &b"nack"[..],
        };
        buf[..s.len()].copy_from_slice(s);
        s.len()
    }

    fn error() -> Self {
        // TODO: more detailed error
        Self::Nack
    }
}

pub struct UsbSerial<Rx, Tx> {
    dev: UsbDevice<'static, UsbBusType>,
    serial: SerialPort<'static, UsbBusType>,
    read_buf: ArrayVec<u8, BUF_SIZE>,
    write_buf: ArrayVec<u8, BUF_SIZE>,
    _rx: core::marker::PhantomData<Rx>,
    _tx: core::marker::PhantomData<Tx>,
}

impl<Rx, Tx> UsbSerial<Rx, Tx>
where
    Rx: ParseMsg,
    Tx: WriteMsg,
{
    pub fn new(
        dev: UsbDevice<'static, UsbBusType>,
        serial: SerialPort<'static, UsbBusType>,
    ) -> Self {
        UsbSerial {
            dev,
            serial,
            read_buf: ArrayVec::new(),
            write_buf: ArrayVec::new(),
            _rx: core::marker::PhantomData,
            _tx: core::marker::PhantomData,
        }
    }

    pub fn poll(&mut self) -> bool {
        self.dev.poll(&mut [&mut self.serial])
    }

    /// Tries to read a message from the serial port, returning it if possible.
    ///
    /// This should be called often, probably on an interrupt. If it returns `Some`,
    /// maybe call it again to help process the queue faster.
    pub fn read(&mut self) -> Option<Rx> {
        let mut buf = [0u8; READ_SIZE];

        // TODO: it would be nice to read directly into read_buf, but uninitialized memory etc etc
        if self.read_buf.len() + READ_SIZE > BUF_SIZE {
            defmt::println!("ran out of buffer, clearing it");
            self.read_buf.clear();
        }

        loop {
            match self.serial.read(&mut buf) {
                Ok(count) => {
                    defmt::println!("read '{:?}'", buf[0..count]);
                    // unwrap is ok because we checked capacity
                    self.read_buf.try_extend_from_slice(&buf[..count]).unwrap();
                    if count < READ_SIZE || self.read_buf.len() > BUF_SIZE - READ_SIZE {
                        break;
                    }
                }
                Err(UsbError::WouldBlock) => {
                    break;
                }
                Err(e) => {
                    defmt::println!("error: {}", e);
                    return None;
                }
            }
        }

        if let Some(idx) = self.read_buf.iter().position(|&c| c == b'\n') {
            let res = Rx::parse(&self.read_buf[..idx]);
            if res.is_err() {
                defmt::println!(
                    "buffer {}, {:?}",
                    self.read_buf[..idx],
                    core::str::from_utf8(&self.read_buf[..idx]).ok()
                );
            }
            self.read_buf.drain(..=idx);
            match res {
                Ok(msg) => return Some(msg),
                Err(e) => {
                    let _ = self.send(Tx::error());
                    defmt::println!("error: {}", e);
                }
            }
        }
        None
    }

    /// Tries to push our write buffer out onto the port. This should be called often,
    /// probably on an interrupt.
    pub fn write(&mut self) {
        // FIXME: if the tty has echo on, we'll get back our writes as reads. For some reason,
        // every time the usb serial gets reopened, it defaults to echo. So for now, don't
        // actually write anything.
        //self.write_buf.clear();
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
    pub fn send(&mut self, msg: Tx) -> Result<(), Tx> {
        self.write();
        if self.write_buf.len() + WRITE_SIZE + 2 > self.write_buf.capacity() {
            Err(msg)
        } else {
            let mut buf = [0u8; WRITE_SIZE];
            let count = msg.write(&mut buf);
            self.write_buf.try_extend_from_slice(&buf[..count]).unwrap();
            self.write_buf.push(b'\r');
            self.write_buf.push(b'\n');
            self.write();
            Ok(())
        }
    }
}
