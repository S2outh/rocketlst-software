
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug)]
pub struct Empty;

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug)]
pub enum PushErr<SerialErr> {
    Overflow,
    Serial(SerialErr)
}

/// This is a ringbuffer that is designed to enqueue values using the
/// embedded-async-io read() trait (or any other function with a similar interface)
pub struct SerialRingbuffer<T, const N: usize, const SN: usize> {
    storage: [T; N],
    head: usize,
    tail: usize,
    len: usize,
}
impl<T: Copy, const N: usize, const SN: usize> SerialRingbuffer<T, N, SN> {
    pub const fn new(val: T) -> Self 
    {
        Self { storage: [val; N], head: 0, tail: 0, len: 0 }
    }
    pub fn pop(&mut self) -> Result<T, Empty> {
        // if tail higher than len wraparound and reset len
        if self.tail >= self.len {
            self.len = self.head;
            self.tail = 0;
        }
        if self.head == self.tail {
            return Err(Empty);
        }
        let v = self.storage[self.tail];
        self.tail += 1;
        Ok(v)
    }
    pub async fn push_from_read<F, E>(&mut self, read: F) -> Result<(), PushErr<E>>
        where F: AsyncFnOnce(&mut [T]) -> Result<usize, E> {
        if N - SN < self.head {
            if self.tail < SN {
                return Err(PushErr::Overflow);
            }
            self.head = read(&mut self.storage[..SN]).await
                .map_err(|e| PushErr::Serial(e))?;
        } else {
            if self.tail < self.head + SN {
                return Err(PushErr::Overflow)
            }
            self.head += read(&mut self.storage[self.head..(self.head+SN)]).await
                .map_err(|e| PushErr::Serial(e))?;
        }
        self.len = usize::max(self.len, self.head);
        Ok(())
    }
}
