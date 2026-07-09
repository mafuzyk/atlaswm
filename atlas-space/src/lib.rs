use std::collections::HashMap;

pub type Coordinate = f64;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: Coordinate,
    pub y: Coordinate,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub width: Coordinate,
    pub height: Coordinate,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: Coordinate,
    pub y: Coordinate,
    pub width: Coordinate,
    pub height: Coordinate,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VisualRegion {
    pub id: u64,
    pub name: String,
    pub rect: Rect,
    pub z_index: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Viewport {
    pub output_name: String,
    pub x: Coordinate,
    pub y: Coordinate,
    pub zoom: Coordinate,
}

#[derive(Debug)]
pub struct GlobalSpace {
    regions: HashMap<u64, VisualRegion>,
    next_id: u64,
}

impl GlobalSpace {
    pub fn new() -> Self {
        GlobalSpace {
            regions: HashMap::new(),
            next_id: 1,
        }
    }

    pub fn add_region(&mut self, name: String, rect: Rect) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.regions.insert(
            id,
            VisualRegion {
                id,
                name,
                rect,
                z_index: 0,
            },
        );
        id
    }

    pub fn region(&self, id: u64) -> Option<&VisualRegion> {
        self.regions.get(&id)
    }

    pub fn regions(&self) -> impl Iterator<Item = &VisualRegion> {
        self.regions.values()
    }

    pub fn canvas_to_screen(&self, pos: Point, viewport: &Viewport) -> Point {
        Point {
            x: (pos.x - viewport.x) * viewport.zoom,
            y: (pos.y - viewport.y) * viewport.zoom,
        }
    }

    pub fn screen_to_canvas(&self, pos: Point, viewport: &Viewport) -> Point {
        Point {
            x: pos.x / viewport.zoom + viewport.x,
            y: pos.y / viewport.zoom + viewport.y,
        }
    }
}
