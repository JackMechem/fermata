//! Audio-thread COM objects: parameter change queues and event lists. These are preallocated and
//! never allocate while processing.

use std::cell::{Cell, UnsafeCell};

use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComWrapper};

const MAX_POINTS: usize = 64;
const MAX_QUEUES: usize = 256;
const MAX_EVENTS: usize = 1024;

pub struct ParamValueQueue {
    id: Cell<ParamID>,
    points: UnsafeCell<Vec<(i32, f64)>>,
}

unsafe impl Send for ParamValueQueue {}
unsafe impl Sync for ParamValueQueue {}

impl Class for ParamValueQueue {
    type Interfaces = (IParamValueQueue,);
}

impl ParamValueQueue {
    /// # Safety
    /// The queue is used from one thread at a time (the audio thread, during `process`), and
    /// the returned reference is dropped before `points` is called again.
    #[allow(clippy::mut_from_ref)]
    unsafe fn points(&self) -> &mut Vec<(i32, f64)> {
        unsafe { &mut *self.points.get() }
    }
}

impl IParamValueQueueTrait for ParamValueQueue {
    unsafe fn getParameterId(&self) -> ParamID {
        self.id.get()
    }

    unsafe fn getPointCount(&self) -> int32 {
        self.points().len() as int32
    }

    unsafe fn getPoint(&self, index: int32, offset: *mut int32, value: *mut ParamValue) -> tresult {
        match self.points().get(index as usize) {
            Some(&(o, v)) => {
                *offset = o;
                *value = v;
                kResultOk
            }
            None => kInvalidArgument,
        }
    }

    unsafe fn addPoint(&self, offset: int32, value: ParamValue, index: *mut int32) -> tresult {
        let pts = self.points();
        if pts.len() >= pts.capacity() {
            // Keep the newest value rather than allocating on the audio thread.
            if let Some(last) = pts.last_mut() {
                *last = (offset, value);
            }
        } else {
            pts.push((offset, value));
        }
        if !index.is_null() {
            *index = pts.len() as int32 - 1;
        }
        kResultOk
    }
}

pub struct ParameterChanges {
    queues: Vec<ComWrapper<ParamValueQueue>>,
    ptrs: Vec<*mut IParamValueQueue>,
    count: Cell<usize>,
}

unsafe impl Send for ParameterChanges {}
unsafe impl Sync for ParameterChanges {}

impl Class for ParameterChanges {
    type Interfaces = (IParameterChanges,);
}

impl ParameterChanges {
    pub fn new() -> ComWrapper<ParameterChanges> {
        let queues: Vec<_> = (0..MAX_QUEUES)
            .map(|_| {
                ComWrapper::new(ParamValueQueue {
                    id: Cell::new(0),
                    points: UnsafeCell::new(Vec::with_capacity(MAX_POINTS)),
                })
            })
            .collect();
        let ptrs = queues.iter().map(|q| q.as_com_ref::<IParamValueQueue>().unwrap().as_ptr()).collect();
        ComWrapper::new(ParameterChanges { queues, ptrs, count: Cell::new(0) })
    }

    pub fn clear(&self) {
        for q in &self.queues[..self.count.get()] {
            unsafe { q.points().clear() };
        }
        self.count.set(0);
    }

    fn queue_for(&self, id: ParamID) -> Option<usize> {
        let n = self.count.get();
        if let Some(i) = self.queues[..n].iter().position(|q| q.id.get() == id) {
            return Some(i);
        }
        if n >= self.queues.len() {
            return None;
        }
        self.queues[n].id.set(id);
        unsafe { self.queues[n].points().clear() };
        self.count.set(n + 1);
        Some(n)
    }

    /// Host-side helper for queueing a change.
    pub fn push(&self, id: ParamID, offset: i32, value: f64) {
        if let Some(i) = self.queue_for(id) {
            unsafe {
                self.queues[i].addPoint(offset, value, std::ptr::null_mut());
            }
        }
    }

    /// Iterates the last value written to each queue (used for processor -> controller output).
    pub fn for_each_last(&self, mut f: impl FnMut(ParamID, f64)) {
        for q in &self.queues[..self.count.get()] {
            if let Some(&(_, v)) = unsafe { q.points() }.last() {
                f(q.id.get(), v);
            }
        }
    }
}

impl IParameterChangesTrait for ParameterChanges {
    unsafe fn getParameterCount(&self) -> int32 {
        self.count.get() as int32
    }

    unsafe fn getParameterData(&self, index: int32) -> *mut IParamValueQueue {
        if (index as usize) < self.count.get() {
            self.ptrs[index as usize]
        } else {
            std::ptr::null_mut()
        }
    }

    unsafe fn addParameterData(&self, id: *const ParamID, index: *mut int32) -> *mut IParamValueQueue {
        if id.is_null() {
            return std::ptr::null_mut();
        }
        match self.queue_for(*id) {
            Some(i) => {
                if !index.is_null() {
                    *index = i as int32;
                }
                self.ptrs[i]
            }
            None => std::ptr::null_mut(),
        }
    }
}

pub struct EventList {
    events: UnsafeCell<Vec<Event>>,
}

unsafe impl Send for EventList {}
unsafe impl Sync for EventList {}

impl Class for EventList {
    type Interfaces = (IEventList,);
}

impl EventList {
    pub fn new() -> ComWrapper<EventList> {
        ComWrapper::new(EventList { events: UnsafeCell::new(Vec::with_capacity(MAX_EVENTS)) })
    }

    /// # Safety
    /// As for `ParamValueQueue::points`: one thread at a time, one reference at a time.
    #[allow(clippy::mut_from_ref)]
    unsafe fn events(&self) -> &mut Vec<Event> {
        unsafe { &mut *self.events.get() }
    }

    pub fn clear(&self) {
        unsafe { self.events().clear() };
    }

    pub fn push(&self, e: Event) {
        let ev = unsafe { self.events() };
        if ev.len() < ev.capacity() {
            ev.push(e);
        }
    }

    pub fn len(&self) -> usize {
        unsafe { self.events().len() }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl IEventListTrait for EventList {
    unsafe fn getEventCount(&self) -> int32 {
        self.events().len() as int32
    }

    unsafe fn getEvent(&self, index: int32, e: *mut Event) -> tresult {
        match self.events().get(index as usize) {
            Some(ev) => {
                *e = *ev;
                kResultOk
            }
            None => kInvalidArgument,
        }
    }

    unsafe fn addEvent(&self, e: *mut Event) -> tresult {
        if e.is_null() {
            return kInvalidArgument;
        }
        self.push(*e);
        kResultOk
    }
}
