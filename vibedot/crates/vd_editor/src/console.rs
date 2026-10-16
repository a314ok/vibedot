//! Bottom dock: Output log and Problems (blueprint compile errors).

use eframe::egui::{self, RichText};
use vd_egui::theme;
use vd_engine::LogLevel;

use crate::app::{OpenProject, Request, Shared};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BottomTab {
    Output,
    Problems,
}

pub fn show(ui: &mut egui::Ui, p: &mut OpenProject, shared: &mut Shared) {
    let problems: Vec<(String, String, Option<vd_blueprint::BpNodeId>)> =
        p.blueprints.iter().flat_map(|b| b.errors.iter().map(move |e| (b.path.clone(), e.message.clone(), e.node))).collect();
    ui.horizontal(|ui| {
        let tab = |ui: &mut egui::Ui, t: BottomTab, label: String, cur: &mut BottomTab| {
            let active = *cur == t;
            if ui
                .add(egui::Button::new(RichText::new(label).color(if active { theme::TEXT } else { theme::TEXT_DIM })).fill(if active {
                    theme::SURFACE_HI
                } else {
                    egui::Color32::TRANSPARENT
                }))
                .clicked()
            {
                *cur = t;
            }
        };
        let errors = shared.log.iter().filter(|l| l.level == LogLevel::Error).count();
        tab(
            ui,
            BottomTab::Output,
            format!("📜 Output{}", if errors > 0 { format!("  ({errors}⚠)") } else { String::new() }),
            &mut p.bottom_tab,
        );
        tab(ui, BottomTab::Problems, format!("🚩 Problems ({})", problems.len()), &mut p.bottom_tab);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if p.bottom_tab == BottomTab::Output && ui.small_button("Clear").clicked() {
                shared.log.clear();
            }
        });
    });
    ui.separator();
    match p.bottom_tab {
        BottomTab::Output => {
            egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                for l in shared.log.iter().rev().take(500).collect::<Vec<_>>().into_iter().rev() {
                    let (icon, color) = match l.level {
                        LogLevel::Info => ("›", theme::TEXT),
                        LogLevel::Warn => ("⚠", theme::WARN),
                        LogLevel::Error => ("✖", theme::ERR),
                    };
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(format!("{:>7.2}", l.time)).monospace().small().color(theme::TEXT_DIM));
                        ui.label(RichText::new(icon).color(color));
                        ui.label(RichText::new(&l.text).monospace().color(color));
                    });
                }
            });
        }
        BottomTab::Problems => {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                if problems.is_empty() {
                    ui.label(RichText::new("✔ All open blueprints compile").color(theme::OK));
                }
                for (path, msg, node) in problems {
                    let text = match node {
                        Some(n) => format!("{path}  ·  node #{}  ·  {msg}", n.0),
                        None => format!("{path}  ·  {msg}"),
                    };
                    if ui
                        .add(egui::Label::new(RichText::new(text).color(theme::ERR)).sense(egui::Sense::click()))
                        .on_hover_text("Click to open")
                        .clicked()
                    {
                        shared.request(Request::OpenBlueprint(path.clone()));
                        if let Some(n) = node {
                            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("focus_bp_node"), n.0));
                        }
                    }
                }
            });
        }
    }
}
