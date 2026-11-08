//! Shared inspector text and semantic colors, without a window or renderer.

#[test]
fn package_summary_preserves_quality_counts_and_missing_data() {
    let scene = edn::parse(
        r#"{:packages [
        {:id :core :label "Core" :crap {:mu 4} :killed 5 :survived 0}
        {:id :empty}]
        :classes [{:id :a :package :core} {:id :b :package :core} {:id :c :package :other}]}"#,
    )
    .unwrap();
    assert_eq!(
        inspector::package_summary(&scene, &edn::Value::keyword("core")),
        vec![
            label("Core", Role::Ink),
            segments(&["Crap μ 4.0", "max 4.0", "σ 0.0"], Role::Accent),
            segments(&["5 killed / 0 survived"], Role::Muted),
            label("类数：2", Role::Muted),
        ]
    );
    assert_eq!(
        inspector::package_summary(&scene, &edn::Value::keyword("empty")),
        vec![
            label("empty", Role::Ink),
            label("类数：0", Role::Muted),
            label("无数据", Role::NoData),
        ]
    );
    assert!(inspector::package_summary(&scene, &edn::Value::keyword("absent")).is_empty());
}

use frame::detail::Model;
use frame::inspector::{self, SummaryLine};
use frame::theme::Role;

fn label(text: &str, color: Role) -> SummaryLine {
    SummaryLine::Label {
        text: text.into(),
        color,
    }
}

fn segments(parts: &[&str], color: Role) -> SummaryLine {
    SummaryLine::Segments {
        parts: parts.iter().map(|s| (*s).into()).collect(),
        color,
    }
}

#[test]
fn class_summary_preserves_identity_quality_members_and_no_data() {
    let class = edn::parse(
        r#"{:name "Worker" :package "Core" :level 2 :stereotype :abstract
        :crap {:mu 4 :max 8 :sigma 1.5} :killed 7 :survived 2 :uncovered 3
        :lines [{:kind :name :text "Worker"} {:kind :field :text "+ id"}
                {:kind :rule} {:kind :op :text "+ run()"}]}"#,
    )
    .unwrap();
    assert_eq!(
        inspector::class_summary(&Model::of(class)),
        vec![
            label("Worker", Role::Ink),
            label("包 Core", Role::Muted),
            label("层级 2", Role::Muted),
            label("«abstract»", Role::Muted),
            segments(&["Crap μ 4.0", "max 8.0", "σ 1.5"], Role::Accent),
            segments(&["7 killed / 2 survived", "3 uncovered"], Role::Muted),
            label("+ id", Role::Ink),
            label("+ run()", Role::Ink),
        ]
    );
    let missing = edn::parse(r#"{:name "Missing" :survived 0 :uncovered 0}"#).unwrap();
    assert_eq!(
        inspector::class_summary(&Model::of(missing)),
        vec![
            label("Missing", Role::Ink),
            segments(&["0 killed / 0 survived"], Role::Muted),
            label("无数据", Role::NoData),
        ]
    );
    let numeric = edn::parse(r#"{:name "Numeric" :crap 3.25}"#).unwrap();
    assert_eq!(
        inspector::class_summary(&Model::of(numeric)),
        vec![label("Numeric", Role::Ink), label("Crap 3.2", Role::Accent),]
    );
}
