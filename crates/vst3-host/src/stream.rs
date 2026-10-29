//! An in-memory IBStream used for plugin state save/restore.

use std::ffi::c_void;

use parking_lot::Mutex;
use vst3::Steinberg::IBStream_::IStreamSeekMode_::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComWrapper};

pub struct MemoryStream {
    inner: Mutex<(Vec<u8>, usize)>,
}

impl Class for MemoryStream {
    type Interfaces = (IBStream,);
}

impl MemoryStream {
    pub fn new(data: Vec<u8>) -> ComWrapper<MemoryStream> {
        ComWrapper::new(MemoryStream { inner: Mutex::new((data, 0)) })
    }

    pub fn data(&self) -> Vec<u8> {
        self.inner.lock().0.clone()
    }

    pub fn rewind(&self) {
        self.inner.lock().1 = 0;
    }
}

pub fn as_stream(s: &ComWrapper<MemoryStream>) -> ComPtr<IBStream> {
    s.to_com_ptr::<IBStream>().unwrap()
}

impl IBStreamTrait for MemoryStream {
    unsafe fn read(&self, buffer: *mut c_void, num_bytes: int32, num_read: *mut int32) -> tresult {
        let mut g = self.inner.lock();
        let (data, pos) = &mut *g;
        let n = (num_bytes.max(0) as usize).min(data.len().saturating_sub(*pos));
        if n > 0 {
            std::ptr::copy_nonoverlapping(data.as_ptr().add(*pos), buffer as *mut u8, n);
        }
        *pos += n;
        if !num_read.is_null() {
            *num_read = n as int32;
        }
        kResultOk
    }

    unsafe fn write(&self, buffer: *mut c_void, num_bytes: int32, num_written: *mut int32) -> tresult {
        let mut g = self.inner.lock();
        let (data, pos) = &mut *g;
        let n = num_bytes.max(0) as usize;
        if data.len() < *pos + n {
            data.resize(*pos + n, 0);
        }
        std::ptr::copy_nonoverlapping(buffer as *const u8, data.as_mut_ptr().add(*pos), n);
        *pos += n;
        if !num_written.is_null() {
            *num_written = n as int32;
        }
        kResultOk
    }

    unsafe fn seek(&self, offset: int64, mode: int32, result: *mut int64) -> tresult {
        let mut g = self.inner.lock();
        let (data, pos) = &mut *g;
        let base = match mode as u32 {
            m if m == kIBSeekSet => 0i64,
            m if m == kIBSeekCur => *pos as i64,
            m if m == kIBSeekEnd => data.len() as i64,
            _ => return kInvalidArgument,
        };
        let new = (base + offset).max(0) as usize;
        *pos = new;
        if !result.is_null() {
            *result = new as int64;
        }
        kResultOk
    }

    unsafe fn tell(&self, pos: *mut int64) -> tresult {
        if !pos.is_null() {
            *pos = self.inner.lock().1 as int64;
        }
        kResultOk
    }
}
