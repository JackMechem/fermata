//! Host-side COM objects handed to plugins: IHostApplication, IComponentHandler, IMessage,
//! IAttributeList.

use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::OnceLock;

use parking_lot::Mutex;
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComWrapper, Interface};

use crate::util::{copy_utf16, utf16_ptr};

pub const HOST_NAME: &str = "Fermata";

pub struct HostApp;

impl Class for HostApp {
    type Interfaces = (IHostApplication, IPlugInterfaceSupport);
}

unsafe fn guid_of(ptr: *const TUID) -> [u8; 16] {
    std::ptr::read_unaligned(ptr as *const [u8; 16])
}

impl IHostApplicationTrait for HostApp {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        if name.is_null() {
            return kInvalidArgument;
        }
        copy_utf16(HOST_NAME, &mut *name);
        kResultOk
    }

    unsafe fn createInstance(&self, cid: *mut TUID, iid: *mut TUID, obj: *mut *mut c_void) -> tresult {
        if obj.is_null() {
            return kInvalidArgument;
        }
        *obj = std::ptr::null_mut();
        let cid = guid_of(cid);
        let iid = if iid.is_null() { cid } else { guid_of(iid) };
        if cid == IMessage::IID || iid == IMessage::IID {
            let msg = ComWrapper::new(HostMessage::default());
            if let Some(p) = msg.to_com_ptr::<IMessage>() {
                *obj = p.into_raw() as *mut c_void;
                return kResultOk;
            }
        }
        if cid == IAttributeList::IID || iid == IAttributeList::IID {
            let list = ComWrapper::new(HostAttributeList::default());
            if let Some(p) = list.to_com_ptr::<IAttributeList>() {
                *obj = p.into_raw() as *mut c_void;
                return kResultOk;
            }
        }
        kNoInterface
    }
}

impl IPlugInterfaceSupportTrait for HostApp {
    unsafe fn isPlugInterfaceSupported(&self, iid: *const TUID) -> tresult {
        let iid = guid_of(iid);
        let supported = [
            IComponent::IID,
            IAudioProcessor::IID,
            IEditController::IID,
            IConnectionPoint::IID,
            IMidiMapping::IID,
            IPlugView::IID,
        ];
        if supported.contains(&iid) {
            kResultTrue
        } else {
            kResultFalse
        }
    }
}

struct HostCtx(ComWrapper<HostApp>);
unsafe impl Send for HostCtx {}
unsafe impl Sync for HostCtx {}
static HOST: OnceLock<HostCtx> = OnceLock::new();

/// The single host-application object shared by every plugin.
pub fn host_context() -> ComPtr<FUnknown> {
    HOST.get_or_init(|| HostCtx(ComWrapper::new(HostApp)))
        .0
        .to_com_ptr::<IHostApplication>()
        .unwrap()
        .upcast()
}

// ---------------------------------------------------------------------------------------------

/// One step of a parameter gesture in the plug-in's own editor, as the host sees it through
/// `beginEdit` / `performEdit` / `endEdit`. The session uses these to write automation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gesture {
    Begin(u32),
    Perform(u32, f64),
    End(u32),
}

/// Receives parameter edits made in the plugin's own editor (UI thread) and queues them for the
/// audio thread, which forwards them to the processor.
pub struct ComponentHandler {
    pub(crate) edits: Mutex<rtrb::Producer<(u32, f64)>>,
    pub(crate) restart_flags: AtomicI32,
    pub(crate) touched: AtomicI32,
    /// The same edits with their begin/end, for the controller's owner.
    pub(crate) gestures: Mutex<rtrb::Producer<Gesture>>,
}

impl Class for ComponentHandler {
    type Interfaces = (IComponentHandler,);
}

impl IComponentHandlerTrait for ComponentHandler {
    unsafe fn beginEdit(&self, id: ParamID) -> tresult {
        let _ = self.gestures.lock().push(Gesture::Begin(id));
        kResultOk
    }

    unsafe fn performEdit(&self, id: ParamID, value: ParamValue) -> tresult {
        let _ = self.edits.lock().push((id, value));
        let _ = self.gestures.lock().push(Gesture::Perform(id, value));
        self.touched.fetch_add(1, Ordering::Relaxed);
        kResultOk
    }

    unsafe fn endEdit(&self, id: ParamID) -> tresult {
        let _ = self.gestures.lock().push(Gesture::End(id));
        kResultOk
    }

    unsafe fn restartComponent(&self, flags: int32) -> tresult {
        self.restart_flags.fetch_or(flags, Ordering::Relaxed);
        kResultOk
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Clone)]
enum Attr {
    Int(i64),
    Float(f64),
    Str(Vec<u16>),
    Bin(Vec<u8>),
}

#[derive(Default)]
pub struct HostAttributeList {
    values: Mutex<HashMap<String, Attr>>,
}

impl Class for HostAttributeList {
    type Interfaces = (IAttributeList,);
}

unsafe fn key(id: *const c_char) -> Option<String> {
    if id.is_null() {
        return None;
    }
    Some(CStr::from_ptr(id).to_string_lossy().into_owned())
}

impl IAttributeListTrait for HostAttributeList {
    unsafe fn setInt(&self, id: IAttributeList_::AttrID, value: int64) -> tresult {
        let Some(k) = key(id) else { return kInvalidArgument };
        self.values.lock().insert(k, Attr::Int(value));
        kResultOk
    }

    unsafe fn getInt(&self, id: IAttributeList_::AttrID, value: *mut int64) -> tresult {
        let Some(k) = key(id) else { return kInvalidArgument };
        match self.values.lock().get(&k) {
            Some(Attr::Int(v)) => {
                *value = *v;
                kResultOk
            }
            _ => kResultFalse,
        }
    }

    unsafe fn setFloat(&self, id: IAttributeList_::AttrID, value: f64) -> tresult {
        let Some(k) = key(id) else { return kInvalidArgument };
        self.values.lock().insert(k, Attr::Float(value));
        kResultOk
    }

    unsafe fn getFloat(&self, id: IAttributeList_::AttrID, value: *mut f64) -> tresult {
        let Some(k) = key(id) else { return kInvalidArgument };
        match self.values.lock().get(&k) {
            Some(Attr::Float(v)) => {
                *value = *v;
                kResultOk
            }
            _ => kResultFalse,
        }
    }

    unsafe fn setString(&self, id: IAttributeList_::AttrID, string: *const TChar) -> tresult {
        let Some(k) = key(id) else { return kInvalidArgument };
        let mut s = utf16_ptr(string);
        s.push(0);
        self.values.lock().insert(k, Attr::Str(s));
        kResultOk
    }

    unsafe fn getString(&self, id: IAttributeList_::AttrID, string: *mut TChar, size_bytes: uint32) -> tresult {
        let Some(k) = key(id) else { return kInvalidArgument };
        match self.values.lock().get(&k) {
            Some(Attr::Str(s)) => {
                let max = (size_bytes as usize) / 2;
                if max == 0 {
                    return kResultFalse;
                }
                let n = s.len().min(max);
                std::ptr::copy_nonoverlapping(s.as_ptr(), string, n);
                *string.add(n - 1) = 0;
                kResultOk
            }
            _ => kResultFalse,
        }
    }

    unsafe fn setBinary(&self, id: IAttributeList_::AttrID, data: *const c_void, size: uint32) -> tresult {
        let Some(k) = key(id) else { return kInvalidArgument };
        let bytes = if data.is_null() {
            Vec::new()
        } else {
            std::slice::from_raw_parts(data as *const u8, size as usize).to_vec()
        };
        self.values.lock().insert(k, Attr::Bin(bytes));
        kResultOk
    }

    unsafe fn getBinary(&self, id: IAttributeList_::AttrID, data: *mut *const c_void, size: *mut uint32) -> tresult {
        let Some(k) = key(id) else { return kInvalidArgument };
        match self.values.lock().get(&k) {
            Some(Attr::Bin(b)) => {
                // The Vec's heap buffer stays put while the entry lives, which is the lifetime
                // VST3 promises to callers.
                *data = b.as_ptr() as *const c_void;
                *size = b.len() as u32;
                kResultOk
            }
            _ => kResultFalse,
        }
    }
}

pub struct HostMessage {
    id: Mutex<CString>,
    attrs: ComWrapper<HostAttributeList>,
}

impl Default for HostMessage {
    fn default() -> Self {
        HostMessage { id: Mutex::new(CString::default()), attrs: ComWrapper::new(HostAttributeList::default()) }
    }
}

impl Class for HostMessage {
    type Interfaces = (IMessage,);
}

impl IMessageTrait for HostMessage {
    unsafe fn getMessageID(&self) -> FIDString {
        self.id.lock().as_ptr()
    }

    unsafe fn setMessageID(&self, id: FIDString) {
        if !id.is_null() {
            *self.id.lock() = CStr::from_ptr(id).to_owned();
        }
    }

    unsafe fn getAttributes(&self) -> *mut IAttributeList {
        self.attrs.as_com_ref::<IAttributeList>().map(|r| r.as_ptr()).unwrap_or(std::ptr::null_mut())
    }
}
