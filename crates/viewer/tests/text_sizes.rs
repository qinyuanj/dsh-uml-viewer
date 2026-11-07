//! Canvas text draws at one size (the 16px canvas size) in every view of
//! this repository's own diagram, under the pinned Sarasa face and the real
//! fit the painter applies. A clone without the face provisions it through
//! the installer's font step first: another face has other advances, so it
//! would measure nothing. That provisioning goes under Cargo's target, never
//! into the source tree: a mutation worker overlay leaves `.uml-viewer/` out
//! and treats any new file in it as a poisoned worker.

use std::collections::BTreeMap;
use std::path::Path;

use edn::Value;
use edn::record::{get, is_true, local_name};
use viewer::commands::{self, Command};
use viewer::paint::{self, TextCache};
use viewer::theme::fonts;

#[test]
fn every_view_draws_its_canvas_text_at_the_canvas_size() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // Verifies the pinned hash and downloads the face only when it is
    // missing or wrong.
    let provisioned = root.join(".uml-viewer/fonts");
    let fonts_dir = if provisioned.join(viewer::fonts::SARASA).is_file() {
        provisioned
    } else {
        Path::new(env!("CARGO_TARGET_TMPDIR")).join("uml-viewer/fonts")
    };
    // The entries' Python: `UML_PYTHON_EXECUTABLE`, else `python3`; Windows
    // has no `python3` on PATH, its interpreter is `python`.
    let python = std::env::var_os("UML_PYTHON_EXECUTABLE")
        .unwrap_or_else(|| (if cfg!(windows) { "python" } else { "python3" }).into());
    let step = std::process::Command::new(&python)
        .arg("-I")
        .arg(root.join("scripts/uml_posix.py"))
        .arg("font")
        .arg(&fonts_dir)
        .output()
        .expect("Python runs the installer's font step");
    assert!(
        step.status.success(),
        "provisioning Sarasa failed: {}",
        String::from_utf8_lossy(&step.stderr)
    );
    let face = std::fs::read(fonts_dir.join(viewer::fonts::SARASA)).unwrap();
    let ctx = egui::Context::default();
    ctx.set_fonts(viewer::fonts::definitions(egui::FontData::from_owned(face)));
    ctx.begin_pass(egui::RawInput::default());
    // The layout sizes boxes for Sarasa's half-width Latin at the canvas
    // size; the painter must measure the same advance.
    let advance = paint::measure(&ctx, fonts::CANVAS, "x");
    assert_eq!(scene::CHAR_W, advance, "Sarasa advance at the canvas size");
    let text = std::fs::read_to_string(root.join("examples/uml-viewer.edn")).unwrap();
    let doc = scene::ir::read_document(&text).unwrap();
    let (snapshots, _) = viewer::metrics::snapshot(&root);
    let mut cache = TextCache::default();
    let mut off: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut views = 0;
    let mut queue: Vec<Vec<Value>> = vec![vec![]];
    while let Some(focus) = queue.pop() {
        let view = scene::document::compile_view(&doc, &snapshots, &focus, &Value::Map(vec![]));
        views += 1;
        for c in match get(&view, "classes") {
            Some(Value::Vector(cs)) => cs.as_slice(),
            _ => &[],
        } {
            if is_true(c, "drill?") {
                let id = local_name(get(c, "id").unwrap()).unwrap_or_default();
                queue.push(id.split('.').map(Value::keyword).collect());
            }
        }
        for command in commands::commands(&view, None, None) {
            let (role, label, size, max_w, bold, italic) = match &command {
                Command::Text {
                    role,
                    text,
                    size,
                    max_w,
                    bold,
                    italic,
                    ..
                } => (role.as_str(), text, *size, *max_w, *bold, *italic),
                Command::Package {
                    text, size, max_w, ..
                } => ("package", text, *size, *max_w, false, true),
                _ => continue,
            };
            if label.is_empty() {
                continue;
            }
            let px = paint::fit_px(
                &ctx,
                &mut cache,
                size as f32,
                label,
                max_w as f32,
                bold,
                italic,
            );
            if px != fonts::CANVAS {
                off.entry(role.to_owned())
                    .or_default()
                    .push(format!("{label} at {px}px in {focus:?}"));
            }
        }
    }
    ctx.end_pass().textures_delta.clear();
    assert!(views > 1, "the diagram has no drillable views");
    assert!(off.is_empty(), "canvas text off the canvas size: {off:#?}");
}
