//! Fill & Sign (Acrobat's Fill & Sign tool, execution plan M5.7): type text onto the page, place
//! ✓ ✕ ● ─ marks and today's date, and sign with a typed, drawn or uploaded signature. Everything
//! is an annotation (typewriter text, PdfCraft-drawn stamps, image stamps, ink), so it can be
//! moved, deleted and undone like any comment.

use std::sync::Arc;

use egui::{Color32, CornerRadius, Pos2, Sense, Stroke, pos2, vec2};
use pdfcraft_engine::{Edit, FillMark, NewAnnotation, Shape, Style};
use pdfcraft_render::DocInfo;

use crate::canvas::{DocView, PageXform};
use crate::theme::Tokens;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FillTool {
    Text,
    Check,
    Cross,
    Dot,
    Line,
    Date,
    Signature,
    Initials,
}

pub const FILL_TOOLS: [FillTool; 8] =
    [FillTool::Text, FillTool::Cross, FillTool::Check, FillTool::Dot, FillTool::Line, FillTool::Date, FillTool::Signature, FillTool::Initials];

/// A saved signature or initials: drawn strokes (normalised to the pad width, y up), typed
/// text (drawn in the script font) or an uploaded picture.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SavedSig {
    Drawn(Vec<Vec<[f32; 2]>>),
    Typed(String),
    Image(SigImage),
}

/// A picture of a signature or initials (a PNG with a transparent background works best).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SigImage {
    /// The source file's name.
    pub file: String,
    #[serde(with = "crate::stamps_ui::b64")]
    pub data: Arc<Vec<u8>>,
    /// Width and height in pixels, so placement keeps the aspect ratio.
    pub size: (u32, u32),
}

/// Signature pictures larger than this are refused (they live in the app's settings).
pub const MAX_SIGNATURE_IMAGE_BYTES: usize = crate::stamps_ui::MAX_STAMP_BYTES;
/// Pictures wider or taller than this many pixels are refused.
pub const MAX_SIGNATURE_IMAGE_SIDE: u32 = 8192;

/// Why a picture can't be a signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SigImageError {
    TooLarge,
    NotAnImage,
}

/// Decode a signature picture, bounded in size and memory. `None` when it isn't a picture
/// PdfCraft can both show and embed (PNG, JPEG, GIF, BMP, TIFF) or it is too large.
pub(crate) fn decode_image(bytes: &[u8]) -> Option<image::RgbaImage> {
    if bytes.len() > MAX_SIGNATURE_IMAGE_BYTES {
        return None;
    }
    let format = image::guess_format(bytes).ok()?;
    if !matches!(
        format,
        image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::Gif | image::ImageFormat::Bmp | image::ImageFormat::Tiff
    ) {
        return None;
    }
    // A PDF that starts like a picture would be embedded as a page; only real pictures pass.
    if bytes.get(..bytes.len().min(1024)).is_some_and(|head| head.windows(5).any(|w| w == b"%PDF-")) {
        return None;
    }
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIGNATURE_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_SIGNATURE_IMAGE_SIDE);
    limits.max_alloc = Some(512 << 20);
    reader.limits(limits);
    let img = reader.decode().ok()?.into_rgba8();
    (img.width() > 0 && img.height() > 0).then_some(img)
}

impl SigImage {
    /// Check an uploaded file: at most [`MAX_SIGNATURE_IMAGE_BYTES`] and a picture that decodes.
    pub fn from_file(file: String, bytes: Vec<u8>) -> Result<Self, SigImageError> {
        if bytes.len() > MAX_SIGNATURE_IMAGE_BYTES {
            return Err(SigImageError::TooLarge);
        }
        let img = decode_image(&bytes).ok_or(SigImageError::NotAnImage)?;
        Ok(Self { file, size: img.dimensions(), data: Arc::new(bytes) })
    }

    /// Re-check a picture read back from settings (untrusted): the stored size is replaced by
    /// the decoded one.
    pub fn validated(self) -> Option<Self> {
        Self::from_file(self.file, Arc::unwrap_or_clone(self.data)).ok()
    }
}

impl SavedSig {
    /// Settings are untrusted: drop values that can't be shown or placed.
    pub fn validated(self) -> Option<Self> {
        match self {
            SavedSig::Image(img) => img.validated().map(SavedSig::Image),
            SavedSig::Drawn(s) => s.iter().flatten().flatten().all(|v| v.is_finite()).then_some(SavedSig::Drawn(s)),
            SavedSig::Typed(t) => (!t.trim().is_empty()).then_some(SavedSig::Typed(t)),
        }
    }
}

/// What a preview texture shows: typed text, or a picture (compared by identity: the cache holds
/// a reference, so the bytes can't be replaced at the same address).
#[derive(Clone, Debug)]
pub(crate) enum PreviewKey {
    Text(String),
    Image(Arc<Vec<u8>>),
}

impl PreviewKey {
    fn is_text(&self, text: &str) -> bool {
        matches!(self, PreviewKey::Text(t) if t == text)
    }

    fn is_image(&self, data: &Arc<Vec<u8>>) -> bool {
        matches!(self, PreviewKey::Image(d) if Arc::ptr_eq(d, data))
    }
}

/// A cached preview: its key and the texture (`None` when the picture couldn't be decoded, so
/// it isn't tried again every frame).
pub(crate) type Preview = Option<(PreviewKey, Option<egui::TextureHandle>)>;

/// A picture's texture for previews, at most `max` pixels on its longer side.
fn image_texture(ctx: &egui::Context, name: &str, data: &[u8], max: u32) -> Option<egui::TextureHandle> {
    let img = decode_image(data)?;
    let (w, h) = img.dimensions();
    let k = (f64::from(max) / f64::from(w.max(h).max(1))).min(1.0);
    let (tw, th) = (((f64::from(w) * k).round() as u32).max(1), ((f64::from(h) * k).round() as u32).max(1));
    let img = if (tw, th) == (w, h) { img } else { image::imageops::thumbnail(&img, tw, th) };
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture(name, color, egui::TextureOptions::LINEAR))
}

/// Paint a picture preview inside `rect`, keeping its aspect ratio.
fn paint_image(painter: &egui::Painter, rect: egui::Rect, tex: &egui::TextureHandle) {
    let [w, h] = tex.size().map(|v| v.max(1) as f32);
    let k = (rect.width() / w).min(rect.height() / h);
    painter.image(
        tex.id(),
        egui::Rect::from_center_size(rect.center(), vec2(w * k, h * k)),
        egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}

/// The Create signature / initials dialog.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SigDraft {
    pub strokes: Vec<Vec<[f32; 2]>>,
    pub text: String,
    /// The Draw tab (otherwise Type).
    pub drawing: bool,
    /// The Upload tab (wins over `drawing`).
    pub uploading: bool,
    /// The chosen picture on the Upload tab.
    pub image: Option<SigImage>,
    /// Creating initials (otherwise the signature).
    pub initials: bool,
    /// Replacing an existing saved value.
    pub editing: bool,
}

impl SigDraft {
    pub fn new(initials: bool, name: &str) -> Self {
        let text = if initials { name.split_whitespace().filter_map(|w| w.chars().next()).collect() } else { name.trim().to_string() };
        Self { text, initials, ..Default::default() }
    }

    /// Edit a copy of the saved value; Cancel must leave the original untouched.
    pub fn from_saved(initials: bool, saved: &SavedSig) -> Self {
        match saved {
            SavedSig::Drawn(strokes) => Self { strokes: strokes.clone(), drawing: true, initials, editing: true, ..Default::default() },
            SavedSig::Typed(text) => Self { text: text.clone(), initials, editing: true, ..Default::default() },
            SavedSig::Image(img) => Self { image: Some(img.clone()), uploading: true, initials, editing: true, ..Default::default() },
        }
    }

    fn ready(&self) -> bool {
        if self.uploading {
            self.image.is_some()
        } else if self.drawing {
            self.strokes.iter().any(|s| s.len() > 1)
        } else {
            !self.text.trim().is_empty()
                && self.text.chars().take(pdfcraft_engine::MAX_SIGNATURE_CHARS + 1).count() <= pdfcraft_engine::MAX_SIGNATURE_CHARS
        }
    }

    pub fn saved(&self) -> SavedSig {
        match (&self.image, self.uploading) {
            (Some(img), true) => SavedSig::Image(img.clone()),
            _ if self.drawing => SavedSig::Drawn(self.strokes.clone()),
            _ => SavedSig::Typed(self.text.trim().to_string()),
        }
    }
}

impl FillTool {
    pub fn command(self) -> &'static str {
        match self {
            FillTool::Text => "sign.fill.text",
            FillTool::Check => "sign.fill.check",
            FillTool::Cross => "sign.fill.cross",
            FillTool::Dot => "sign.fill.dot",
            FillTool::Line => "sign.fill.line",
            FillTool::Date => "sign.fill.date",
            FillTool::Signature => "sign.fill.signature",
            FillTool::Initials => "sign.fill.initials",
        }
    }

    pub fn from_command(id: &str) -> Option<Self> {
        FILL_TOOLS.into_iter().find(|t| t.command() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            FillTool::Text => "Add text",
            FillTool::Check => "Checkmark",
            FillTool::Cross => "Cross",
            FillTool::Dot => "Dot",
            FillTool::Line => "Line",
            FillTool::Date => "Date",
            FillTool::Signature => "Sign",
            FillTool::Initials => "Initials",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            FillTool::Text => "type",
            FillTool::Check => "check",
            FillTool::Cross => "x",
            FillTool::Dot => "circle-dot",
            FillTool::Line => "minus",
            FillTool::Date => "clock-3",
            FillTool::Signature | FillTool::Initials => "signature",
        }
    }

    fn mark(self) -> Option<FillMark> {
        match self {
            FillTool::Check => Some(FillMark::Check),
            FillTool::Cross => Some(FillMark::Cross),
            FillTool::Dot => Some(FillMark::Dot),
            FillTool::Line => Some(FillMark::Line),
            _ => None,
        }
    }
}

/// Text being typed onto a page: (page, top-left in user space, text, focus requested).
#[derive(Clone, Debug, PartialEq)]
pub struct TypeBox {
    pub page: usize,
    pub at: [f64; 2],
    pub text: String,
    pub focus: bool,
}

/// The size Fill & Sign uses for typed text (Acrobat's default is 10 pt).
pub const TEXT_SIZE: f64 = 10.0;

fn to_user(xf: &PageXform, info: &DocInfo, page: usize, p: Pos2) -> [f64; 2] {
    let (vx, vy) = xf.screen_to_view(p);
    let u = info.pages[page].view_to_user(vx, vy);
    [u[0] as f64, u[1] as f64]
}

fn new(page: usize, shape: Shape, contents: String, author: &str) -> Edit {
    let style = Style::default_for(&shape);
    Edit::AddAnnotation(NewAnnotation { page, shape, style, contents, author: author.to_string() })
}

/// A typewriter annotation sized to its text.
pub fn typed(page: usize, at: [f64; 2], text: &str, author: &str) -> Edit {
    let rect = crate::comments::text_box_rect(at, text, TEXT_SIZE);
    new(page, Shape::Typewriter { rect, font_size: TEXT_SIZE }, text.to_string(), author)
}

/// Place a saved signature (strokes normalised to a 0–1 box, y up) with its left edge at `at`,
/// 150 pt wide.
pub fn signature_at(page: usize, at: [f64; 2], strokes: &[Vec<[f32; 2]>], author: &str) -> Option<Edit> {
    let w = 150.0;
    let (min_y, max_y) = strokes.iter().flatten().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p[1]), b.max(p[1])));
    if !min_y.is_finite() {
        return None;
    }
    let h = f64::from(max_y - min_y).max(0.05) * w;
    let strokes: Vec<Vec<[f64; 2]>> = strokes
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| s.iter().map(|p| [at[0] + f64::from(p[0]) * w, at[1] - h / 2.0 + f64::from(p[1] - min_y) * w]).collect())
        .collect();
    (!strokes.is_empty()).then(|| new(page, Shape::Signature { strokes }, String::new(), author))
}

/// Place typed text in the script font with its left edge at `at`, `height` points tall.
pub fn typed_signature_at(page: usize, at: [f64; 2], text: &str, height: f64, author: &str) -> Option<Edit> {
    pdfcraft_engine::typed_signature_shape(at, text, height).map(|shape| new(page, shape, String::new(), author))
}

/// Place a picture with its left edge at `at`, vertically centred there: a signature fits in
/// 150 × 40 pt, initials in 75 × 24 pt, keeping the picture's aspect ratio. It is an image stamp
/// (transparency kept), so it moves, resizes and undoes like any comment.
pub fn image_signature_at(page: usize, at: [f64; 2], img: &SigImage, initials: bool, author: &str) -> Option<Edit> {
    let (pw, ph) = (f64::from(img.size.0), f64::from(img.size.1));
    if pw < 1.0 || ph < 1.0 || !at.iter().all(|v| v.is_finite()) {
        return None;
    }
    let (max_w, max_h) = if initials { (75.0, 24.0) } else { (150.0, 40.0) };
    let k = (max_w / pw).min(max_h / ph);
    // At least a point each way: the engine reads a zero-size rectangle as "natural size".
    let (w, h) = ((pw * k).max(1.0), (ph * k).max(1.0));
    Some(Edit::AddCustomStamp {
        page,
        rect: [at[0], at[1] - h / 2.0, at[0] + w, at[1] + h / 2.0],
        name: if initials { "Initials" } else { "Signature" }.into(),
        file: pdfcraft_engine::MarkFile { name: img.file.clone(), bytes: img.data.clone(), page: 0 },
        author: author.to_string(),
    })
}

/// Place a saved signature or initials.
pub fn place(page: usize, at: [f64; 2], sig: &SavedSig, initials: bool, author: &str) -> Option<Edit> {
    match sig {
        SavedSig::Image(img) => image_signature_at(page, at, img, initials, author),
        SavedSig::Drawn(strokes) => signature_at(page, at, strokes, author),
        SavedSig::Typed(text) => {
            let [left, bottom, right, top] = pdfcraft_engine::script_outline(text).bounds();
            let height = if initials { 24.0_f64 } else { 32.0_f64 };
            // Keep long names within the same placement width as drawn signatures.
            let height = height.min(150.0 * (top - bottom).max(0.1) / (right - left).max(0.01));
            typed_signature_at(page, at, text, height, author)
        }
    }
}

/// The text in the script font as a picture (`w`×`h` px, black on transparent), for previews.
pub(crate) fn script_preview(text: &str, w: usize, h: usize) -> egui::ColorImage {
    let o = pdfcraft_engine::script_outline(text);
    let mut img = egui::ColorImage::filled([w, h], Color32::TRANSPARENT);
    let [left, bottom, right, top] = o.bounds();
    let span = (top - bottom).max(0.1);
    let width = (right - left).max(0.01);
    if o.contours.is_empty() {
        return img;
    }
    let k = ((h as f64 * 0.9) / span).min((w as f64 * 0.95) / width);
    let x0 = (w as f64 - width * k) / 2.0;
    // Device points (y down), then an even-odd scanline fill.
    let polys: Vec<Vec<(f64, f64)>> = o
        .contours
        .iter()
        .map(|c| c.iter().map(|p| (x0 + (p[0] - left) * k, h as f64 * 0.5 + (top + bottom) / 2.0 * k - p[1] * k)).collect())
        .collect();
    for y in 0..h {
        let sy = y as f64 + 0.5;
        let mut xs: Vec<f64> = Vec::new();
        for poly in &polys {
            for i in 0..poly.len() {
                let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                if (a.1 <= sy) != (b.1 <= sy) {
                    xs.push(a.0 + (sy - a.1) / (b.1 - a.1) * (b.0 - a.0));
                }
            }
        }
        xs.sort_by(|a, b| a.total_cmp(b));
        for pair in xs.as_chunks::<2>().0 {
            let (from, to) = (pair[0].round().clamp(0.0, w as f64) as usize, pair[1].round().clamp(0.0, w as f64) as usize);
            for x in from..to {
                img[(x, y)] = Color32::BLACK;
            }
        }
    }
    img
}

/// Saved previews and add/remove controls, shared by the left panel and quick-tool picker.
pub(crate) fn signature_entries(ui: &mut egui::Ui, app: &mut crate::PdfCraftApp, t: &Tokens) -> Option<&'static str> {
    ui.set_width(ui.available_width().clamp(240.0, 248.0));
    let mut command = None;
    for (i, (saved, what, use_id, change_id, remove_id)) in [
        (app.signature.as_ref(), "signature", "sign.fill.signature", "sign.fill.signature.change", "sign.fill.signature.remove"),
        (app.initials.as_ref(), "initials", "sign.fill.initials", "sign.fill.initials.change", "sign.fill.initials.remove"),
    ]
    .into_iter()
    .enumerate()
    {
        let label = |template: &str| crate::i18n::fmt(tl!(template), &[("what", tl!(what))]);
        let Some(saved) = saved else {
            if crate::widgets::ghost_button(ui, "plus", &label("Add {what}")).clicked() {
                command = Some(use_id);
            }
            continue;
        };
        ui.horizontal(|ui| {
            let (rect, response) = ui.allocate_exact_size(vec2((ui.available_width() - 68.0).max(140.0), 52.0), Sense::click());
            let use_label = label("Use {what}");
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, use_label.clone()));
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, CornerRadius::same(4), Color32::WHITE);
            painter.rect_stroke(
                rect,
                CornerRadius::same(4),
                Stroke::new(1.0, if response.hovered() { t.accent } else { t.border }),
                egui::StrokeKind::Inside,
            );
            let preview_rect = rect.shrink(8.0);
            match saved {
                SavedSig::Image(img) => {
                    let Some(cache) = app.saved_signature_previews.get_mut(i) else { return };
                    if cache.as_ref().is_none_or(|(k, _)| !k.is_image(&img.data)) {
                        *cache = Some((PreviewKey::Image(img.data.clone()), image_texture(ui.ctx(), &format!("saved-{what}"), &img.data, 256)));
                    }
                    if let Some((_, Some(tex))) = cache {
                        paint_image(&painter, preview_rect, tex);
                    }
                }
                SavedSig::Typed(text) => {
                    let Some(cache) = app.saved_signature_previews.get_mut(i) else { return };
                    if cache.as_ref().is_none_or(|(k, _)| !k.is_text(text)) {
                        *cache = Some((
                            PreviewKey::Text(text.clone()),
                            Some(ui.ctx().load_texture(format!("saved-{what}"), script_preview(text, 480, 104), egui::TextureOptions::LINEAR)),
                        ));
                    }
                    if let Some((_, Some(tex))) = cache {
                        let size = vec2(480.0, 104.0) * (preview_rect.width() / 480.0).min(preview_rect.height() / 104.0);
                        painter.image(
                            tex.id(),
                            egui::Rect::from_center_size(preview_rect.center(), size),
                            egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                }
                SavedSig::Drawn(strokes) => {
                    let bounds = strokes
                        .iter()
                        .flatten()
                        .filter(|p| p.iter().all(|v| v.is_finite()))
                        .fold(egui::Rect::NOTHING, |r, p| r.union(egui::Rect::from_min_max(pos2(p[0], p[1]), pos2(p[0], p[1]))));
                    if bounds.is_finite() {
                        let scale = (preview_rect.width() / bounds.width().max(0.01)).min(preview_rect.height() / bounds.height().max(0.01));
                        for stroke in strokes {
                            let pts = stroke
                                .iter()
                                .filter(|p| p.iter().all(|v| v.is_finite()))
                                .map(|p| preview_rect.center() + vec2(p[0] - bounds.center().x, bounds.center().y - p[1]) * scale)
                                .collect();
                            painter.add(egui::Shape::line(pts, Stroke::new(1.5, Color32::BLACK)));
                        }
                    }
                }
            }
            if response.on_hover_text(label("Place saved {what}")).clicked() {
                command = Some(use_id);
            }
            if crate::icons::button(ui, "pencil", 26.0, false, &label("Change {what}")).clicked() {
                command = Some(change_id);
            }
            if crate::icons::button(ui, "x", 26.0, false, &label("Remove saved {what}")).clicked() {
                command = Some(remove_id);
            }
        });
        ui.add_space(4.0);
    }
    command
}

impl crate::PdfCraftApp {
    /// Create signature ▸ Upload ▸ Choose image…: pick a picture for the open dialog.
    pub(crate) fn pick_signature_image(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let initials = self.signature_draft.initials;
            let read = move |app: &mut Self, path: std::path::PathBuf| {
                // The dialog may have closed (or switched to the other kind) while the picker
                // was open: the picture then belongs to nothing.
                if app.dialog != Some(crate::Dialog::Signature) || app.signature_draft.initials != initials {
                    return;
                }
                match std::fs::read(&path) {
                    Ok(bytes) => {
                        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        app.use_signature_image(name, bytes);
                    }
                    Err(e) => app.notify_fmt("Couldn't read {name}: {e}", &[("name", &path.display().to_string()), ("e", &e.to_string())]),
                }
            };
            const EXTENSIONS: [&str; 7] = ["png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff"];
            match self.save_override.clone() {
                Some(p) if EXTENSIONS.iter().any(|e| p.to_ascii_lowercase().ends_with(&format!(".{e}"))) => read(self, p.into()),
                Some(_) => {}
                None => {
                    let dialog = rfd::AsyncFileDialog::new().add_filter(tl!("Image"), &EXTENSIONS).set_title(tl!("Choose an image"));
                    self.ask_one(crate::pickers::Ask::File(dialog), None, read);
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        self.notify_tr("Uploading a signature picture arrives on the web with file pickers for images");
    }

    /// Put a picture into the open signature dialog's Upload tab, or tell the user why not.
    pub fn use_signature_image(&mut self, file: String, bytes: Vec<u8>) {
        match SigImage::from_file(file.clone(), bytes) {
            Ok(img) => {
                self.signature_draft.uploading = true;
                self.signature_draft.image = Some(img);
            }
            Err(SigImageError::TooLarge) => self.notify_fmt(
                "{file} is too large for a signature (at most {n} MB)",
                &[("file", &file), ("n", &(MAX_SIGNATURE_IMAGE_BYTES >> 20).to_string())],
            ),
            Err(SigImageError::NotAnImage) => {
                self.notify_fmt("{file} isn't a picture PdfCraft can use. Choose a PNG, JPEG, GIF, BMP or TIFF image.", &[("file", &file)])
            }
        }
    }
}

/// Clicks with a Fill & Sign tool on one page. Returns `true` when the click was used.
#[allow(clippy::too_many_arguments)]
pub(crate) fn page_input(
    ui: &egui::Ui,
    resp: &egui::Response,
    xf: &PageXform,
    page: usize,
    info: &DocInfo,
    tool: FillTool,
    view: &mut DocView,
    signature: Option<&SavedSig>,
    initials: Option<&SavedSig>,
    author: &str,
    today: (i64, u32, u32),
) -> Option<FillAction> {
    let pointer = ui.input(|i| i.pointer.hover_pos())?;
    if !xf.rect.contains(pointer) {
        return None;
    }
    ui.ctx().set_cursor_icon(if tool == FillTool::Text { egui::CursorIcon::Text } else { egui::CursorIcon::Crosshair });
    if !resp.clicked() {
        return None;
    }
    let at = to_user(xf, info, page, pointer);
    match tool {
        FillTool::Text => {
            view.fill_text = Some(TypeBox { page, at: [at[0], at[1] + TEXT_SIZE * 0.6], text: String::new(), focus: true });
            None
        }
        FillTool::Date => {
            let (y, m, d) = today;
            Some(FillAction::Edit(Box::new(typed(page, [at[0], at[1] + TEXT_SIZE * 0.6], &format!("{m}/{d}/{y}"), author))))
        }
        FillTool::Signature => match signature {
            Some(s) => place(page, at, s, false, author).map(|e| FillAction::Edit(Box::new(e))),
            None => Some(FillAction::CreateSignature),
        },
        FillTool::Initials => match initials {
            Some(s) => place(page, at, s, true, author).map(|e| FillAction::Edit(Box::new(e))),
            None => Some(FillAction::CreateInitials),
        },
        mark => {
            let mark = mark.mark()?;
            let (w, h) = if mark == FillMark::Line { (36.0, 4.0) } else { (12.0, 12.0) };
            let rect = [at[0] - w / 2.0, at[1] - h / 2.0, at[0] + w / 2.0, at[1] + h / 2.0];
            Some(FillAction::Edit(Box::new(new(page, Shape::Mark { rect, mark }, String::new(), author))))
        }
    }
}

/// What a Fill & Sign click asks the app to do.
#[derive(Clone, Debug, PartialEq)]
pub enum FillAction {
    Edit(Box<Edit>),
    /// No signature yet: open the signature pad.
    CreateSignature,
    /// No initials yet.
    CreateInitials,
}

/// The in-place editor for typed text. Returns the edit once committed.
pub(crate) fn type_box(ctx: &egui::Context, view: &mut DocView, info: &DocInfo, author: &str) -> Option<Edit> {
    let tb = view.fill_text.clone()?;
    let xf = view.page_xform(tb.page)?;
    let v = info.pages.get(tb.page)?.user_to_view(tb.at[0] as f32, tb.at[1] as f32);
    let pos = xf.norm_to_screen(v[0] / xf.pw, v[1] / xf.ph);
    let zoom = xf.rect.width() / xf.pw.max(1.0);
    let mut commit = false;
    let mut cancel = false;
    egui::Area::new(egui::Id::new(("fill-text", view.id.0))).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        let Some(t) = view.fill_text.as_mut() else { return };
        let width = ((t.text.len().max(8) as f32) * TEXT_SIZE as f32 * 0.6 * zoom).clamp(60.0, 600.0);
        let r = ui.add(
            egui::TextEdit::singleline(&mut t.text)
                .font(egui::FontId::proportional((TEXT_SIZE as f32 * zoom).max(8.0)))
                .desired_width(width)
                .background_color(Color32::from_rgba_unmultiplied(255, 255, 255, 230))
                .text_color(Color32::BLACK)
                .hint_text(tl!("Type text"))
                .id_salt("fill-text-edit"),
        );
        if t.focus {
            r.request_focus();
            t.focus = false;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        } else if r.lost_focus() {
            commit = true;
        }
    });
    if cancel {
        view.fill_text = None;
        return None;
    }
    if commit {
        let tb = view.fill_text.take()?;
        if !tb.text.trim().is_empty() {
            return Some(typed(tb.page, tb.at, tb.text.trim(), author));
        }
    }
    None
}

/// What the signature dialog asks for this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PadResult {
    pub apply: bool,
    pub cancel: bool,
    /// Upload ▸ Choose image…
    pub choose: bool,
}

/// The signature pad: type, draw with the pointer, or upload a picture; Apply keeps the result.
pub(crate) fn signature_pad(ui: &mut egui::Ui, t: &Tokens, d: &mut SigDraft, preview: &mut Preview) -> PadResult {
    let what = if d.initials { tl!("initials") } else { tl!("signature") };
    let title = if d.editing { tl!("Change {what}") } else { tl!("Create {what}") };
    ui.label(egui::RichText::new(crate::i18n::fmt(title, &[("what", what)])).font(crate::theme::semibold(18.0)));
    ui.horizontal(|ui| {
        if crate::widgets::pill_button(ui, tl!("Type"), !d.drawing && !d.uploading).clicked() {
            d.drawing = false;
            d.uploading = false;
        }
        if crate::widgets::pill_button(ui, tl!("Draw"), d.drawing && !d.uploading).clicked() {
            d.drawing = true;
            d.uploading = false;
        }
        if crate::widgets::pill_button(ui, tl!("Upload"), d.uploading).clicked() {
            d.uploading = true;
        }
    });
    ui.add_space(6.0);
    if d.uploading {
        let mut choose = false;
        ui.label(
            egui::RichText::new(crate::i18n::fmt(
                tl!("Choose a picture of your {what}. A PNG with a transparent background works best."),
                &[("what", what)],
            ))
            .color(t.text_muted),
        );
        ui.horizontal(|ui| {
            if ui.button(tl!("Choose image…")).clicked() {
                choose = true;
            }
            if let Some(img) = &d.image {
                ui.label(egui::RichText::new(&img.file).color(t.text_muted));
            }
        });
        let (rect, _) = ui.allocate_exact_size(vec2(460.0, 150.0), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::same(6), Color32::WHITE);
        painter.rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        match &d.image {
            Some(img) => {
                if preview.as_ref().is_none_or(|(k, _)| !k.is_image(&img.data)) {
                    *preview = Some((PreviewKey::Image(img.data.clone()), image_texture(ui.ctx(), "uploaded-signature", &img.data, 920)));
                }
                if let Some((_, Some(tex))) = preview {
                    paint_image(&painter, rect.shrink(8.0), tex);
                }
            }
            None => {
                ui.put(rect, egui::Label::new(egui::RichText::new(tl!("No image chosen")).color(Color32::GRAY)));
            }
        }
        let (apply, cancel) = pad_buttons(ui, d);
        return PadResult { apply, cancel, choose };
    }
    if !d.drawing {
        let l = ui.label(egui::RichText::new(crate::i18n::fmt(tl!("Type your {what}."), &[("what", what)])).color(t.text_muted));
        ui.add(
            egui::TextEdit::singleline(&mut d.text)
                .char_limit(pdfcraft_engine::MAX_SIGNATURE_CHARS)
                .desired_width(460.0)
                .hint_text(tl!(if d.initials { "Initials" } else { "Your name" })),
        )
        .labelled_by(l.id);
        let (rect, _) = ui.allocate_exact_size(vec2(460.0, 150.0), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::same(6), Color32::WHITE);
        painter.rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        if preview.as_ref().is_none_or(|(k, _)| !k.is_text(&d.text)) {
            let img = script_preview(&d.text, 920, 300);
            *preview = Some((PreviewKey::Text(d.text.clone()), Some(ui.ctx().load_texture("typed-signature", img, egui::TextureOptions::LINEAR))));
        }
        if let Some((_, Some(tex))) = preview {
            painter.image(tex.id(), rect.shrink(4.0), egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        let (apply, cancel) = pad_buttons(ui, d);
        return PadResult { apply, cancel, choose: false };
    }
    ui.label(egui::RichText::new(crate::i18n::fmt(tl!("Draw your {what} below."), &[("what", what)])).color(t.text_muted));
    let strokes = &mut d.strokes;
    let (rect, resp) = ui.allocate_exact_size(vec2(460.0, 150.0), Sense::drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(6), Color32::WHITE);
    painter.rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    painter.hline(rect.x_range().shrink(24.0), rect.bottom() - 34.0, Stroke::new(1.0, t.divider));
    // Normalised: x 0–1 across the pad, y up, in units of the pad's width.
    let norm = |p: Pos2| [(p.x - rect.left()) / rect.width(), (rect.bottom() - p.y) / rect.width()];
    if resp.drag_started() {
        strokes.push(Vec::new());
    }
    if resp.dragged()
        && let (Some(p), Some(s)) = (resp.interact_pointer_pos(), strokes.last_mut())
    {
        let n = norm(p.clamp(rect.min, rect.max));
        if s.last().is_none_or(|l| (l[0] - n[0]).abs() + (l[1] - n[1]).abs() > 0.002) {
            s.push(n);
        }
    }
    for s in strokes.iter() {
        let pts: Vec<Pos2> = s.iter().map(|p| pos2(rect.left() + p[0] * rect.width(), rect.bottom() - p[1] * rect.width())).collect();
        painter.add(egui::Shape::line(pts, Stroke::new(2.0, Color32::BLACK)));
    }
    let (apply, cancel) = pad_buttons(ui, d);
    PadResult { apply, cancel, choose: false }
}

fn pad_buttons(ui: &mut egui::Ui, d: &mut SigDraft) -> (bool, bool) {
    ui.add_space(10.0);
    let (mut apply, mut cancel) = (false, false);
    ui.horizontal(|ui| {
        if ui.button(tl!("Clear")).clicked() {
            d.strokes.clear();
            d.text.clear();
            d.image = None;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ready = d.ready();
            if ui.add_enabled_ui(ready, |ui| crate::widgets::pill_button(ui, tl!("Apply"), true)).inner.clicked() {
                apply = true;
            }
            if crate::widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
                cancel = true;
            }
        });
    });
    (apply, cancel)
}

#[cfg(test)]
mod tests {
    #[test]
    fn long_signature_previews_have_clear_margins() {
        for text in ["Alexandria Catherine Elizabeth Montgomery-Wellington", "Jg Jg Jg Jg Jg Jg Jg Jg Jg Jg Jg"] {
            for [w, h] in [[920, 300], [480, 104]] {
                let image = super::script_preview(text, w, h);
                assert!(image.pixels.iter().any(|p| p.a() > 0));
                for x in 0..w {
                    assert_eq!(image[(x, 0)].a(), 0);
                    assert_eq!(image[(x, h - 1)].a(), 0);
                }
                for y in 0..h {
                    assert_eq!(image[(0, y)].a(), 0);
                    assert_eq!(image[(w - 1, y)].a(), 0);
                }
            }
        }
    }
}
