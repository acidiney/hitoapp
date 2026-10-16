use crate::{AudioFormat, Error, Result};
use soundtouch_ffi as _;
use std::{ffi::c_void, ptr::NonNull};

unsafe extern "C" {
    fn hito_tempo_create(rate: u32, channels: u32, speed: f64) -> *mut c_void;
    fn hito_tempo_destroy(handle: *mut c_void);
    fn hito_tempo_put(handle: *mut c_void, samples: *const f32, frames: u32) -> bool;
    fn hito_tempo_receive(handle: *mut c_void, samples: *mut f32, frames: u32) -> i64;
    fn hito_tempo_flush(handle: *mut c_void) -> bool;
}

pub(crate) struct Processor {
    handle: NonNull<c_void>,
    channels: usize,
}

impl Processor {
    pub fn new(format: AudioFormat, speed: f32) -> Result<Self> {
        // C++ owns construction, layout and destruction. The worker exclusively owns this handle.
        let handle = unsafe {
            hito_tempo_create(format.sample_rate, u32::from(format.channels), f64::from(speed))
        };
        Ok(Self {
            handle: NonNull::new(handle).ok_or(Error::Output)?,
            channels: usize::from(format.channels),
        })
    }

    pub fn put(&mut self, samples: &[f32]) -> Result<()> {
        let frames = u32::try_from(samples.len() / self.channels).map_err(|_| Error::Output)?;
        if !samples.len().is_multiple_of(self.channels) {
            return Err(Error::Output);
        }
        if unsafe { hito_tempo_put(self.handle.as_ptr(), samples.as_ptr(), frames) } {
            Ok(())
        } else {
            Err(Error::Output)
        }
    }

    pub fn receive(&mut self, samples: &mut [f32]) -> Result<usize> {
        let capacity = u32::try_from(samples.len() / self.channels).map_err(|_| Error::Output)?;
        let frames = unsafe { hito_tempo_receive(self.handle.as_ptr(), samples.as_mut_ptr(), capacity) };
        usize::try_from(frames)
            .ok()
            .filter(|frames| *frames <= capacity as usize)
            .ok_or(Error::Output)
    }

    pub fn flush(&mut self) -> Result<()> {
        if unsafe { hito_tempo_flush(self.handle.as_ptr()) } {
            Ok(())
        } else {
            Err(Error::Output)
        }
    }
}

impl Drop for Processor {
    fn drop(&mut self) {
        unsafe { hito_tempo_destroy(self.handle.as_ptr()) };
    }
}

#[cfg(feature = "native-probe")]
#[doc(hidden)]
pub fn native_processor_counts() -> (u64, u64) {
    unsafe extern "C" {
        fn hito_tempo_created() -> u64;
        fn hito_tempo_destroyed() -> u64;
    }
    unsafe { (hito_tempo_created(), hito_tempo_destroyed()) }
}
