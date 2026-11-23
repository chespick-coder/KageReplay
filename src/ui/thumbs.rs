use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crossbeam_channel::{unbounded, Receiver, Sender};
use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use windows::Win32::Graphics::Gdi::{DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_THUMBNAILONLY};
use windows::Win32::Foundation::SIZE;

use crate::util::WideString;

pub struct Thumbs {
    requests: Sender<PathBuf>,
    results: Receiver<(PathBuf, Option<ColorImage>)>,
    textures: HashMap<PathBuf, Option<TextureHandle>>,
    pending: HashSet<PathBuf>,
}

impl Thumbs {
    pub fn new(ctx: egui::Context) -> Self {
        let (requests, request_rx) = unbounded::<PathBuf>();
        let (result_tx, results) = unbounded();
        std::thread::Builder::new()
            .name("kage-thumbs".into())
            .spawn(move || {
                unsafe {
                    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                }
                while let Ok(path) = request_rx.recv() {
                    let image = load(&path);
                    if result_tx.send((path, image)).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
            })
            .expect("thumbnail thread");
        Self { requests, results, textures: HashMap::new(), pending: HashSet::new() }
    }

    pub fn get(&mut self, ctx: &egui::Context, path: &Path) -> Option<TextureHandle> {
        while let Ok((done, image)) = self.results.try_recv() {
            self.pending.remove(&done);
            let texture = image.map(|img| ctx.load_texture(done.display().to_string(), img, TextureOptions::LINEAR));
            self.textures.insert(done, texture);
        }
        if let Some(entry) = self.textures.get(path) {
            return entry.clone();
        }
        if self.pending.insert(path.to_path_buf()) {
            let _ = self.requests.send(path.to_path_buf());
        }
        None
    }

    pub fn forget(&mut self, path: &Path) {
        self.textures.remove(path);
    }

    pub fn retain(&mut self, keep: &HashSet<PathBuf>) {
        self.textures.retain(|p, _| keep.contains(p));
    }
}

fn load(path: &Path) -> Option<ColorImage> {
    unsafe {
        let wide = WideString::new(&path.display().to_string());
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(wide.pcwstr(), None).ok()?;
        let bitmap = factory.GetImage(SIZE { cx: 384, cy: 216 }, SIIGBF_BIGGERSIZEOK | SIIGBF_THUMBNAILONLY).ok()?;
        let object = HGDIOBJ(bitmap.0);
        let mut info = BITMAP::default();
        if GetObjectW(object, std::mem::size_of::<BITMAP>() as i32, Some(&mut info as *mut _ as *mut _)) == 0 {
            let _ = DeleteObject(object);
            return None;
        }
        let (width, height) = (info.bmWidth.max(1) as usize, info.bmHeight.unsigned_abs().max(1) as usize);
        let mut header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; width * height * 4];
        let dc = GetDC(None);
        let lines = GetDIBits(dc, bitmap, 0, height as u32, Some(pixels.as_mut_ptr() as *mut _), &mut header, DIB_RGB_COLORS);
        ReleaseDC(None, dc);
        let _ = DeleteObject(object);
        if lines == 0 {
            return None;
        }
        let has_alpha = pixels.as_chunks::<4>().0.iter().any(|p| p[3] != 0);
        let mut rgba = Vec::with_capacity(pixels.len());
        for p in pixels.as_chunks::<4>().0 {
            rgba.extend_from_slice(&[p[2], p[1], p[0], if has_alpha { p[3] } else { 255 }]);
        }
        Some(if has_alpha { ColorImage::from_rgba_premultiplied([width, height], &rgba) } else { ColorImage::from_rgba_unmultiplied([width, height], &rgba) })
    }
}
