//! Canvas painting: camera scaling and relationship head masking.

use scene::document::ViewState;
use viewer::paint::TextCache;
use viewer::render_cache::RenderCache;

/// Every painted label at `zoom`, as (text, world position, world size):
/// screen values divided by the zoom, the camera at the origin.
fn painted(scene: &edn::Value, zoom: f64) -> Vec<(String, egui::Pos2, f32)> {
    let state = ViewState {
        scene: scene.clone(),
        revision: 1,
        zoom,
        ..ViewState::default()
    };
    let ctx = egui::Context::default();
    let (mut cache, mut text) = (RenderCache::default(), TextCache::default());
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(2000.0, 2000.0));
        let painter = ui.painter_at(screen);
        viewer::paint::paint(
            ui.ctx(),
            &painter,
            &state,
            screen,
            None,
            None,
            None,
            false,
            &mut cache,
            &mut text,
        );
    });
    out.textures_delta.clear();
    let z = zoom as f32;
    let mut labels: Vec<_> = out
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(t) => Some((
                t.galley.text().to_owned(),
                (t.pos.to_vec2() / z).to_pos2(),
                t.galley.job.sections[0].format.font_id.size / z,
            )),
            _ => None,
        })
        .collect();
    labels.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.y.total_cmp(&b.1.y)));
    labels
}

#[test]
fn dimmed_closed_heads_mask_the_shaft_with_opaque_interiors() {
    for (kind, head, fill) in [
        (
            "inheritance",
            "triangle",
            egui::Color32::from_rgb(22, 28, 32),
        ),
        (
            "implements",
            "triangle",
            egui::Color32::from_rgb(22, 28, 32),
        ),
        (
            "aggregation",
            "diamond",
            egui::Color32::from_rgb(22, 28, 32),
        ),
        (
            "composition",
            "diamond-fill",
            egui::Color32::from_rgb(236, 236, 228),
        ),
    ] {
        let doc = scene::ir::read_document(&format!(
            r#"{{:hierarchical true
                :classes [{{:id :a :name "A"}} {{:id :b :name "B"}} {{:id :c :name "C"}}]
                :edges [{{:from :a :to :b :kind :{kind}}}]}}"#
        ))
        .unwrap();
        let state = ViewState {
            scene: scene::document::compile_view(&doc, &[], &[], &edn::Value::Map(vec![])),
            selected: Some(edn::parse("{:kind :class :id :c}").unwrap()),
            revision: 1,
            zoom: 1.0,
            ..ViewState::default()
        };
        // The exported frame and the native renderer consume the same view.
        let frame = frame::frame::build(&state, 2000.0, 2000.0, None, None).to_json();
        let edge = frame["edges"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["kind"] == "head")
            .unwrap();
        assert_eq!(edge["head"], head, "{kind}");
        assert_eq!(edge["dimmed"], true, "unrelated edge must dim");
        assert_eq!(
            edge["fill"]["role"],
            if head == "diamond-fill" {
                "ink"
            } else {
                "canvas"
            }
        );
        let tip = egui::pos2(
            edge["tip"][0].as_f64().unwrap() as f32,
            edge["tip"][1].as_f64().unwrap() as f32,
        );
        let ctx = egui::Context::default();
        let (mut cache, mut text) = (RenderCache::default(), TextCache::default());
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(2000.0, 2000.0));
            viewer::paint::paint(
                ui.ctx(),
                &ui.painter_at(screen),
                &state,
                screen,
                None,
                None,
                None,
                false,
                &mut cache,
                &mut text,
            );
        });
        out.textures_delta.clear();
        let paths: Vec<_> = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Path(p) => Some(p),
                _ => None,
            })
            .collect();
        let shaft = paths
            .iter()
            .position(|p| !p.closed && p.points.last() == Some(&tip))
            .expect("prepared scene keeps the shaft running to the head tip");
        let mask = paths
            .iter()
            .position(|p| p.closed && p.points.contains(&tip))
            .expect("closed arrowhead must be painted");
        assert!(shaft < mask, "{kind}: the head must cover the shaft");
        assert_eq!(
            paths[mask].fill, fill,
            "{kind}: opaque interior masks the shaft even when dimmed"
        );
        assert_eq!(
            paths[mask].stroke.color,
            egui::epaint::ColorMode::Solid(viewer::color::ink(viewer::theme::Role::Relation, true)),
            "{kind}: only the outline dims"
        );
    }
}

#[test]
fn every_canvas_label_scales_and_moves_with_zoom() {
    // One of each world-space label: package title, class name and member,
    // level, α/I marker, C/M letters and a port.
    let scene = edn::parse(
        "{:packages [{:id :p :label \"Pkg\" :rect {:x 20 :y 20 :w 400 :h 300}}] \
          :classes [{:id :a :name \"Alpha\" :stereotype :abstract :level 2 \
                     :crap {:mu 2} :killed 1 :survived 0 \
                     :rect {:x 60 :y 70 :w 160 :h 72} \
                     :lines [{:kind :name :text \"Alpha\"} {:kind :rule} {:kind :op :text \"run\"}] \
                     :out-ports [{:id :common :name \"Common\" :rect {:x 80 :y 200 :w 80 :h 24}}]}] \
          :edges []}",
    )
    .unwrap();
    let near = painted(&scene, 1.0);
    let far = painted(&scene, 3.0);
    let names: Vec<&str> = near.iter().map(|(t, ..)| t.as_str()).collect();
    for label in ["Pkg", "Alpha", "run", "2", "α", "C", "M", "Common"] {
        assert!(names.contains(&label), "{label} not painted: {names:?}");
    }
    assert_eq!(near.len(), far.len(), "{near:?}\n{far:?}");
    for ((text, at, size), (_, far_at, far_size)) in near.iter().zip(&far) {
        // Whole-pixel font sizes round per zoom; anything fixed in screen
        // pixels is off by its own size times (1 - 1/3).
        assert!(
            (size - far_size).abs() <= 0.5,
            "{text}: size {size} vs {far_size} at 3x"
        );
        assert!(
            at.distance(*far_at) <= 1.5,
            "{text}: at {at:?} vs {far_at:?} at 3x"
        );
    }
}
