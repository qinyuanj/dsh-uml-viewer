//! Scene frame goldens (#16): `uml-viewer scene --frame` on the shop
//! fixture, byte for byte. The goldens under `fixtures/frames/` are the
//! frame contract the viewer panel consumes (`docs/scene-frame.md`); a
//! deliberate change regenerates them with `UPDATE_FRAME_GOLDENS=1` and the
//! diff is reviewed like the schema change it is.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Run the `scene` role in process and return the frame it wrote.
fn frame(args: &[&str]) -> String {
    let shop = fixtures().join("shop");
    let out = std::env::temp_dir().join(format!(
        "uml-frame-{}-{}.json",
        std::process::id(),
        args.join("_")
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect::<String>()
    ));
    let mut argv: Vec<OsString> = vec![
        "--frame".into(),
        "--ir".into(),
        shop.join("shop.ir.edn").into(),
        "--metrics-root".into(),
        shop.clone().into(),
        "--output".into(),
        out.clone().into(),
    ];
    argv.extend(args.iter().map(OsString::from));
    assert_eq!(ExitCode::SUCCESS, frame::run_scene(&argv), "{args:?}");
    let text = std::fs::read_to_string(&out).unwrap();
    let _ = std::fs::remove_file(&out);
    text
}

fn golden(name: &str, args: &[&str]) {
    let actual = frame(args);
    let path = fixtures().join("frames").join(format!("{name}.frame.json"));
    if std::env::var_os("UPDATE_FRAME_GOLDENS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &actual).unwrap();
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("missing golden {}: {e}", path.display()))
        .replace("\r\n", "\n");
    assert!(
        actual == expected,
        "{name}: frame differs from {}; rerun with UPDATE_FRAME_GOLDENS=1 and review the diff",
        path.display()
    );
}

#[test]
fn root_view_fitted_to_the_default_canvas() {
    golden("root", &[]);
}

#[test]
fn a_selected_class_lights_its_dependency_chain_and_dims_the_rest() {
    golden(
        "core-selected",
        &[
            "--focus",
            "[:core]",
            "--select",
            "{:kind :class :id :core.order}",
        ],
    );
}

#[test]
fn a_hovered_class_warms_its_edges_and_names_itself_at_overview_zoom() {
    golden(
        "root-hover-frames",
        &[
            "--zoom",
            "0.3",
            "--camera",
            "-200,-100",
            "--hover",
            "{:kind :class :id :core}",
            "--pointer",
            "300,240",
        ],
    );
}

#[test]
fn decluttered_triangles_at_block_zoom() {
    golden(
        "core-triangles-blocks",
        &[
            "--focus",
            "[:core]",
            "--extra",
            "{:declutter :triangles}",
            "--zoom",
            "0.12",
            "--camera",
            "0,0",
        ],
    );
}

/// The panel primitives at a reading zoom under 1 (dsh-uml-viewer#24): a
/// selected (marked) dependency port, its sibling ports and arrows, α/I,
/// level numbers and stereotypes, all scaled and rounded like the window.
#[test]
fn a_selected_port_marks_itself_at_reading_zoom() {
    golden(
        "core-port-selected",
        &[
            "--focus",
            "[:core]",
            "--select",
            "{:kind :port :id :api :parent :core.order :dir :in}",
            "--zoom",
            "0.6",
            "--camera",
            "0,40",
        ],
    );
}

#[test]
fn a_proposal_names_itself_on_the_banner() {
    golden(
        "proposal",
        &["--extra", "{:proposal true :proposal-id :p-shop}"],
    );
}
