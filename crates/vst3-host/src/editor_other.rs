//! Placeholder editor for platforms whose native window embedding is not implemented yet.

use vst3::ComPtr;
use vst3::Steinberg::IPlugView;

pub struct Editor;

impl Editor {
    pub fn open(_view: ComPtr<IPlugView>, _title: &str) -> Result<Editor, String> {
        Err("plugin editor windows are not implemented on this platform yet".into())
    }
    pub fn pump(&mut self) -> bool {
        true
    }
    pub fn raise(&self) {}
    pub fn close<K: Send + 'static>(self, _keep_alive: K) {}
}
