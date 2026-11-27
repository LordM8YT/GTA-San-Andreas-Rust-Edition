//! Test › Server › Network: live graphs of the test's connection, the
//! last two minutes of ping, data and lost packets, and a line per player
//! connection. Ping is measured through the Connection link (a status
//! request every second), so it is what the players have now.
//!
//! The idea (a fixed history per value, drawn as egui lines) follows
//! renet_visualizer (MIT OR Apache-2.0), written anew for SARE's own
//! counters.
use super::BuildMode;
use crate::ui_theme::{ACCENT, MUTED, SUCCESS, SURFACE};
use egui::{Color32, RichText, Ui};
use sa_net::conditioner::{Conditioner, Traffic};
use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

/// Seconds of history kept and drawn.
pub const HISTORY: usize = 120;
const STEP: Duration = Duration::from_secs(1);

/// What one test has measured so far.
pub struct NetGraphs {
    /// Ping in ms each second (None: no answer in time).
    pub ping: VecDeque<Option<f32>>,
    /// All players together, bytes per second.
    pub down: VecDeque<f32>,
    pub up: VecDeque<f32>,
    /// Lost packets per second.
    pub lost: VecDeque<f32>,
    pings: Option<Receiver<Option<f32>>>,
    last: Option<(Instant, [u64; 3])>,
}

impl NetGraphs {
    /// Start measuring ping through `conditioner` on a thread of its own.
    pub fn start(conditioner: Option<&Arc<Conditioner>>) -> Self {
        let pings = conditioner.map(|conditioner| {
            let (send, receive) = channel();
            let weak: Weak<Conditioner> = Arc::downgrade(conditioner);
            let _ = std::thread::Builder::new()
                .name("sare-test-ping".into())
                .spawn(move || loop {
                    let started = Instant::now();
                    let Some(conditioner) = weak.upgrade() else {
                        break;
                    };
                    let ping = conditioner.measure().ok().map(|d| d.as_secs_f32() * 1000.0);
                    drop(conditioner);
                    if send.send(ping).is_err() {
                        break;
                    }
                    std::thread::sleep(STEP.saturating_sub(started.elapsed()));
                });
            receive
        });
        Self {
            ping: VecDeque::new(),
            down: VecDeque::new(),
            up: VecDeque::new(),
            lost: VecDeque::new(),
            pings,
            last: None,
        }
    }

    /// Once a frame: take new pings, and once a second the data counters.
    pub fn tick(&mut self, traffic: &[Arc<Traffic>], now: Instant) {
        if let Some(pings) = &self.pings {
            while let Ok(ping) = pings.try_recv() {
                push(&mut self.ping, ping);
            }
        }
        let totals = totals(traffic);
        match self.last {
            None => self.last = Some((now, totals)),
            Some((then, before)) if now.duration_since(then) >= STEP => {
                let seconds = now.duration_since(then).as_secs_f32();
                let rate = |i: usize| totals[i].saturating_sub(before[i]) as f32 / seconds;
                push(&mut self.down, rate(0));
                push(&mut self.up, rate(1));
                push(&mut self.lost, rate(2));
                self.last = Some((now, totals));
            }
            Some(_) => {}
        }
    }
}

/// Bytes down, bytes up and lost packets of every connection.
fn totals(traffic: &[Arc<Traffic>]) -> [u64; 3] {
    traffic.iter().fold([0; 3], |sum, t| {
        [
            sum[0] + t.down.load(Ordering::Relaxed),
            sum[1] + t.up.load(Ordering::Relaxed),
            sum[2] + t.lost.load(Ordering::Relaxed),
        ]
    })
}

fn push<T>(history: &mut VecDeque<T>, value: T) {
    if history.len() == HISTORY {
        history.pop_front();
    }
    history.push_back(value);
}

/// "12.3 KB/s".
fn rate_label(bytes: f32) -> String {
    if bytes >= 1_000_000.0 {
        format!("{:.1} MB/s", bytes / 1_000_000.0)
    } else {
        format!("{:.1} KB/s", bytes / 1000.0)
    }
}

/// "3.4 MB".
fn size_label(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f32 / 1_000_000.0)
    } else {
        format!("{:.0} KB", bytes as f32 / 1000.0)
    }
}

/// A line chart of `lines` (all on one scale) over the last `HISTORY`
/// seconds. Gaps (None) break the line.
fn chart(ui: &mut Ui, lines: &[(&[Option<f32>], Color32)], unit: fn(f32) -> String) {
    let height = 64.0;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, SURFACE);
    let top = lines
        .iter()
        .flat_map(|(values, _)| values.iter().flatten())
        .fold(0.0f32, |a, &b| a.max(b));
    let top = if top > 0.0 { top * 1.15 } else { 1.0 };
    let x = |i: usize, len: usize| {
        rect.right() - (len - 1 - i) as f32 * rect.width() / (HISTORY - 1) as f32
    };
    let y = |v: f32| rect.bottom() - 4.0 - (v / top) * (height - 8.0);
    for (values, colour) in lines {
        let mut run: Vec<egui::Pos2> = Vec::new();
        for (i, value) in values.iter().enumerate() {
            match value {
                Some(v) => run.push(egui::pos2(x(i, values.len()), y(*v))),
                None => {
                    draw_run(&painter, &mut run, *colour);
                }
            }
        }
        draw_run(&painter, &mut run, *colour);
    }
    painter.text(
        rect.left_top() + egui::vec2(4.0, 2.0),
        egui::Align2::LEFT_TOP,
        unit(top),
        egui::FontId::proportional(10.0),
        MUTED,
    );
}

fn draw_run(painter: &egui::Painter, run: &mut Vec<egui::Pos2>, colour: Color32) {
    match run.len() {
        0 => {}
        1 => {
            painter.circle_filled(run[0], 1.5, colour);
        }
        _ => {
            painter.add(egui::Shape::line(
                std::mem::take(run),
                egui::Stroke::new(1.5, colour),
            ));
        }
    };
    run.clear();
}

impl BuildMode {
    /// The Network window, while it is open.
    pub fn net_graphs_window(&mut self, ctx: &egui::Context) {
        if !self.ui.test_graphs {
            return;
        }
        let mut open = true;
        egui::Window::new("📈 Network")
            .open(&mut open)
            .default_width(360.0)
            .resizable(true)
            .show(ctx, |ui| {
                let Some(test) = &self.local_test else {
                    ui.label("Start a test (Test › Server) to see its connection here.");
                    return;
                };
                ui.label(
                    RichText::new(format!(
                        "Connection: {}",
                        super::local_test::describe(self.ui.test_link)
                    ))
                    .small(),
                );
                let graphs = &test.graphs;
                let latest = |v: &VecDeque<f32>| v.back().copied().unwrap_or(0.0);
                ui.add_space(4.0);
                let ping_now = match graphs.ping.back() {
                    Some(Some(ms)) => format!("{ms:.0} ms"),
                    Some(None) => "no answer".into(),
                    None => "measuring...".into(),
                };
                ui.label(format!("Ping: {ping_now}"))
                    .on_hover_text("A status request through the connection every second: the time from asking to the server's answer, as a player's data travels. The server answers on its next tick (up to 50 ms).");
                let ping: Vec<Option<f32>> = graphs.ping.iter().copied().collect();
                chart(ui, &[(&ping, ACCENT)], |v| format!("{v:.0} ms"));
                ui.add_space(4.0);
                ui.label(format!(
                    "Data: ↓ {}  ↑ {}",
                    rate_label(latest(&graphs.down)),
                    rate_label(latest(&graphs.up))
                ))
                .on_hover_text("All players together. ↓ server to players (green), ↑ players to server (blue).");
                let down: Vec<Option<f32>> = graphs.down.iter().map(|&v| Some(v)).collect();
                let up: Vec<Option<f32>> = graphs.up.iter().map(|&v| Some(v)).collect();
                chart(ui, &[(&down, SUCCESS), (&up, ACCENT)], rate_label);
                ui.add_space(4.0);
                ui.label(format!("Lost packets: {:.0}/s", latest(&graphs.lost)))
                    .on_hover_text("Packets Connection lost on purpose; each one held its stream back for a resend.");
                let lost: Vec<Option<f32>> = graphs.lost.iter().map(|&v| Some(v)).collect();
                chart(ui, &[(&lost, super::panels::WARNING)], |v| format!("{v:.0}/s"));
                ui.add_space(6.0);
                let connections = test
                    .conditioner
                    .as_ref()
                    .map(|c| c.traffic())
                    .unwrap_or_default();
                if connections.is_empty() {
                    ui.label(RichText::new("No player has connected yet.").small().color(MUTED));
                }
                egui::Grid::new("net-connections").striped(true).show(ui, |ui| {
                    if !connections.is_empty() {
                        for heading in ["Connection", "", "↓", "↑", "Lost"] {
                            ui.label(RichText::new(heading).small().strong());
                        }
                        ui.end_row();
                    }
                    for (n, t) in connections.iter().enumerate() {
                        let open = t.open.load(Ordering::Relaxed);
                        ui.label(RichText::new(format!("#{}", n + 1)).small());
                        ui.label(
                            RichText::new(if open { "open" } else { "closed" })
                                .small()
                                .color(if open { SUCCESS } else { MUTED }),
                        );
                        ui.label(RichText::new(size_label(t.down.load(Ordering::Relaxed))).small());
                        ui.label(RichText::new(size_label(t.up.load(Ordering::Relaxed))).small());
                        ui.label(RichText::new(t.lost.load(Ordering::Relaxed).to_string()).small());
                        ui.end_row();
                    }
                });
            });
        if !open {
            self.ui.test_graphs = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_is_sampled_once_a_second_and_history_is_capped() {
        let traffic = Arc::new(Traffic::default());
        let list = [traffic.clone()];
        let mut graphs = NetGraphs::start(None);
        let start = Instant::now();
        graphs.tick(&list, start);
        traffic.down.store(4_000, Ordering::Relaxed);
        traffic.up.store(1_000, Ordering::Relaxed);
        traffic.lost.store(2, Ordering::Relaxed);
        graphs.tick(&list, start + Duration::from_millis(500));
        assert!(graphs.down.is_empty(), "not a second yet");
        graphs.tick(&list, start + Duration::from_secs(2));
        assert_eq!(graphs.down.back(), Some(&2_000.0));
        assert_eq!(graphs.up.back(), Some(&500.0));
        assert_eq!(graphs.lost.back(), Some(&1.0));
        for s in 3..300 {
            graphs.tick(&list, start + Duration::from_secs(s));
        }
        assert_eq!(graphs.down.len(), HISTORY);
        assert_eq!(graphs.down.back(), Some(&0.0));
    }

    #[test]
    fn labels_read_well() {
        assert_eq!(rate_label(12_345.0), "12.3 KB/s");
        assert_eq!(rate_label(2_500_000.0), "2.5 MB/s");
        assert_eq!(size_label(3_400_000), "3.4 MB");
        assert_eq!(size_label(800), "1 KB");
    }
}
