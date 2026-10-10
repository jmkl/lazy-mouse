//! Settings window, run in its own process. Each change is sent to the main
//! process as a thread message: wparam = Param index, lparam = f64 bits.

use crate::config::{Param, Settings};
use egui::{Id, Margin, PointerButton, RichText, Sense, UiBuilder, ViewportCommand};
use elegance::{Accent, Button, MetricSlider, Slider, Theme, glyphs};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_APP};

pub const WM_SET: u32 = WM_APP + 3;

fn send(main_thread: u32, p: Param, v: f64) {
    unsafe {
        let _ = PostThreadMessageW(
            main_thread,
            WM_SET,
            WPARAM(p as usize),
            LPARAM(v.to_bits() as isize),
        );
    }
}

struct App {
    main_thread: u32,
    values: [f32; Param::COUNT], // in Param::ALL order
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let tid = self.main_thread;
        Theme::slate().install(ui.ctx());
        egui::CentralPanel::default().show(ui, |ui| {
            egui::Frame::new().corner_radius(0).show(ui, |ui| {
                let app_rect = ui.max_rect();
                ui.expand_to_include_rect(app_rect);
                let title_bar_height = 20.0;
                let title_bar_rect = {
                    let mut rect = app_rect;
                    rect.max.y = rect.min.y + title_bar_height;
                    rect
                };
                // Add the contents:
                let content_rect = {
                    let mut rect = app_rect;
                    rect.min.y = title_bar_rect.max.y;
                    rect
                };
                ui.scope_builder(
                    UiBuilder::new()
                        .max_rect(title_bar_rect)
                        .layout(egui::Layout::right_to_left(egui::Align::RIGHT)),
                    |ui| {
                        let tb_response = ui.interact(
                            title_bar_rect,
                            egui::Id::new("title_bar"),
                            Sense::click_and_drag(),
                        );
                        if tb_response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                        }
                        if tb_response.drag_started_by(PointerButton::Primary) {
                            ui.send_viewport_cmd(ViewportCommand::StartDrag);
                        }
                        let close_response = ui
                            .add(
                                Button::icon(glyphs::X, "Close")
                                    .size(elegance::ButtonSize::Small)
                                    .accent(Accent::Red),
                            )
                            .on_hover_text("Close the window");
                        if close_response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if close_response.clicked() {
                            ui.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    },
                );
                let mut content = ui.new_child(UiBuilder::new().max_rect(content_rect));
                content.horizontal(|ui| {
                    egui::Frame::NONE
                        .inner_margin(Margin::same(10))
                        .show(ui, |ui| {
                            egui::Grid::new("params").num_columns(1).show(ui, |ui| {
                                for (i, p) in Param::ALL.into_iter().enumerate() {
                                    let (lo, hi) = p.range();
                                    // ui.label(p.name()).on_hover_text(p.help());

                                    // let mut slider = egui::Slider::new(&mut self.values[i], lo..=hi);

                                    let slider = match p {
                                        Param::Friction => {
                                            Slider::new(&mut self.values[i], lo as f32..=hi as f32)
                                                .label(p.name())
                                                .accent(Accent::Green)
                                                .step(0.05)
                                        }

                                        Param::Stretch => {
                                            Slider::new(&mut self.values[i], lo as f32..=1000.0)
                                                .label(p.name())
                                                .accent(Accent::Green)
                                                .step(50.0)
                                        }

                                        _ => {
                                            Slider::new(&mut self.values[i], lo as f32..=hi as f32)
                                                .label(p.name())
                                                .step(1.0)
                                                .accent(Accent::Green)
                                        }
                                    };

                                    let r = ui.add(slider);

                                    // Radius rebuilds the overlay window, so send it on release only.
                                    let commit = if p == Param::Radius {
                                        r.drag_stopped() || (r.changed() && !r.dragged())
                                    } else {
                                        r.changed()
                                    };
                                    if commit {
                                        send(tid, p, self.values[i] as f64);
                                    }
                                    ui.end_row();
                                }
                            });
                        });
                });
            });
        });
    }
}

pub fn run(main_thread: u32, s: Settings) {
    let app = App {
        main_thread,
        values: Param::ALL.map(|p| s.get(p) as f32),
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([285.0, 320.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false),

        ..Default::default()
    };
    let _ = eframe::run_native("Lazy Mouse", options, Box::new(|_cc| Ok(Box::new(app))));
}
