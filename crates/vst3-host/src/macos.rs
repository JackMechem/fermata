use std::ffi::c_void;
use std::path::Path;

/// Creates a CFBundleRef for `bundleEntry`. The bundle is intentionally leaked: modules are never
/// unloaded.
pub fn create_bundle_ref(path: &Path) -> *mut c_void {
    use core_foundation::base::TCFType;
    use core_foundation::bundle::CFBundle;
    use core_foundation::url::CFURL;
    let Some(url) = CFURL::from_path(path, true) else { return std::ptr::null_mut() };
    let Some(bundle) = CFBundle::new(url) else { return std::ptr::null_mut() };
    let r = bundle.as_concrete_TypeRef();
    std::mem::forget(bundle);
    r as *mut c_void
}
