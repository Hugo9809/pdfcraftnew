//! Fill & Sign in the real shell (egui_kittest): text, marks, date and a drawn signature.

use egui::{Pos2, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::fill_sign::{FillTool, SavedSig};
use pdfcraft_ui_egui::{Dialog, PdfCraftApp, QuickTool};

const FIXTURE: &[u8] = b"%PDF-1.7
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 400] >> endobj
3 0 obj << /Type /Page /Parent 2 0 R >> endobj
trailer << /Root 1 0 R >>
%%EOF";

fn harness() -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.open_bytes("form.pdf", None, FIXTURE.to_vec()).unwrap();
        app.set_option("left", "closed").unwrap();
        app.set_option("zoom", "150").unwrap();
        app.set_option("author", "Ada").unwrap();
        app
    });
    h.run_steps(5);
    h
}

fn at(h: &Harness<'static, PdfCraftApp>, x: f32, y: f32) -> Pos2 {
    let r = h.state().views[0].page_screen_rect(0).expect("on screen");
    pos2(r.left() + x / 300.0 * r.width(), r.top() + (400.0 - y) / 400.0 * r.height())
}

fn click(h: &mut Harness<'static, PdfCraftApp>, x: f32, y: f32) {
    let p = at(h, x, y);
    h.hover_at(p);
    h.run_steps(1);
    h.drag_at(p);
    h.run_steps(1);
    h.drop_at(p);
    h.run_steps(3);
}

fn items(h: &Harness<'static, PdfCraftApp>) -> Vec<(String, Option<String>)> {
    let s = h.state();
    let mut v: Vec<_> = s.session.get(s.views[0].id).unwrap().info.annotations.iter().map(|a| (a.subtype.clone(), a.contents.clone())).collect();
    v.sort();
    v
}

#[test]
fn text_marks_and_date() {
    let mut h = harness();
    assert!(h.state_mut().execute("sign.fill.check"));
    assert!(matches!(h.state().quick_tool, QuickTool::Fill(_)));
    click(&mut h, 50.0, 350.0);
    h.state_mut().execute("sign.fill.date");
    click(&mut h, 50.0, 300.0);
    h.state_mut().execute("sign.fill.text");
    click(&mut h, 50.0, 250.0);
    h.event(egui::Event::Text("Ada Lovelace".into()));
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    let v = items(&h);
    assert_eq!(v.len(), 3, "{v:?}");
    assert!(v.iter().any(|(t, c)| t == "FreeText" && c.as_deref() == Some("Ada Lovelace")));
    assert!(v.iter().any(|(t, c)| t == "FreeText" && c.as_deref().is_some_and(|c| c.matches('/').count() == 2)), "a date: {v:?}");
    assert!(v.iter().any(|(t, _)| t == "Stamp"));
}

#[test]
fn signing_draws_a_signature_once_and_places_it() {
    let mut h = harness();
    assert!(h.state_mut().execute("sign.fill.signature"));
    h.run_steps(2);
    assert_eq!(h.state().dialog, Some(Dialog::Signature), "no signature yet: the pad opens");
    h.get_all_by_label("Draw").last().unwrap().click();
    h.run_steps(2);
    let pad = h.get_by_label("Draw your signature below.").rect();
    let start = pos2(pad.left() + 40.0, pad.bottom() + 70.0);
    h.hover_at(start);
    h.run_steps(1);
    h.drag_at(start);
    h.run_steps(1);
    for k in 1..=8 {
        h.hover_at(start + egui::vec2(k as f32 * 30.0, if k % 2 == 0 { -20.0 } else { 20.0 }));
        h.run_steps(1);
    }
    h.drop_at(start + egui::vec2(240.0, 0.0));
    h.run_steps(2);
    h.get_by_label("Apply").click();
    h.run_steps(3);
    assert!(h.state().signature.is_some());
    assert_eq!(h.state().dialog, None);
    click(&mut h, 60.0, 100.0);
    assert!(items(&h).iter().any(|(t, _)| t == "Ink"));
    // The signature is remembered (persisted with the app's settings).
    let saved = h.state().persist();
    let mut again = PdfCraftApp::new();
    again.restore(&saved);
    assert!(again.signature.is_some());
}

#[test]
fn typed_signatures_and_initials() {
    let mut h = harness();
    h.state_mut().comment_prefs.author = "Grace Hopper".into();
    assert!(h.state_mut().execute("sign.fill.signature"));
    h.run_steps(2);
    // Type is the default, with the author's name filled in.
    h.get_by_label("Type your signature.");
    assert_eq!(h.state().signature_draft.text, "Grace Hopper");
    h.get_by_label("Apply").click();
    h.run_steps(3);
    assert_eq!(h.state().signature, Some(pdfcraft_ui_egui::fill_sign::SavedSig::Typed("Grace Hopper".into())));
    click(&mut h, 60.0, 100.0);
    assert!(items(&h).iter().any(|(t, _)| t == "Stamp"), "typed signatures are filled outlines");
    // Initials: their own pad (GH), then placed.
    assert!(h.state_mut().execute("sign.fill.initials"));
    h.run_steps(2);
    h.get_by_label("Create initials");
    assert_eq!(h.state().signature_draft.text, "GH");
    h.get_by_label("Apply").click();
    h.run_steps(3);
    click(&mut h, 60.0, 160.0);
    assert_eq!(items(&h).iter().filter(|(t, _)| t == "Stamp").count(), 2);
    // Both are remembered.
    let saved = h.state().persist();
    let mut again = PdfCraftApp::new();
    again.restore(&saved);
    assert_eq!(again.signature, h.state().signature);
    assert_eq!(again.initials, Some(pdfcraft_ui_egui::fill_sign::SavedSig::Typed("GH".into())));
}

#[test]
fn changing_saved_signatures_and_initials_preserves_placed_marks() {
    let mut h = harness();
    h.state_mut().signature = Some(SavedSig::Typed("Ada Lovelace".into()));
    h.state_mut().initials = Some(SavedSig::Typed("AL".into()));
    h.state_mut().execute("sign.fill.signature");
    h.run_steps(2);
    assert_eq!(h.state().dialog, None, "Sign still reuses the saved signature");
    click(&mut h, 40.0, 300.0);
    // Placing returns to the Select tool (so the signature can be moved or resized).
    assert_eq!(h.state().quick_tool, QuickTool::Select);
    h.state_mut().quick_tool = QuickTool::Fill(FillTool::Signature);
    h.run_steps(2);
    let original = format!("{:?}", h.state().session.get(h.state().views[0].id).unwrap().info.annotations);

    // The quick toolbar exposes replacement without changing the normal placement action.
    h.get_by_label("Fill & Sign").click();
    h.run_steps(2);
    h.get_by_label("Change signature").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog, Some(Dialog::Signature));
    assert_eq!(h.state().signature_draft.text, "Ada Lovelace");
    h.get_by_label("Type your signature.").click();
    h.run_steps(1);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.event(egui::Event::Text("Grace Hopper".into()));
    h.run_steps(2);
    h.get_by_label("Apply").click();
    h.run_steps(3);
    assert_eq!(h.state().signature, Some(SavedSig::Typed("Grace Hopper".into())));
    assert_eq!(h.state().initials, Some(SavedSig::Typed("AL".into())));
    assert_eq!(h.state().quick_tool, QuickTool::Fill(FillTool::Signature));
    assert_eq!(format!("{:?}", h.state().session.get(h.state().views[0].id).unwrap().info.annotations), original);
    click(&mut h, 40.0, 200.0);
    assert_eq!(items(&h).len(), 2);
    h.state_mut().quick_tool = QuickTool::Fill(FillTool::Signature);
    h.run_steps(2);

    h.get_by_label("Fill & Sign").click();
    h.run_steps(2);
    h.get_by_label("Change initials").click();
    h.run_steps(2);
    assert!(h.state().signature_draft.initials);
    assert_eq!(h.state().signature_draft.text, "AL");
    h.state_mut().signature_draft.text = "GH".into();
    h.get_by_label("Apply").click();
    h.run_steps(3);
    assert_eq!(h.state().quick_tool, QuickTool::Fill(FillTool::Initials));
    click(&mut h, 40.0, 100.0);
    assert_eq!(items(&h).len(), 3);
    let mut again = PdfCraftApp::new();
    again.restore(&h.state().persist());
    assert_eq!(again.signature, Some(SavedSig::Typed("Grace Hopper".into())));
    assert_eq!(again.initials, Some(SavedSig::Typed("GH".into())));
}

#[test]
fn changing_drawn_signatures_can_be_cancelled_or_switched_to_type() {
    let mut h = harness();
    let drawn = SavedSig::Drawn(vec![vec![[0.1, 0.1], [0.5, 0.2], [0.9, 0.1]]]);
    h.state_mut().signature = Some(drawn.clone());
    h.state_mut().initials = Some(SavedSig::Typed("AL".into()));
    h.state_mut().quick_tool = QuickTool::Fill(FillTool::Check);
    assert!(h.state_mut().execute("sign.fill.signature.change"));
    h.run_steps(2);
    assert!(h.state().signature_draft.drawing);
    assert_eq!(h.state().signature_draft.saved(), drawn);
    h.get_by_label("Clear").click();
    h.run_steps(2);
    assert!(h.state().signature_draft.strokes.is_empty());
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    assert_eq!(h.state().signature, Some(drawn.clone()));
    assert_eq!(h.state().quick_tool, QuickTool::Fill(FillTool::Check));

    assert!(h.state_mut().execute("sign.fill.signature.change"));
    h.run_steps(2);
    assert_eq!(h.state().signature_draft.saved(), drawn);
    h.get_all_by_label("Type").last().unwrap().click();
    h.run_steps(2);
    h.state_mut().signature_draft.text = "Grace Hopper".into();
    h.get_by_label("Apply").click();
    h.run_steps(3);
    assert_eq!(h.state().signature, Some(SavedSig::Typed("Grace Hopper".into())));
    assert_eq!(h.state().initials, Some(SavedSig::Typed("AL".into())));
}

#[test]
fn saved_signature_cards_remove_and_add_without_changing_the_document() {
    let mut h = harness();
    h.state_mut().signature = Some(SavedSig::Typed("Ada Lovelace".into()));
    h.state_mut().initials = Some(SavedSig::Typed("AL".into()));
    h.state_mut().execute("sign.fill.signature");
    click(&mut h, 40.0, 300.0);
    let original = format!("{:?}", h.state().session.get(h.state().views[0].id).unwrap().info.annotations);
    h.state_mut().left = pdfcraft_ui_egui::LeftPanel::Tool("fill_sign");
    h.state_mut().left_open = true;
    h.run_steps(3);
    h.get_by_label("Use signature").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog, None, "the preview reuses the saved value");
    h.get_by_label("Remove saved signature").click();
    h.run_steps(3);
    assert_eq!(h.state().signature, None);
    assert_eq!(h.state().initials, Some(SavedSig::Typed("AL".into())));
    let mut again = PdfCraftApp::new();
    again.restore(&h.state().persist());
    assert_eq!(again.signature, None, "removal survives restart");
    h.get_by_label("Add signature").click();
    h.run_steps(3);
    h.state_mut().signature_draft.text = "Grace Hopper".into();
    h.get_by_label("Apply").click();
    h.run_steps(3);
    assert_eq!(h.state().signature, Some(SavedSig::Typed("Grace Hopper".into())));
    h.get_by_label("Remove saved initials").click();
    h.run_steps(3);
    assert_eq!(h.state().initials, None);
    h.get_by_label("Add initials").click();
    h.run_steps(3);
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    assert_eq!(h.state().initials, None);
    assert_eq!(format!("{:?}", h.state().session.get(h.state().views[0].id).unwrap().info.annotations), original);
}

#[test]
fn long_typed_names_fit_the_placed_signature_and_keep_every_outline() {
    let text = "Alexandria Catherine Elizabeth Montgomery-Wellington";
    let sig = SavedSig::Typed(text.into());
    let pdfcraft_engine::Edit::AddAnnotation(a) = pdfcraft_ui_egui::fill_sign::place(0, [40.0, 200.0], &sig, false, "").unwrap() else {
        panic!("expected annotation");
    };
    let pdfcraft_engine::Shape::TypedSignature { rect, contours } = a.shape else { panic!("expected typed signature") };
    assert!((rect[2] - rect[0] - 150.0).abs() < 0.001, "long names shrink to fit: {rect:?}");
    assert_eq!(contours.len(), pdfcraft_engine::script_outline(text).contours.len());
    assert!(contours.iter().flatten().all(|p| p.iter().all(|v| (0.0..=1.0).contains(v))), "all ink stays inside the appearance bounds");
    let mut h = harness();
    h.state_mut().signature = Some(sig.clone());
    h.state_mut().execute("sign.fill.signature.change");
    h.run_steps(3);
    assert_eq!(h.state().signature_draft.text, text);
    h.get_by_label("Apply").click();
    h.run_steps(3);
    assert_eq!(h.state().signature, Some(sig));
    click(&mut h, 40.0, 200.0);
    let s = h.state();
    let a = &s.session.get(s.views[0].id).unwrap().info.annotations[0];
    assert!(a.rect[2] <= 190.001, "the full name fits on the page: {:?}", a.rect);
}

/// A PNG `w`×`h` px: black ink on a transparent background.
fn ink_png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_fn(w, h, |x, y| if (x + y) % 7 == 0 { image::Rgba([0, 0, 0, 255]) } else { image::Rgba([0, 0, 0, 0]) });
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    png
}

fn stamps(h: &Harness<'static, PdfCraftApp>) -> Vec<[f32; 4]> {
    let s = h.state();
    s.session.get(s.views[0].id).unwrap().info.annotations.iter().filter(|a| a.subtype == "Stamp").map(|a| a.rect).collect()
}

#[test]
fn uploading_pictures_as_signature_and_initials() {
    let mut h = harness();
    let dir = std::env::temp_dir().join(format!("pdfcraft-sig-upload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("my signature.png");
    std::fs::write(&file, ink_png(300, 100)).unwrap();

    assert!(h.state_mut().execute("sign.fill.signature"));
    h.run_steps(2);
    assert_eq!(h.state().dialog, Some(Dialog::Signature));
    h.get_all_by_label("Upload").last().unwrap().click();
    h.run_steps(2);
    assert!(h.state().signature_draft.uploading);
    h.get_by_label("No image chosen");
    // Apply waits for a picture.
    h.get_by_label("Apply").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog, Some(Dialog::Signature));
    // Choose image… asks for a file (answered by the test hook); the dialog and its draft stay.
    h.state_mut().save_override = Some(file.to_string_lossy().into_owned());
    h.get_by_label("Choose image…").click();
    h.run_steps(3);
    h.state_mut().save_override = None;
    assert_eq!(h.state().dialog, Some(Dialog::Signature));
    let img = h.state().signature_draft.image.clone().expect("the picture is loaded");
    assert_eq!((img.file.as_str(), img.size), ("my signature.png", (300, 100)));
    h.get_by_label("my signature.png");
    h.get_by_label("Apply").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog, None);
    assert_eq!(h.state().signature, Some(SavedSig::Image(img.clone())));
    assert_eq!(h.state().quick_tool, QuickTool::Fill(FillTool::Signature));

    // Placed like a drawn signature: left edge at the click, vertically centred, within 150×40 pt.
    click(&mut h, 60.0, 100.0);
    let placed = stamps(&h);
    assert_eq!(placed.len(), 1, "an image stamp: {:?}", items(&h));
    let r = placed[0];
    let (w, ht) = (r[2] - r[0], r[3] - r[1]);
    assert!((w - 120.0).abs() < 1.5 && (ht - 40.0).abs() < 1.5, "300×100 px fits 120×40 pt: {r:?}");
    assert!((r[0] - 60.0).abs() < 2.0 && ((r[1] + r[3]) / 2.0 - 100.0).abs() < 2.0, "anchored at the click: {r:?}");
    // The picture keeps its transparency (a soft mask), and the stamp undoes like any comment.
    let s = h.state();
    let bytes = s.session.save_bytes(s.views[0].id).unwrap();
    assert!(bytes.windows(6).any(|w| w == b"/SMask"), "PNG alpha is kept");
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    assert!(stamps(&h).is_empty());

    // Initials: a square picture, at most 24 pt tall.
    assert!(h.state_mut().execute("sign.fill.initials"));
    h.run_steps(2);
    h.get_by_label("Create initials");
    h.get_all_by_label("Upload").last().unwrap().click();
    h.run_steps(1);
    h.state_mut().use_signature_image("initials.png".into(), ink_png(80, 80));
    h.run_steps(2);
    h.get_by_label("Apply").click();
    h.run_steps(3);
    assert!(matches!(h.state().initials, Some(SavedSig::Image(ref i)) if i.size == (80, 80)));
    click(&mut h, 60.0, 200.0);
    let r = stamps(&h)[0];
    assert!(((r[2] - r[0]) - 24.0).abs() < 1.0 && ((r[3] - r[1]) - 24.0).abs() < 1.0, "{r:?}");

    // The saved previews show both, and both survive a restart.
    h.state_mut().left = pdfcraft_ui_egui::LeftPanel::Tool("fill_sign");
    h.state_mut().left_open = true;
    h.run_steps(3);
    h.get_by_label("Use signature");
    h.get_by_label("Use initials");
    let saved = h.state().persist();
    let mut again = PdfCraftApp::new();
    again.restore(&saved);
    assert_eq!(again.signature, h.state().signature);
    assert_eq!(again.initials, h.state().initials);

    // Changing an uploaded signature opens on Upload with the picture loaded.
    assert!(h.state_mut().execute("sign.fill.signature.change"));
    h.run_steps(2);
    assert!(h.state().signature_draft.uploading);
    assert_eq!(h.state().signature_draft.image, Some(img));
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn oversize_and_non_picture_files_are_refused() {
    let mut h = harness();
    assert!(h.state_mut().execute("sign.fill.signature"));
    h.run_steps(2);
    h.get_all_by_label("Upload").last().unwrap().click();
    h.run_steps(1);
    let mut big = ink_png(10, 10);
    big.resize(pdfcraft_ui_egui::fill_sign::MAX_SIGNATURE_IMAGE_BYTES + 1, 0);
    h.state_mut().use_signature_image("huge.png".into(), big);
    assert!(h.state().signature_draft.image.is_none());
    assert!(h.state().toast.as_ref().is_some_and(|(t, _)| t.contains("too large")), "{:?}", h.state().toast);
    for (name, bytes) in [("notes.png", b"hello, not a picture".to_vec()), ("doc.png", FIXTURE.to_vec()), ("cut.png", ink_png(40, 40)[..60].to_vec())]
    {
        h.state_mut().toast = None;
        h.state_mut().use_signature_image(name.into(), bytes);
        assert!(h.state().signature_draft.image.is_none(), "{name}");
        assert!(h.state().toast.as_ref().is_some_and(|(t, _)| t.contains(name)), "{name}: {:?}", h.state().toast);
    }
    h.run_steps(2);
    assert_eq!(h.state().dialog, Some(Dialog::Signature), "the dialog stays open to choose again");
}

#[test]
fn malformed_signature_settings_are_ignored() {
    use base64::Engine as _;
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    let good = b64(&ink_png(30, 10));
    let not_image = b64(b"plain text");
    let huge = b64(&vec![0u8; pdfcraft_ui_egui::fill_sign::MAX_SIGNATURE_IMAGE_BYTES + 1]);
    for bad in [
        r#"{"signature_image": "nonsense", "initials": 7}"#.to_string(),
        r#"{"signature_image": {"Image": {"file": "a.png", "data": "!!!", "size": [1, 1]}}}"#.to_string(),
        format!(
            r#"{{"signature_image": {{"Image": {{"file": "a.png", "data": "{not_image}", "size": [5, 5]}}}}, "initials": {{"Image": {{"file": "b.png", "data": "{huge}", "size": [1, 1]}}}}}}"#
        ),
        format!(r#"{{"signature_image": {{"Typed": "smuggled"}}, "initials": {{"Image": {{"file": "b.png", "data": "{good}"}}}}}}"#),
        r#"{"initials": {"Drawn": [[[1e999, 0]]]}}"#.to_string(),
    ] {
        let mut app = PdfCraftApp::new();
        app.restore(&bad);
        assert_eq!((app.signature.clone(), app.initials.clone()), (None, None), "{bad}");
    }
    // A stored size is not trusted: the picture's real size is used.
    let mut app = PdfCraftApp::new();
    app.restore(&format!(r#"{{"signature_image": {{"Image": {{"file": "a.png", "data": "{good}", "size": [9999, 1]}}}}}}"#));
    assert!(matches!(app.signature, Some(SavedSig::Image(ref i)) if i.size == (30, 10)), "{:?}", app.signature.is_some());
    // Settings written before pictures still load.
    let mut old = PdfCraftApp::new();
    old.restore(r#"{"signature_text": "Ada Lovelace", "initials": {"Typed": "AL"}}"#);
    assert_eq!(old.signature, Some(SavedSig::Typed("Ada Lovelace".into())));
    assert_eq!(old.initials, Some(SavedSig::Typed("AL".into())));
    let mut old = PdfCraftApp::new();
    old.restore(r#"{"signature": [[[0.1, 0.1], [0.5, 0.2]]], "initials": {"Drawn": [[[0.1, 0.1], [0.2, 0.2]]]}}"#);
    assert!(matches!(old.signature, Some(SavedSig::Drawn(_))) && matches!(old.initials, Some(SavedSig::Drawn(_))));
}

#[test]
fn placed_pictures_keep_their_aspect_ratio() {
    let img = |w, h| pdfcraft_ui_egui::fill_sign::SigImage { file: "s.png".into(), data: std::sync::Arc::new(ink_png(w, h)), size: (w, h) };
    for ((w, h), initials, want) in
        [((600, 100), false, (150.0, 25.0)), ((100, 100), false, (40.0, 40.0)), ((200, 100), true, (48.0, 24.0)), ((1000, 50), true, (75.0, 3.75))]
    {
        let sig = SavedSig::Image(img(w, h));
        let Some(pdfcraft_engine::Edit::AddCustomStamp { rect, .. }) = pdfcraft_ui_egui::fill_sign::place(0, [10.0, 100.0], &sig, initials, "")
        else {
            panic!("an image stamp")
        };
        assert!(((rect[2] - rect[0]) - want.0).abs() < 1e-6 && ((rect[3] - rect[1]) - want.1).abs() < 1e-6, "{w}×{h}: {rect:?}");
        assert!((rect[0] - 10.0).abs() < 1e-9 && ((rect[1] + rect[3]) / 2.0 - 100.0).abs() < 1e-9);
    }
    let zero = SavedSig::Image(pdfcraft_ui_egui::fill_sign::SigImage { size: (0, 5), ..img(1, 1) });
    assert!(pdfcraft_ui_egui::fill_sign::place(0, [10.0, 100.0], &zero, false, "").is_none());
}

fn drag(h: &mut Harness<'static, PdfCraftApp>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.run_steps(1);
    h.drag_at(from);
    h.run_steps(1);
    for k in 1..=4 {
        h.hover_at(from + (to - from) * (k as f32 / 4.0));
        h.run_steps(1);
    }
    h.drop_at(to);
    h.run_steps(3);
}

#[test]
fn placed_picture_signatures_move_and_resize_keeping_their_shape() {
    let mut h = harness();
    assert!(h.state_mut().execute("sign.fill.signature"));
    h.run_steps(2);
    h.get_all_by_label("Upload").last().unwrap().click();
    h.run_steps(1);
    h.state_mut().use_signature_image("sig.png".into(), ink_png(300, 100));
    h.run_steps(2);
    h.get_by_label("Apply").click();
    h.run_steps(3);

    // Placing comes back under the Select tool with the new signature selected.
    click(&mut h, 60.0, 100.0);
    assert_eq!(h.state().quick_tool, QuickTool::Select);
    let r = stamps(&h)[0];
    assert!(h.state().views[0].comments.selected.is_some(), "the placed signature is selected");

    // Drag its middle: it moves, same size.
    let mid = at(&h, (r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0);
    drag(&mut h, mid, mid + egui::vec2(60.0, -45.0));
    let m = stamps(&h)[0];
    assert_eq!(stamps(&h).len(), 1, "moved, not placed again");
    assert!(m[0] > r[0] + 20.0 && m[1] > r[1] + 15.0, "moved right and up: {r:?} -> {m:?}");
    assert!(((m[2] - m[0]) - (r[2] - r[0])).abs() < 0.5 && ((m[3] - m[1]) - (r[3] - r[1])).abs() < 0.5);

    // Drag the bottom-right corner out (mostly sideways): it grows and keeps its 3:1 shape,
    // with the top-left corner where it was.
    let corner = at(&h, m[2], m[1]);
    drag(&mut h, corner, corner + egui::vec2(90.0, 5.0));
    let g = stamps(&h)[0];
    let (w, ht) = (g[2] - g[0], g[3] - g[1]);
    assert!(w > (m[2] - m[0]) * 1.3, "grew: {m:?} -> {g:?}");
    assert!((w / ht - 3.0).abs() < 0.05, "kept 3:1: {g:?}");
    assert!((g[0] - m[0]).abs() < 0.5 && (g[3] - m[3]).abs() < 0.5, "top-left stays: {m:?} -> {g:?}");

    // And shrinks from the same corner; undo goes back one step at a time.
    let corner = at(&h, g[2], g[1]);
    drag(&mut h, corner, corner - egui::vec2(120.0, 0.0));
    let s = stamps(&h)[0];
    assert!(s[2] - s[0] < w && ((s[2] - s[0]) / (s[3] - s[1]) - 3.0).abs() < 0.05, "{s:?}");
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(2);
    let u = stamps(&h)[0];
    assert!((u[2] - g[2]).abs() < 0.5 && (u[1] - g[1]).abs() < 0.5, "{u:?} vs {g:?}");
}
