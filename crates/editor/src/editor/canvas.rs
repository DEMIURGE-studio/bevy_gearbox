use bevy_egui::egui;

#[derive(Debug, Clone, Copy)]
pub struct CanvasTransform {
    pub pan: egui::Vec2,
    pub zoom: f32,
}

impl Default for CanvasTransform {
    fn default() -> Self {
        Self { pan: egui::vec2(200.0, 0.0), zoom: 1.0 }
    }
}

impl CanvasTransform {
    pub fn to_screen(&self, world: egui::Pos2) -> egui::Pos2 {
        egui::Pos2::new(world.x * self.zoom + self.pan.x, world.y * self.zoom + self.pan.y)
    }
    pub fn to_world(&self, screen: egui::Pos2) -> egui::Pos2 {
        egui::Pos2::new((screen.x - self.pan.x) / self.zoom, (screen.y - self.pan.y) / self.zoom)
    }

    pub fn pan_screen_delta(&mut self, screen_delta: egui::Vec2) {
        self.pan += screen_delta;
    }

    pub fn zoom_around_screen_point(&mut self, factor: f32, screen_point: egui::Pos2) {
        let factor = factor.clamp(0.25, 4.0);
        let world_at_cursor = self.to_world(screen_point);
        self.zoom = (self.zoom * factor).clamp(0.1, 8.0);
        // Keep world_at_cursor fixed under the cursor by adjusting pan
        let desired = egui::pos2(world_at_cursor.x * self.zoom, world_at_cursor.y * self.zoom);
        self.pan = screen_point - desired;
    }
}


