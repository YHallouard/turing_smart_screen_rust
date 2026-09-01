//! Dirty-region accumulation.
//!
//! The engine reports the bounding boxes of layers that changed; this merges
//! overlapping / touching boxes so a backend sends a handful of regions instead
//! of dozens of tiny ones.

use crate::Rect;

#[derive(Debug, Default)]
pub struct DirtyTracker {
    regions: Vec<Rect>,
}

impl DirtyTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.regions.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    pub fn regions(&self) -> &[Rect] {
        &self.regions
    }

    /// Add a rectangle, merging it into any existing region it touches.
    pub fn add(&mut self, r: Rect) {
        if r.w == 0 || r.h == 0 {
            return;
        }
        let mut merged = r;
        let mut i = 0;
        while i < self.regions.len() {
            if touches(self.regions[i], merged) {
                merged = self.regions.swap_remove(i).union(merged);
                i = 0; // restart: the bigger box may now touch earlier ones
            } else {
                i += 1;
            }
        }
        self.regions.push(merged);
    }
}

/// True if the rectangles overlap or share an edge.
fn touches(a: Rect, b: Rect) -> bool {
    let ax1 = a.x as i32 + a.w as i32;
    let ay1 = a.y as i32 + a.h as i32;
    let bx1 = b.x as i32 + b.w as i32;
    let by1 = b.y as i32 + b.h as i32;
    a.x as i32 <= bx1 && b.x as i32 <= ax1 && a.y as i32 <= by1 && b.y as i32 <= ay1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_overlapping() {
        let mut t = DirtyTracker::new();
        t.add(Rect::new(0, 0, 10, 10));
        t.add(Rect::new(5, 5, 10, 10));
        assert_eq!(t.regions(), &[Rect::new(0, 0, 15, 15)]);
    }

    #[test]
    fn keeps_disjoint() {
        let mut t = DirtyTracker::new();
        t.add(Rect::new(0, 0, 10, 10));
        t.add(Rect::new(100, 100, 10, 10));
        assert_eq!(t.regions().len(), 2);
    }
}
