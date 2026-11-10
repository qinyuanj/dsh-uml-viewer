//! The UI string tables (port of `love/uml/application/strings.lua`). Terms
//! follow GLOSSARY.md; metric and tool names (CRAP, CC, Cov, mutation,
//! killed, survived, uncovered, IR) and key names stay English. EDN keywords,
//! mail commands, stderr and the window title are not UI copy.
//!
//! The text the resident lays out for the panel (#44, ADR 0015) — the class
//! card, the class box's 无数据 and the source error — comes in Chinese and
//! English through [`Lang::text`]; any other language falls back to English.
//! The inspector's Chinese-only constants left with it (#47). The canvas
//! chrome (title, proposal banner, return line, status, hint) left with #46, whose text lives in the panel's locale files.
//! The window's glyph-coverage probe and F3 performance line left with the
//! standalone window (dsh-uml-viewer#30).

/// The language the resident lays text out in. The panel passes the DSH
/// language; only Chinese and English are shipped and the rest read English.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    Zh,
    #[default]
    En,
}

impl Lang {
    /// The language for a locale tag: any Chinese variant (`zh`, `zh-CN`,
    /// `zh_TW`) is Chinese, everything else English.
    pub fn from_tag(tag: &str) -> Lang {
        let primary = tag.split(['-', '_']).next().unwrap_or_default();
        if primary.eq_ignore_ascii_case("zh") {
            Lang::Zh
        } else {
            Lang::En
        }
    }

    /// This language's string table.
    pub fn text(self) -> &'static Text {
        match self {
            Lang::Zh => &ZH,
            Lang::En => &EN,
        }
    }
}

/// The text the resident lays out in one language.
#[derive(Debug)]
pub struct Text {
    /// The class box marker for a class without CRAP or mutation data.
    pub no_data: &'static str,
    /// The source window's error; `%s` is the member or module asked for.
    pub source_not_opened: &'static str,
    // Class card.
    pub level: &'static str,
    pub package: &'static str,
    pub fields: &'static str,
    pub relationships: &'static str,
    pub no_mutation_sites: &'static str,
    pub member: &'static str,
    pub column_tips: [(&'static str, &'static str); 6],
    /// Relationship phrases: kind, outgoing, incoming.
    pub phrases: [(&'static str, &'static str, &'static str); 6],
    pub phrase_fallback: (&'static str, &'static str),
    /// The legend's section titles and row labels (#50); the rows' swatches
    /// are semantic roles, not words, and live in `legend.rs`.
    pub legend: LegendText,
}

/// The legend's text (#50): the section titles and the label of each row, in
/// the resident's language. The row identity (`class-box`, `relation`, …) and
/// the color each swatch shows are model, not text, and stay in `legend.rs`.
#[derive(Debug)]
pub struct LegendText {
    /// Section titles.
    pub shapes: &'static str,
    pub quality: &'static str,
    pub relations: &'static str,
    /// The shapes section: what the diagram's boxes are.
    pub class_box: &'static str,
    pub component_box: &'static str,
    pub foreign_package: &'static str,
    /// The quality section: 质量色, 无数据 and the selection.
    pub quality_ramp: &'static str,
    pub no_data: &'static str,
    pub selected: &'static str,
    /// The relations section: 关系连线 and the interactive states.
    pub relation: &'static str,
    pub violation: &'static str,
    pub relation_hover: &'static str,
    pub dependency_chain: &'static str,
    pub dimmed: &'static str,
}

// The metric names read the same in every language.
pub const CRAP_LINE: &str = "Crap μ %s max %s σ %s";
pub const CRAP_SINGLE: &str = "Crap %s";
pub const GROUP_CRAP: &str = "--crap--";
pub const GROUP_MUTATION: &str = "--mutation--";
pub const COLUMNS: [(&str, &str); 6] = [
    ("crap", "Crap"),
    ("cc", "CC"),
    ("cov", "Cov"),
    ("killed", "killed"),
    ("survived", "survived"),
    ("uncovered", "uncovered"),
];

pub const ZH: Text = Text {
    no_data: "无数据",
    source_not_opened: "未能打开源码：%s",
    level: "层级 ",
    package: "包 ",
    fields: "字段",
    relationships: "关系",
    no_mutation_sites: "---无 mutation 位点---",
    member: "成员",
    column_tips: [
        ("crap", "CRAP 风险分数：圈复杂度与覆盖率的综合，越低越好"),
        ("cc", "圈复杂度"),
        ("cov", "行覆盖率"),
        ("killed", "被测试杀死的变异体数"),
        ("survived", "测试未能杀死的存活变异体数"),
        ("uncovered", "没有测试覆盖的变异体数"),
    ],
    phrases: [
        ("inheritance", "继承", "被继承"),
        ("implements", "实现", "被实现"),
        ("association", "关联", "被关联"),
        ("dependency", "依赖", "被依赖"),
        ("aggregation", "聚合", "被聚合"),
        ("composition", "组合", "被组合"),
    ],
    phrase_fallback: ("指向", "来自"),
    legend: LegendText {
        shapes: "形状",
        quality: "质量色",
        relations: "关系",
        class_box: "类框：填充与框线取自质量色",
        component_box: "组件框：可下钻的组件，质量取成员的汇总",
        foreign_package: "外来包椭圆：只有依赖关系，没有质量数据",
        quality_ramp: "质量色：红到绿，CRAP 越低越好",
        no_data: "无数据：缺少 CRAP 或 mutation 数据",
        selected: "选中：框线改用选中色",
        relation: "关系连线：灰绿实线，头部形状表示关系种类",
        violation: "违规关系色：红色，依赖方向违反架构规则",
        relation_hover: "关系悬停高亮：悬停的关系变白",
        dependency_chain: "依赖链高亮：选中类的依赖链变蓝",
        dimmed: "关系淡化：不在依赖链上的元素变淡",
    },
};

pub const EN: Text = Text {
    no_data: "No data",
    source_not_opened: "Could not open the source: %s",
    level: "Level ",
    package: "Package ",
    fields: "Fields",
    relationships: "Relationships",
    no_mutation_sites: "---no mutation sites---",
    member: "Member",
    column_tips: [
        (
            "crap",
            "CRAP risk score: cyclomatic complexity combined with coverage; lower is better",
        ),
        ("cc", "Cyclomatic complexity"),
        ("cov", "Line coverage"),
        ("killed", "Mutants the tests killed"),
        ("survived", "Mutants that survived the tests"),
        ("uncovered", "Mutants no test covers"),
    ],
    phrases: [
        ("inheritance", "inherits", "inherited by"),
        ("implements", "implements", "implemented by"),
        ("association", "associates", "associated by"),
        ("dependency", "depends on", "depended on by"),
        ("aggregation", "aggregates", "aggregated by"),
        ("composition", "composes", "composed by"),
    ],
    phrase_fallback: ("to", "from"),
    legend: LegendText {
        shapes: "Shapes",
        quality: "Quality colors",
        relations: "Relations",
        class_box: "Class box: its fill and frame use the quality colors",
        component_box: "Component box: a component you can drill into; quality pooled from its members",
        foreign_package: "Foreign package: an ellipse with relations only, no quality data",
        quality_ramp: "Quality color: red to green; lower CRAP is better",
        no_data: "No data: CRAP or mutation data is missing",
        selected: "Selected: the outline takes the selection color",
        relation: "Relation: a grey-green solid line; the head shape names its kind",
        violation: "Violating relation: red, the dependency points outward",
        relation_hover: "Relation hover: the hovered relation turns white",
        dependency_chain: "Dependency chain: the selected class's chain turns blue",
        dimmed: "Dimmed: elements off the chain fade",
    },
};

/// Non-table glyphs the canvas and cards draw: stereotype guillemets, the
/// abstract marker, crap statistics and arrows.
pub const EXTRA_GLYPHS: &str = "«»αμσ←→—…";
