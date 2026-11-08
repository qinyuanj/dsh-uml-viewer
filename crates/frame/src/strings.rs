//! The single Chinese UI string table (port of
//! `love/uml/application/strings.lua`). Terms follow GLOSSARY.md; metric and
//! tool names (CRAP, CC, Cov, mutation, killed, survived, uncovered, IR) and
//! key names stay English. EDN keywords, mail commands, stderr and the window
//! title are not UI copy. Every string here is part of [`glyph_text`], the
//! probe the fonts driver checks for glyph coverage. Entries the inspector,
//! class card and menus use land with #141/#142; they live here already so
//! the font coverage stays that of the LOVE viewer.

pub const REAL_DIAGRAM: &str = "源码图";
pub const INSPECTOR: &str = "检查器";
pub const SEARCH: &str = "搜索类名或命名空间";
pub const SEARCH_OPEN: &str = "打开所选结果 (Enter)";
pub const SEARCH_EMPTY: &str = "没有匹配的类";
pub const TREE_AND_PROPOSALS: &str = "命名空间树 / 提案";
pub const NEW_PROPOSAL: &str = "新建提案";
pub const DECLUTTER: &str = "精简视图";
/// Declutter control labels by current mode (`declutter_modes`).
pub const DECLUTTER_MODES: [(&str, &str); 5] = [
    ("full", "精简：无"),
    ("arrows", "精简：箭头"),
    ("triangles", "移除箭头"),
    ("elements", "精简：元素"),
    ("classes", "精简：类"),
];
pub const RENAME: &str = "重命名";
pub const DELETE: &str = "删除";
pub const PROPOSAL_NAME: &str = "提案名称";
pub const RENAME_PROPOSAL: &str = "重命名提案";
pub const CANCEL: &str = "取消";
pub const P_HINT: &str = "P：返回命名空间树";
pub const PROPOSALS_LABEL: &str = "提案";
pub const CLASS_COUNT: &str = "类数：";
pub const PACKAGE_CRAP: &str = "Crap μ %s max %s σ %s";
pub const PACKAGE_MUTATION: &str = "%d killed / %d survived";
pub const PACKAGE_UNCOVERED: &str = "   %d uncovered";
pub const NO_DATA: &str = "无数据";
pub const SELECT_CLASS: &str = "单击类或命名空间查看摘要；双击打开详情卡片。";
pub const REGEN: &str = "重新生成 IR";
pub const SOURCE_NOT_OPENED: &str = "未能打开源码：%s";
pub const BACK: &str = "< 返回";
pub const PROPOSAL_BANNER: &str = "提案（未在源码中实例化）";
pub const PERFORMANCE: &str = "性能：%d FPS · 帧耗时 %.2f ms";
pub const HELP: &str = "单击类或命名空间查看摘要。
双击类打开详情卡片。
双击命名空间进入下一层。
Esc（或 ←）返回上一层。
滚轮平移；Shift+滚轮横向平移。
Ctrl+滚轮或 Ctrl+/− 以光标为中心缩放。
空白处左键拖拽平移；Ctrl+0 适配全图。
R 重新加载；F3 切换性能显示。
单击上方列表中的图或提案即可切换。";
/// Element menu labels; the mail operations themselves stay English.
pub const MENU: [(&str, &str); 4] = [
    ("refresh-crap", "刷新 CRAP"),
    ("refresh-mutate", "刷新 mutation"),
    ("refresh-mutate-all", "全量 mutation"),
    ("omit", "隐藏"),
];
// Class card.
pub const LEVEL: &str = "层级 ";
pub const PACKAGE: &str = "包 ";
pub const FIELDS: &str = "字段";
pub const RELATIONSHIPS: &str = "关系";
pub const NO_MUTATION_SITES: &str = "---无 mutation 位点---";
pub const MEMBER: &str = "成员";
pub const EMPTY_DETAIL: &str = "选择成员查看质量数据。";
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
pub const COLUMN_TIPS: [(&str, &str); 6] = [
    ("crap", "CRAP 风险分数：圈复杂度与覆盖率的综合，越低越好"),
    ("cc", "圈复杂度"),
    ("cov", "行覆盖率"),
    ("killed", "被测试杀死的变异体数"),
    ("survived", "测试未能杀死的存活变异体数"),
    ("uncovered", "没有测试覆盖的变异体数"),
];
/// Relationship phrases: outgoing, incoming.
pub const PHRASES: [(&str, &str, &str); 6] = [
    ("inheritance", "继承", "被继承"),
    ("implements", "实现", "被实现"),
    ("association", "关联", "被关联"),
    ("dependency", "依赖", "被依赖"),
    ("aggregation", "聚合", "被聚合"),
    ("composition", "组合", "被组合"),
];
pub const PHRASE_FALLBACK: (&str, &str) = ("指向", "来自");
/// Non-table glyphs the canvas and cards draw: stereotype guillemets, the
/// abstract marker, crap statistics and arrows.
pub const EXTRA_GLYPHS: &str = "«»αμσ←→—…";

/// The declutter control label for a mode name; unknown modes read as the
/// generic 精简视图.
pub fn declutter_label(mode: &str) -> &'static str {
    DECLUTTER_MODES
        .iter()
        .find(|(m, _)| *m == mode)
        .map_or(DECLUTTER, |(_, label)| label)
}

/// F3 overlay; matches `性能：%d FPS · 帧耗时 %.2f ms`.
pub fn performance(fps: u32, frame_ms: f64) -> String {
    format!("性能：{fps} FPS · 帧耗时 {frame_ms:.2} ms")
}
/// Empty canvas while a `--backend herdr` launch waits for mail (#139; the
/// mailbox itself is #142).
pub const WAITING_DISPLAY: &str = "等待 :display 命令加载图示…";
/// The diagram file named on the command line does not exist.
pub fn missing_file(path: &str) -> String {
    format!("找不到 IR 文件：{path}")
}

/// Every UI string concatenated: the glyph-coverage probe.
pub fn glyph_text() -> String {
    let mut out: Vec<&str> = vec![
        REAL_DIAGRAM,
        INSPECTOR,
        SEARCH,
        SEARCH_OPEN,
        SEARCH_EMPTY,
        TREE_AND_PROPOSALS,
        NEW_PROPOSAL,
        DECLUTTER,
        RENAME,
        DELETE,
        PROPOSAL_NAME,
        RENAME_PROPOSAL,
        CANCEL,
        P_HINT,
        PROPOSALS_LABEL,
        CLASS_COUNT,
        PACKAGE_CRAP,
        PACKAGE_MUTATION,
        PACKAGE_UNCOVERED,
        NO_DATA,
        SELECT_CLASS,
        REGEN,
        SOURCE_NOT_OPENED,
        BACK,
        PROPOSAL_BANNER,
        PERFORMANCE,
        HELP,
        LEVEL,
        PACKAGE,
        FIELDS,
        RELATIONSHIPS,
        NO_MUTATION_SITES,
        MEMBER,
        EMPTY_DETAIL,
        CRAP_LINE,
        CRAP_SINGLE,
        GROUP_CRAP,
        GROUP_MUTATION,
        PHRASE_FALLBACK.0,
        PHRASE_FALLBACK.1,
        EXTRA_GLYPHS,
        WAITING_DISPLAY,
        "找不到 IR 文件：",
    ];
    for (_, label) in DECLUTTER_MODES
        .iter()
        .chain(&MENU)
        .chain(&COLUMNS)
        .chain(&COLUMN_TIPS)
    {
        out.push(label);
    }
    for (_, outgoing, incoming) in &PHRASES {
        out.push(outgoing);
        out.push(incoming);
    }
    out.concat()
}
