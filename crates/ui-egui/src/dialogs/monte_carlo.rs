//! Thin editor for the engine's generic Monte Carlo command.
use super::ref_edit::{RefLabel, ref_edit};
use super::{Dialog, Json, Map, SheetApp, json};

/// Reads the configuration only from a recognizable Monte Carlo report on the active sheet.
/// Invalid or edited report data is left to the user rather than silently run as defaults.
pub(super) fn saved_configuration(sheet: &gridcraft_engine::model::Sheet) -> Option<Result<Json, &'static str>> {
    use gridcraft_engine::core::{CellRef, Value};
    if !matches!(sheet.value(CellRef::new(0, 0)), Value::Text(ref t) if t.as_ref() == "蒙特卡洛模拟")
        || !matches!(sheet.value(CellRef::new(3, 0)), Value::Text(ref t) if t.as_ref() == "复用模拟配置")
    {
        return None;
    }
    let Value::Text(text) = sheet.value(CellRef::new(3, 1)) else { return Some(Err("保存的模拟配置丢失")) };
    Some(serde_json::from_str::<Json>(&text).ok().filter(Json::is_object).ok_or("保存的模拟配置无效"))
}

/// Converts the stored output list to the dialog's line editor while retaining other settings.
pub(super) fn load_configuration(dialog: &mut Dialog, config: Json) -> Result<(), &'static str> {
    let mut values = config.as_object().cloned().ok_or("保存的模拟配置无效")?;
    let outputs = values
        .get("outputs")
        .and_then(Json::as_array)
        .filter(|outputs| !outputs.is_empty() && outputs.len() <= 64)
        .ok_or("保存的模拟输出无效")?;
    let lines = outputs.iter().map(Json::as_str).collect::<Option<Vec<_>>>().ok_or("保存的模拟输出无效")?;
    if !values
        .get("inputs")
        .and_then(Json::as_array)
        .is_some_and(|inputs| !inputs.is_empty() && inputs.len() <= 32 && inputs.iter().all(Json::is_object))
    {
        return Err("保存的模拟输入无效");
    }
    let outputs_text = lines.join("\n");
    values.insert("outputs".into(), json!(outputs_text));
    values.remove("newSheet");
    dialog.values = values;
    dialog.title = "编辑蒙特卡洛模拟".into();
    Ok(())
}

/// Starts a uniform input without preloading parameters belonging to another distribution.
/// The editor supplies those defaults only when that distribution is selected.
pub(super) fn input_template(cell: &str) -> Json {
    json!({"cell":cell,"distribution":"uniform","min":"0","max":"1"})
}

/// Edits a labeled textual parameter while retaining typed numeric values until an actual edit.
/// Engine validation receives invalid text unchanged instead of silently substituting a default.
fn text(ui: &mut egui::Ui, values: &mut Map<String, Json>, key: &str, label: &str) {
    ui.horizontal(|ui| {
        let label = ui.label(label);
        let old = values
            .get(key)
            .map(|v| match v {
                Json::String(s) => s.clone(),
                _ => v.to_string(),
            })
            .unwrap_or_default();
        let mut value = old.clone();

        let resp = ui.add(
            egui::TextEdit::singleline(&mut value)
                .desired_width(110.0)
                .id_source(format!("mc_text_{key}"))
        )
        .labelled_by(label.id);

        if resp.changed() && value != old {
            values.insert(key.into(), json!(value));
        }
    });
}

/// Names distributions without presenting log-space parameters as arithmetic moments.
fn distribution_label(kind: &str) -> &str {
    match kind {
        "uniform" => "均匀分布",
        "triangular" => "三角分布",
        "normal" => "正态分布",
        "lognormal" => "对数正态分布",
        "poisson" => "泊松分布",
        "binomial" => "二项计数分布",
        "hypergeometric" => "超几何计数分布",
        _ => kind,
    }
}

/// Lists scalar parameters and defaults; existing values survive distribution changes.
fn distribution_fields(kind: &str) -> &'static [(&'static str, &'static str, &'static str)] {
    match kind {
        "normal" => &[("mean", "均值:", "0"), ("stdDev", "标准差:", "1")],
        "lognormal" => &[("mean", "算术均值:", "1"), ("cv", "变异系数 (标准差/均值):", "0.1")],
        "poisson" => &[("mean", "平均事件数:", "10")],
        "binomial" => &[("trials", "试验次数:", "10"), ("probability", "成功概率:", "0.5")],
        "hypergeometric" => {
            &[("draws", "样本量:", "10"), ("successes", "总体中的成功个体数:", "20"), ("population", "总体规模:", "100")]
        }
        "triangular" => &[("min", "最小:", "0"), ("mode", "众数:", "0.5"), ("max", "最大:", "1")],
        _ => &[("min", "最小:", "0"), ("max", "最大:", "1")],
    }
}

/// Renders distribution-specific controls without replacing invalid user-entered values.
fn input_parameters(ui: &mut egui::Ui, values: &mut Map<String, Json>) {
    let fields = distribution_fields(values.get("distribution").and_then(Json::as_str).unwrap_or("uniform"));
    ui.horizontal_wrapped(|ui| {
        for &(key, label, default) in fields {
            values.entry(key.to_string()).or_insert_with(|| json!(default));
            text(ui, values, key, label);
        }
    });
}

/// Keeps the nested input in sync while its reference box is drawn in collapsed mode.
pub(super) fn sync_collapsed_input(d: &mut Dialog, key: &str) {
    let Some(index) = key.strip_prefix("monteCarloInput").and_then(|n| n.parse::<usize>().ok()) else { return };
    let Some(cell) = d.values.get(key).cloned() else { return };
    if let Some(input) =
        d.values.get_mut("inputs").and_then(Json::as_array_mut).and_then(|inputs| inputs.get_mut(index)).and_then(Json::as_object_mut)
    {
        input.insert("cell".into(), cell);
    }
}

/// Draws the bounded input editor, sampling settings, matrix and outputs without evaluating
/// formulas. Replica settings apply only to LHS; confirmation delegates validation to the engine.
pub(super) fn form(app: &SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    ui.label(
        "选择数值输入单元格及输出单元格。原始输入单元格的值不会被改写；可选的相关系数，作用于隐高斯变量上。"
    );
    ui.separator();
    let mut inputs = d.values.get("inputs").and_then(Json::as_array).cloned().unwrap_or_default();
    let mut remove = None;
    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
        for (index, item) in inputs.iter_mut().enumerate() {
            ui.push_id(index, |ui| {
                ui.group(|ui| {
                    if let Some(values) = item.as_object_mut() {
                        ui.horizontal(|ui| {
                            let key = format!("monteCarloInput{index}");
                            if d.refs.target.as_deref() != Some(&key) {
                                d.values.insert(key.clone(), values.get("cell").cloned().unwrap_or(json!("")));
                            }
                            let label = ui.label("Cell:");
                            ref_edit(app, ui, d, &key, RefLabel::By(label.id), 140.0);
                            if let Some(value) = d.values.get(&key) {
                                values.insert("cell".into(), value.clone());
                            }
                            let mut kind = values.get("distribution").and_then(Json::as_str).unwrap_or("uniform").to_string();
                            let label = ui.label("Distribution:");
                            egui::ComboBox::from_id_salt(format!("distribution_{index}"))
                                .selected_text(distribution_label(&kind))
                                .show_ui(ui, |ui| {
                                    for k in ["uniform", "triangular", "normal", "lognormal", "poisson", "binomial", "hypergeometric"] {
                                        ui.selectable_value(&mut kind, k.to_string(), distribution_label(k));
                                    }
                                })
                                .response
                                .labelled_by(label.id);
                            values.insert("distribution".into(), json!(kind));
                            if ui.button("删除").clicked() {
                                remove = Some(index);
                            }
                        });
                        input_parameters(ui, values);
                    }
                });
            });
        }
    });
    if let Some(i) = remove {
        inputs.remove(i);
        d.refs.target = None;
        d.refs.collapsed = None;
    }
    if inputs.len() < 32 && ui.button("添加输入").clicked() {
        inputs.push(input_template(""));
    }
    d.values.insert("inputs".into(), json!(inputs));
    ui.label("对于条件计数以及月度一阶自回归（AR1）过程，请使用MC.*工作表函数，并配合IF(MC.ACTIVE(), 抽样值, 基准值)。");
    ui.separator();
    ui.horizontal(|ui| {
        let label = ui.label("抽样:");
        let mut method = d.values.get("method").and_then(Json::as_str).unwrap_or("random").to_string();
        let selected = if method == "latinHypercube" { "拉丁超立方" } else { "随机蒙特卡洛" };
        egui::ComboBox::from_id_salt("mcMethod")
            .selected_text(selected)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut method, "random".into(), "随机蒙特卡洛");
                ui.selectable_value(&mut method, "latinHypercube".into(), "拉丁超立方");
            })
            .response
            .labelled_by(label.id);
        if method == "latinHypercube" {
            text(ui, &mut d.values, "replicates", "独立重复批次:");
        }
        d.values.insert("method".into(), json!(method));
    });
    let matrix_label = ui.label("按输入顺序填写相关矩阵（空白 = 独立），每行一条，空格分隔数值；是隐高斯相关，不是皮尔逊相关。");
    let mut matrix = match d.values.get("correlation") {
        Some(Json::String(s)) => s.clone(),
        Some(Json::Array(rows)) => rows
            .iter()
            .filter_map(Json::as_array)
            .map(|row| row.iter().map(Json::to_string).collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };
    let old_converted = matrix.clone();
    let matrix_response = ui.add(
        egui::TextEdit::multiline(&mut matrix)
            .desired_rows(2)
            .desired_width(510.0)
            .id_source("mc_correlation_matrix")
    )
    .labelled_by(matrix_label.id);
    let mut multiline_focused = matrix_response.has_focus();
    if matrix_response.changed() && matrix != old_converted {
        d.values.insert("correlation".into(), json!(matrix));
    }
    let outputs_label = ui.label("输出单元格，每行一个（示例：Model!F30）, 跨工作表引用时，带上工作表名称.");
    let old_outputs = d.values.get("outputs")
        .and_then(Json::as_str)
        .unwrap_or("");
    let mut outputs = old_outputs.to_string();
    let outputs_response = ui.add(
        egui::TextEdit::multiline(&mut outputs)
            .desired_rows(2)
            .desired_width(510.0)
            .id_source("mc_outputs_multiline")
    )
    .labelled_by(outputs_label.id);
    multiline_focused |= outputs_response.has_focus();
    if outputs_response.changed() && outputs != old_outputs {
        d.values.insert("outputs".into(), json!(outputs));
    }
    ui.horizontal(|ui| {
        text(ui, &mut d.values, "iterations", "迭代次数:");
        text(ui, &mut d.values, "seed", "随机数种子:");
    });
    let mut samples = d.values.get("samples").and_then(Json::as_bool).unwrap_or(true);
    if ui.checkbox(&mut samples, "在报告中包含抽样样本数据").changed() {
        d.values.insert("samples".into(), json!(samples));
    }
    ui.label("生成静态报告，包含百分位结果，以及针对第一个输出变量的蒙特卡洛龙卷风图。修改模型后需要重新运行；大型模型可能耗时较长。");
    super::show::ok_cancel_with_enter(ui, confirm, open, !multiline_focused);
}

/// Invalid typed numbers stay strings so the engine rejects them instead of silently defaulting.
/// Replica counts are sent only for LHS, allowing arbitrary valid random iteration counts;
/// output references stay line-delimited and correlation rows preserve invalid numeric tokens.
pub(super) fn params(d: &Dialog) -> Json {
    let mut values = d.values.clone();
    values.retain(|key, _| !key.starts_with("monteCarloInput"));
    if values.get("method").and_then(Json::as_str) != Some("latinHypercube") {
        values.remove("replicates");
    }
    for key in ["iterations", "seed", "replicates"] {
        if let Some(n) = values.get(key).and_then(Json::as_str).and_then(|s| s.trim().parse::<u64>().ok()) {
            values.insert(key.into(), json!(n));
        }
    }
    if let Some(inputs) = values.get_mut("inputs").and_then(Json::as_array_mut) {
        for input in inputs {
            if let Some(input) = input.as_object_mut() {
                for key in ["min", "mode", "max", "mean", "stdDev", "cv", "trials", "probability", "draws", "successes", "population"] {
                    if let Some(n) = input.get(key).and_then(Json::as_str).and_then(|s| s.trim().parse::<f64>().ok()).filter(|n| n.is_finite()) {
                        input.insert(key.into(), json!(n));
                    }
                }
            }
        }
    }
    if let Some(outputs) = values.get("outputs").and_then(Json::as_str) {
        let outputs = outputs.lines().map(str::trim).filter(|s| !s.is_empty()).map(|s| json!(s)).collect::<Vec<_>>();
        values.insert("outputs".into(), json!(outputs));
    }
    if let Some(text) = values.get("correlation").and_then(Json::as_str) {
        if text.trim().is_empty() {
            values.remove("correlation");
        } else {
            let matrix = text
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| {
                    Json::Array(
                        line.split(|c: char| c.is_whitespace() || c == ',' || c == ';')
                            .filter(|s| !s.is_empty())
                            .map(|v| v.parse::<f64>().ok().filter(|n| n.is_finite()).map(|n| json!(n)).unwrap_or_else(|| json!(v)))
                            .collect(),
                    )
                })
                .collect::<Vec<_>>();
            values.insert("correlation".into(), json!(matrix));
        }
    }
    Json::Object(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A report can be reopened, edited and run again without overwriting the original report.
    #[test]
    fn monte_carlo_report_configuration_can_be_edited_and_rerun() {
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        session.execute("cell.set", json!({"cell":"A1","input":"2"})).unwrap();
        session.execute("cell.set", json!({"cell":"B1","input":"=A1*2"})).unwrap();
        let config =
            json!({"inputs":[{"cell":"A1","distribution":"uniform","min":1,"max":3}],"outputs":["B1"],"iterations":32,"seed":42,"samples":false});
        let original = session.execute("data.monteCarlo", config).unwrap();
        let mut app = crate::SheetApp::new(session, Default::default());
        let mut dialog = Dialog::open(&mut app, "monteCarlo", json!({})).unwrap();
        assert_eq!(dialog.title, "Edit Monte Carlo Simulation");
        assert!(dialog.values["outputs"].as_str().unwrap().contains('!'));
        dialog.values.insert("iterations".into(), json!("48"));
        let rerun = app.session.execute("data.monteCarlo", params(&dialog)).unwrap();
        assert_eq!(rerun["iterations"], 48);
        assert_ne!(original["sheet"], rerun["sheet"]);
        assert!(app.session.active().unwrap().wb.sheet_index(original["sheet"].as_str().unwrap()).is_some());
    }

    /// Malformed reports cannot silently turn into an unrelated default simulation.
    #[test]
    fn monte_carlo_invalid_saved_configuration_is_rejected() {
        let mut sheet = gridcraft_engine::model::Sheet::new("report");
        use gridcraft_engine::core::{CellRef, Value};
        assert!(saved_configuration(&sheet).is_none());
        sheet.set_value(CellRef::new(0, 0), Value::text("Monte Carlo simulation"));
        sheet.set_value(CellRef::new(3, 0), Value::text("Reusable configuration (JSON)"));
        sheet.set_value(CellRef::new(3, 1), Value::text("not json"));
        assert!(saved_configuration(&sheet).unwrap().is_err());
    }

    /// Unqualified inputs stay tied to the sheet where the dialog was opened.
    #[test]
    fn monte_carlo_dialog_returns_to_its_opening_sheet() {
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        session.execute("home.insertSheet", json!({})).unwrap();
        session.execute("sheet.activate", json!({"sheet":0})).unwrap();
        let mut app = crate::SheetApp::new(session, Default::default());
        let original = app.session.active().unwrap().wb.active_sheet();
        let mut dialog = Dialog::open(&mut app, "monteCarlo", json!({})).unwrap();
        app.session.execute("sheet.activate", json!({"sheet":1})).unwrap();
        dialog.end_pick(&mut app).unwrap();
        assert_eq!(app.session.active().unwrap().wb.active_sheet(), original);
    }

    /// Adding an input and then selecting a distribution supplies its own untouched defaults.
    #[test]
    fn monte_carlo_added_inputs_use_distribution_defaults_without_overwriting_edits() {
        use egui_kittest::kittest::Queryable as _;
        for (kind, mean) in [("lognormal", 1.0), ("poisson", 10.0)] {
            let mut session = gridcraft_engine::Session::new();
            session.new_workbook();
            let app = SheetApp::new(session, Default::default());
            let d = Dialog::custom("monteCarlo", "Monte Carlo", json!({"inputs":[input_template("A1")],"method":"random"}));
            let mut h = egui_kittest::Harness::builder()
                .with_size(egui::vec2(1000.0, 1000.0))
                .build_ui_state(|ui, d: &mut Dialog| form(&app, ui, d, &mut false, &mut true), d);
            h.run_steps(2);
            h.get_by_label("Add input").click();
            h.run_steps(2);
            let inputs = h.state_mut().values.get_mut("inputs").unwrap().as_array_mut().unwrap();
            assert_eq!(inputs.len(), 2);
            for input in inputs {
                assert!(input.get("mean").is_none());
                input["distribution"] = json!(kind);
            }
            h.run_steps(2);
            let p = params(h.state());
            assert_eq!(p["inputs"][0]["mean"], mean);
            assert_eq!(p["inputs"][1]["mean"], mean);
            h.state_mut().values["inputs"][1]["mean"] = json!("0");
            h.run_steps(2);
            assert_eq!(params(h.state())["inputs"][1]["mean"], 0.0);
        }
    }

    /// Every added distribution reaches the engine with numeric parameters, including whitespace.
    #[test]
    fn monte_carlo_dialog_serializes_discrete_and_lognormal_parameters() {
        let d = Dialog::custom(
            "monteCarlo",
            "Monte Carlo",
            json!({"inputs":[
                {"cell":"A1","distribution":"lognormal","mean":" 120 ","cv":"0.25"},
                {"cell":"A2","distribution":"poisson","mean":"12.5"},
                {"cell":"A3","distribution":"binomial","trials":" 40 ","probability":"0.3"},
                {"cell":"A4","distribution":"hypergeometric","draws":"10","successes":"20","population":"100"}
            ]}),
        );
        let p = params(&d);
        assert_eq!(p["inputs"][0]["distribution"], "lognormal");
        assert_eq!(p["inputs"][0]["mean"], 120.0);
        assert_eq!(p["inputs"][0]["cv"], 0.25);
        assert_eq!(p["inputs"][1]["mean"], 12.5);
        assert_eq!(p["inputs"][2]["trials"], 40.0);
        assert_eq!(p["inputs"][2]["probability"], 0.3);
        assert_eq!(p["inputs"][3]["draws"], 10.0);
        assert_eq!(p["inputs"][3]["successes"], 20.0);
        assert_eq!(p["inputs"][3]["population"], 100.0);
    }

    /// Fractions are never truncated into valid counts; invalid text and nonfinite values remain visible to validation.
    #[test]
    fn monte_carlo_dialog_preserves_invalid_new_distribution_parameters() {
        let d = Dialog::custom(
            "monteCarlo",
            "Monte Carlo",
            json!({"inputs":[
                {"distribution":"binomial","trials":"3.5","probability":"bad"},
                {"distribution":"hypergeometric","draws":"1.25","successes":"NaN","population":"inf"},
                {"distribution":"lognormal","mean":"-1","cv":"overflow"}
            ]}),
        );
        let p = params(&d);
        assert_eq!(p["inputs"][0]["trials"], 3.5);
        assert_eq!(p["inputs"][0]["probability"], "bad");
        assert_eq!(p["inputs"][1]["draws"], 1.25);
        assert_eq!(p["inputs"][1]["successes"], "NaN");
        assert_eq!(p["inputs"][1]["population"], "inf");
        assert_eq!(p["inputs"][2]["mean"], -1.0);
        assert_eq!(p["inputs"][2]["cv"], "overflow");
    }

    /// Invalid numeric text must survive UI conversion so the engine can report it to the user.
    #[test]
    fn monte_carlo_dialog_builds_typed_params_without_hiding_invalid_input() {
        let d = Dialog::custom(
            "monteCarlo",
            "Monte Carlo",
            json!({"iterations":"2000","seed":"42","inputs":[{"cell":"B2","distribution":"normal","mean":"10","stdDev":"bad"}],"outputs":"Sheet1!C1\n'Other, sheet'!B2\n"}),
        );
        let p = params(&d);
        assert_eq!(p["iterations"], 2000);
        assert_eq!(p["seed"], 42);
        assert_eq!(p["inputs"][0]["mean"], 10.0);
        assert_eq!(p["inputs"][0]["stdDev"], "bad");
        assert_eq!(p["outputs"].as_array().map(Vec::len), Some(2));
    }

    /// Switching a dialog from LHS to random must remove the stale replica restriction.
    /// Exercise the resulting command as well as the serialized parameters for odd trial counts.
    #[test]
    fn monte_carlo_random_dialog_accepts_iterations_not_divisible_by_lhs_replicates() {
        for iterations in [1001, 1500] {
            let mut d = Dialog::custom(
                "monteCarlo",
                "Monte Carlo",
                json!({"method":"latinHypercube","replicates":"8","iterations":iterations.to_string(),"inputs":[{"cell":"A1","min":"0","max":"1"}],"outputs":"B1","samples":false}),
            );
            let lhs = params(&d);
            assert_eq!(lhs["replicates"], 8);
            d.values.insert("method".into(), json!("random"));
            let p = params(&d);
            assert!(p.get("replicates").is_none());
            let mut session = gridcraft_engine::Session::new();
            session.new_workbook();
            session.execute("cell.set", json!({"cell":"A1","input":"0.5"})).unwrap();
            session.execute("cell.set", json!({"cell":"B1","input":"=A1*2"})).unwrap();
            let result = session.execute("data.monteCarlo", p).unwrap();
            assert_eq!(result["iterations"], iterations);
            assert_eq!(result["replicates"], 1);
        }
    }
}
