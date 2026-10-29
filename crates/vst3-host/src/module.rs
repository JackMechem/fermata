//! Loading VST3 bundles and talking to their plugin factory.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use vst3::Steinberg::*;
use vst3::{ComPtr, Interface};

use crate::util::{c_str, hex};

/// A loaded VST3 module. Modules are cached for the lifetime of the process and never unloaded:
/// many plugins do not survive being dlclose()d and reopened.
pub struct Module {
    pub path: PathBuf,
    factory: ComPtr<IPluginFactory>,
    _lib: libloading::Library,
}

unsafe impl Send for Module {}
unsafe impl Sync for Module {}

/// One class exported by a module's factory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginDescriptor {
    /// Stable identifier: `<bundle path>|<class id hex>`.
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub path: PathBuf,
    pub cid: String,
    /// e.g. "Fx|Reverb" or "Instrument|Synth".
    pub subcategories: String,
    pub is_instrument: bool,
}

impl PluginDescriptor {
    /// Top-level grouping used by the plug-in menus, mirroring Logic's category submenus.
    pub fn category(&self) -> String {
        let mut parts = self.subcategories.split('|').filter(|s| !s.is_empty() && *s != "Fx");
        parts.next().unwrap_or("Effect").to_string()
    }
}

static MODULES: OnceLock<Mutex<HashMap<PathBuf, Arc<Module>>>> = OnceLock::new();

pub fn load_module(bundle: &Path) -> Result<Arc<Module>, String> {
    let cache = MODULES.get_or_init(|| Mutex::new(HashMap::new()));
    let key = bundle.to_path_buf();
    if let Some(m) = cache.lock().get(&key) {
        return Ok(m.clone());
    }
    let module = Arc::new(unsafe { Module::open(bundle)? });
    cache.lock().insert(key, module.clone());
    Ok(module)
}

fn binary_path(bundle: &Path) -> Option<PathBuf> {
    if bundle.is_file() {
        return Some(bundle.to_path_buf());
    }
    let stem = bundle.file_stem()?.to_string_lossy().to_string();
    let contents = bundle.join("Contents");

    #[cfg(target_os = "linux")]
    let (dir, ext) = (contents.join(format!("{}-linux", std::env::consts::ARCH)), "so");
    #[cfg(target_os = "windows")]
    let (dir, ext) = (
        contents.join(if cfg!(target_arch = "aarch64") { "arm64-win" } else { "x86_64-win" }),
        "vst3",
    );
    #[cfg(target_os = "macos")]
    let (dir, ext) = (contents.join("MacOS"), "");

    let direct = if ext.is_empty() { dir.join(&stem) } else { dir.join(format!("{stem}.{ext}")) };
    if direct.exists() {
        return Some(direct);
    }
    // Some bundles name the binary differently from the bundle folder.
    std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.is_file() && (ext.is_empty() || p.extension().map(|e| e == ext).unwrap_or(false))
    })
}

impl Module {
    unsafe fn open(bundle: &Path) -> Result<Module, String> {
        let bin = binary_path(bundle).ok_or_else(|| format!("no binary found in {}", bundle.display()))?;

        #[cfg(target_os = "linux")]
        let lib: libloading::Library = {
            use libloading::os::unix::{Library, RTLD_LAZY, RTLD_LOCAL};
            let lib = Library::open(Some(&bin), RTLD_LAZY | RTLD_LOCAL).map_err(|e| e.to_string())?;
            let handle = lib.into_raw();
            let lib = Library::from_raw(handle);
            if let Ok(entry) = lib.get::<unsafe extern "C" fn(*mut std::ffi::c_void) -> bool>(b"ModuleEntry\0") {
                if !entry(handle) {
                    return Err("ModuleEntry returned false".into());
                }
            }
            lib.into()
        };

        #[cfg(target_os = "windows")]
        let lib: libloading::Library = {
            let lib = libloading::Library::new(&bin).map_err(|e| e.to_string())?;
            if let Ok(init) = lib.get::<unsafe extern "system" fn() -> bool>(b"InitDll\0") {
                init();
            }
            lib
        };

        #[cfg(target_os = "macos")]
        let lib: libloading::Library = {
            let lib = libloading::Library::new(&bin).map_err(|e| e.to_string())?;
            if let Ok(entry) = lib.get::<unsafe extern "C" fn(*mut std::ffi::c_void) -> bool>(b"bundleEntry\0") {
                let bundle_ref = crate::macos::create_bundle_ref(bundle);
                if !entry(bundle_ref) {
                    return Err("bundleEntry returned false".into());
                }
            }
            lib
        };

        let get_factory = lib
            .get::<unsafe extern "system" fn() -> *mut IPluginFactory>(b"GetPluginFactory\0")
            .map_err(|e| e.to_string())?;
        let factory = ComPtr::from_raw(get_factory()).ok_or("GetPluginFactory returned null")?;

        if let Some(f3) = factory.cast::<IPluginFactory3>() {
            let host = crate::host::host_context();
            f3.setHostContext(host.as_ptr());
        }

        Ok(Module { path: bundle.to_path_buf(), factory, _lib: lib })
    }

    pub fn factory(&self) -> &ComPtr<IPluginFactory> {
        &self.factory
    }

    /// Lists the audio processor classes in this module.
    pub fn descriptors(&self) -> Vec<PluginDescriptor> {
        let mut out = Vec::new();
        unsafe {
            let mut finfo: PFactoryInfo = std::mem::zeroed();
            let factory_vendor = if self.factory.getFactoryInfo(&mut finfo) == kResultOk {
                c_str(&finfo.vendor)
            } else {
                String::new()
            };
            let f2 = self.factory.cast::<IPluginFactory2>();
            for i in 0..self.factory.countClasses() {
                let mut info: PClassInfo = std::mem::zeroed();
                if self.factory.getClassInfo(i, &mut info) != kResultOk {
                    continue;
                }
                if c_str(&info.category) != "Audio Module Class" {
                    continue;
                }
                let cid: [u8; 16] = std::mem::transmute(info.cid);
                let mut name = c_str(&info.name);
                let mut vendor = factory_vendor.clone();
                let mut version = String::new();
                let mut subcategories = String::new();
                if let Some(f2) = &f2 {
                    let mut info2: PClassInfo2 = std::mem::zeroed();
                    if f2.getClassInfo2(i, &mut info2) == kResultOk {
                        name = c_str(&info2.name);
                        subcategories = c_str(&info2.subCategories);
                        version = c_str(&info2.version);
                        let v = c_str(&info2.vendor);
                        if !v.is_empty() {
                            vendor = v;
                        }
                    }
                }
                out.push(PluginDescriptor {
                    id: format!("{}|{}", self.path.display(), hex(&cid)),
                    name,
                    vendor,
                    version,
                    path: self.path.clone(),
                    cid: hex(&cid),
                    is_instrument: subcategories.contains("Instrument"),
                    subcategories,
                });
            }
        }
        out
    }

    pub(crate) unsafe fn create<I: Interface>(&self, cid: &[u8; 16]) -> Option<ComPtr<I>> {
        let mut obj: *mut std::ffi::c_void = std::ptr::null_mut();
        let iid = I::IID;
        let r = self.factory.createInstance(
            cid.as_ptr() as FIDString,
            iid.as_ptr() as FIDString,
            &mut obj,
        );
        if r != kResultOk || obj.is_null() {
            return None;
        }
        ComPtr::from_raw(obj as *mut I)
    }
}
