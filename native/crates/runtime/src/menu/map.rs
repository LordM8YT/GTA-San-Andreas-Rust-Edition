//! The MAP tab: the whole state to pan and zoom, with
//! blips, a legend, the GPS route and a waypoint you place yourself.
use super::{
    add_radar_tile, blip_dot, clip_line, player_arrow, route_line, waypoint_pin, Action, Menu,
    RadarTile, DESTINATIONS, MUTED, WAYPOINT, WHITE,
};
use egui::{Color32, FontId, Pos2, Rect, Sense, Vec2};

const WORLD: f32 = 3000.0;
/// Most zoomed in, in pixels per metre.
const MAX_ZOOM: f32 = 1.6;
const PANEL: Color32 = Color32::from_black_alpha(185);

/// Screen <-> world for a map view centred on `center` at `zoom` px/m.
#[derive(Clone, Copy)]
struct View {
    rect: Rect,
    center: [f32; 2],
    zoom: f32,
}
impl View {
    fn screen(&self, xy: [f32; 2]) -> Pos2 {
        self.rect.center()
            + Vec2::new(
                (xy[0] - self.center[0]) * self.zoom,
                -(xy[1] - self.center[1]) * self.zoom,
            )
    }
    fn world(&self, pos: Pos2) -> [f32; 2] {
        let offset = pos - self.rect.center();
        [
            self.center[0] + offset.x / self.zoom,
            self.center[1] - offset.y / self.zoom,
        ]
    }
    /// Zoom by `factor` keeping the world point under `anchor` in place.
    fn zoom_at(&mut self, anchor: Pos2, factor: f32, min: f32) {
        let fixed = self.world(anchor);
        self.zoom = (self.zoom * factor).clamp(min, MAX_ZOOM);
        let offset = anchor - self.rect.center();
        self.center = [
            fixed[0] - offset.x / self.zoom,
            fixed[1] + offset.y / self.zoom,
        ];
    }
    fn clamp(&mut self) {
        for value in &mut self.center {
            *value = value.clamp(-WORLD, WORLD);
        }
    }
}

fn hint(painter: &egui::Painter, at: Pos2, text: &str, scale: f32) -> Pos2 {
    let galley = painter.layout_no_wrap(text.into(), FontId::proportional(13.0 * scale), WHITE);
    let width = galley.size().x;
    painter.galley(at, galley, WHITE);
    at + Vec2::new(width + 18.0 * scale, 0.0)
}

impl Menu {
    pub(super) fn gta_map(
        &mut self,
        ui: &mut egui::Ui,
        content: Rect,
        scale: f32,
        coordinates: [f32; 3],
        yaw: f32,
        radar_tiles: &[RadarTile],
    ) -> Option<Action> {
        let ctx = ui.ctx().clone();
        let mut action = None;
        let player = [coordinates[0], coordinates[1]];
        let fit = content.width().min(content.height()) / (2.0 * WORLD);
        let (center, zoom) = self
            .map_view
            .unwrap_or((player, (content.height() / 1600.0).max(fit)));
        let mut view = View {
            rect: content,
            center,
            zoom: zoom.clamp(fit, MAX_ZOOM),
        };

        // Mouse: drag to move, scroll to zoom, right-click or double-click
        // for a waypoint. Keys: WASD move, PageUp / PageDown zoom,
        // C back to the player, Backspace removes the waypoint.
        let response = ui.interact(content, egui::Id::new("gta-map"), Sense::click_and_drag());
        if response.dragged() {
            let delta = response.drag_delta();
            view.center[0] -= delta.x / view.zoom;
            view.center[1] += delta.y / view.zoom;
        }
        let typing = ctx.egui_wants_keyboard_input();
        let (scroll, dt, keys) = ctx.input(|i| {
            let down = |key| !typing && i.key_down(key);
            let pressed = |key| !typing && i.key_pressed(key);
            (
                i.smooth_scroll_delta.y,
                i.stable_dt.min(0.1),
                [
                    down(egui::Key::W),
                    down(egui::Key::S),
                    down(egui::Key::A),
                    down(egui::Key::D),
                    down(egui::Key::PageUp),
                    down(egui::Key::PageDown),
                    pressed(egui::Key::C),
                    pressed(egui::Key::Backspace),
                ],
            )
        });
        let [up, down, left, right, zoom_in, zoom_out, recenter, clear] = keys;
        let pan = 700.0 * dt / view.zoom;
        view.center[1] += pan * (up as i32 - down as i32) as f32;
        view.center[0] += pan * (right as i32 - left as i32) as f32;
        let mut factor = 1.0;
        if response.hovered() && scroll != 0.0 {
            factor *= (scroll * 0.004).exp();
        }
        factor *= (dt * 2.5 * (zoom_in as i32 - zoom_out as i32) as f32).exp();
        if factor != 1.0 {
            let anchor = response
                .hover_pos()
                .filter(|_| scroll != 0.0)
                .unwrap_or(content.center());
            view.zoom_at(anchor, factor, fit);
        }
        if recenter {
            view.center = player;
        }
        view.clamp();
        if clear {
            self.waypoint = None;
        }
        if response.secondary_clicked() || response.double_clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                let near = self
                    .waypoint
                    .is_some_and(|w| view.screen(w).distance(pos) < 14.0 * scale);
                self.waypoint = (!near).then(|| view.world(pos));
            }
        }

        let painter = ui.painter_at(content);
        painter.rect_filled(content, 0.0, Color32::from_rgb(52, 76, 94));
        let clip = [
            content.left_top(),
            content.right_top(),
            content.right_bottom(),
            content.left_bottom(),
        ];
        for tile in radar_tiles {
            add_radar_tile(&painter, *tile, content, view.center, 0.0, view.zoom, &clip);
        }
        if radar_tiles.is_empty() {
            painter.text(
                content.center(),
                egui::Align2::CENTER_CENTER,
                "Original map unavailable",
                FontId::proportional(16.0 * scale),
                MUTED,
            );
        }
        let route: Vec<Pos2> = self
            .overlay
            .route
            .iter()
            .map(|xy| view.screen(*xy))
            .collect();
        route_line(
            &painter,
            &clip_line(&route, content),
            self.overlay.route_color,
            4.0 * scale,
        );
        for blip in &self.overlay.blips {
            let size = if blip.route { 7.0 } else { 5.5 } * scale;
            blip_dot(&painter, view.screen(blip.position), size, blip.color);
        }
        if let Some(waypoint) = self.waypoint {
            waypoint_pin(&painter, view.screen(waypoint), 11.0 * scale);
        }
        let facing = Vec2::new(yaw.sin(), -yaw.cos());
        player_arrow(&painter, view.screen(player), facing, 11.0 * scale);
        // Hovered blip: its name beside the cursor.
        if let Some(pos) = response.hover_pos() {
            if let Some(blip) =
                self.overlay.blips.iter().find(|b| {
                    b.label.is_some() && view.screen(b.position).distance(pos) < 12.0 * scale
                })
            {
                painter.text(
                    pos + Vec2::new(14.0, -14.0) * scale,
                    egui::Align2::LEFT_BOTTOM,
                    blip.label.as_deref().unwrap_or_default(),
                    FontId::proportional(15.0 * scale),
                    WHITE,
                );
            }
        }

        // Legend on the right: click an entry to look at it.
        let mut entries: Vec<(Color32, String, [f32; 2], bool)> =
            vec![(WHITE, "You".into(), player, false)];
        if let Some(waypoint) = self.waypoint {
            entries.push((WAYPOINT, "Waypoint".into(), waypoint, true));
        }
        for blip in &self.overlay.blips {
            if let Some(label) = &blip.label {
                entries.push((blip.color, label.clone(), blip.position, false));
            }
        }
        let width = 230.0 * scale;
        let row = 28.0 * scale;
        let legend = Rect::from_min_size(
            Pos2::new(
                content.right() - width - 12.0 * scale,
                content.top() + 12.0 * scale,
            ),
            Vec2::new(width, row * (entries.len() as f32 + 1.0) + 8.0 * scale),
        );
        painter.rect_filled(legend, 0.0, PANEL);
        painter.text(
            legend.left_top() + Vec2::new(12.0, 8.0) * scale,
            egui::Align2::LEFT_TOP,
            "LEGEND",
            FontId::proportional(14.0 * scale),
            MUTED,
        );
        for (index, (color, label, position, pin)) in entries.iter().enumerate() {
            let line = Rect::from_min_size(
                legend.left_top() + Vec2::new(0.0, row * (index as f32 + 1.0) + 4.0 * scale),
                Vec2::new(width, row),
            );
            let clicked = ui
                .interact(
                    line,
                    egui::Id::new(("gta-map-legend", index)),
                    Sense::click(),
                )
                .clicked();
            if clicked {
                view.center = *position;
            }
            let dot = line.left_center() + Vec2::new(20.0 * scale, 0.0);
            if *pin {
                waypoint_pin(&painter, dot + Vec2::new(0.0, 8.0 * scale), 9.0 * scale);
            } else if index == 0 {
                player_arrow(&painter, dot, Vec2::new(0.0, -1.0), 8.0 * scale);
            } else {
                blip_dot(&painter, dot, 6.0 * scale, *color);
            }
            painter.text(
                dot + Vec2::new(18.0 * scale, 0.0),
                egui::Align2::LEFT_CENTER,
                label,
                FontId::proportional(15.0 * scale),
                WHITE,
            );
        }

        // Travel list on the left (1-9, or the D-pad and A on a controller).
        let width = 220.0 * scale;
        let travel = Rect::from_min_size(
            content.left_top() + Vec2::splat(12.0 * scale),
            Vec2::new(width, row * (DESTINATIONS.len() as f32 + 1.0) + 8.0 * scale),
        );
        painter.rect_filled(travel, 0.0, PANEL);
        painter.text(
            travel.left_top() + Vec2::new(12.0, 8.0) * scale,
            egui::Align2::LEFT_TOP,
            "TRAVEL",
            FontId::proportional(14.0 * scale),
            MUTED,
        );
        for (index, (name, _)) in DESTINATIONS.iter().enumerate() {
            let line = Rect::from_min_size(
                travel.left_top() + Vec2::new(0.0, row * (index as f32 + 1.0) + 4.0 * scale),
                Vec2::new(width, row),
            );
            let response = ui.interact(
                line,
                egui::Id::new(("gta-map-travel", index)),
                Sense::click(),
            );
            let selected = self.selected == index;
            if selected {
                painter.rect_filled(line, 0.0, WHITE);
            } else if response.hovered() {
                painter.rect_filled(line, 0.0, Color32::from_black_alpha(215));
            }
            painter.text(
                line.left_center() + Vec2::new(12.0 * scale, 0.0),
                egui::Align2::LEFT_CENTER,
                format!("{}  {name}", index + 1),
                FontId::proportional(15.0 * scale),
                if selected {
                    Color32::from_rgb(20, 20, 20)
                } else {
                    WHITE
                },
            );
            if response.clicked() {
                action = Some(Action::Teleport(index));
            }
        }
        for (index, key) in [
            egui::Key::Num1,
            egui::Key::Num2,
            egui::Key::Num3,
            egui::Key::Num4,
            egui::Key::Num5,
            egui::Key::Num6,
            egui::Key::Num7,
            egui::Key::Num8,
            egui::Key::Num9,
        ]
        .into_iter()
        .enumerate()
        {
            if !typing && ctx.input(|i| i.key_pressed(key)) {
                action = Some(Action::Teleport(index));
            }
        }

        let hints = Rect::from_min_size(
            Pos2::new(content.left(), content.bottom() - 30.0 * scale),
            Vec2::new(content.width(), 30.0 * scale),
        );
        painter.rect_filled(hints, 0.0, PANEL);
        let mut at = hints.left_top() + Vec2::new(12.0, 7.0) * scale;
        for text in [
            "Right-click  Waypoint",
            "Drag / WASD  Move",
            "Scroll / PgUp PgDn  Zoom",
            "C  Find me",
            "Backspace  Remove waypoint",
        ] {
            at = hint(&painter, at, text, scale);
        }
        self.map_view = Some((view.center, view.zoom));
        action
    }
}
