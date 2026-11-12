//! Debug builds on macOS: a PNG of the main window's web view (WKWebView.takeSnapshot), for the
//! scripted UI checks. Renders at full size even when the window isn't frontmost.

use objc2::rc::Retained;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
use objc2_foundation::{NSDictionary, NSError};
use objc2_web_kit::WKWebView;
use tauri::{AppHandle, Manager};

pub fn snapshot(app: &AppHandle, path: String) -> Result<(), String> {
    let w = app.get_webview_window("main").ok_or("no main window")?;
    w.with_webview(move |pv| unsafe {
        let view: &WKWebView = &*(pv.inner() as *const WKWebView);
        let path = path.clone();
        let block = block2::RcBlock::new(move |img: *mut NSImage, _err: *mut NSError| {
            let Some(img) = img.as_ref() else { return };
            let Some(tiff) = img.TIFFRepresentation() else { return };
            let Some(rep): Option<Retained<NSBitmapImageRep>> = NSBitmapImageRep::imageRepWithData(&tiff) else { return };
            let props = NSDictionary::new();
            if let Some(png) = rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &props) {
                let _ = std::fs::write(&path, png.to_vec());
            }
        });
        view.takeSnapshotWithConfiguration_completionHandler(None, &block);
    })
    .map_err(|e| e.to_string())
}
