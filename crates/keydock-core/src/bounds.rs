//! Pure window-boundary math. Kept free of GUI dependencies so multi-monitor,
//! negative-origin and mixed-DPI layouts are covered by unit tests.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(self) -> i32 {
        self.x + self.width
    }

    pub fn bottom(self) -> i32 {
        self.y + self.height
    }

    pub fn center(self) -> (i32, i32) {
        (self.x + self.width / 2, self.y + self.height / 2)
    }

    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    fn distance_squared(self, x: i32, y: i32) -> i64 {
        let dx = if x < self.x {
            self.x - x
        } else if x >= self.right() {
            x - self.right() + 1
        } else {
            0
        };
        let dy = if y < self.y {
            self.y - y
        } else if y >= self.bottom() {
            y - self.bottom() + 1
        } else {
            0
        };
        i64::from(dx) * i64::from(dx) + i64::from(dy) * i64::from(dy)
    }
}

/// Choose the work area containing the point, else the nearest one.
pub fn work_area_for(point: (i32, i32), works: &[Rect]) -> Option<Rect> {
    works
        .iter()
        .find(|work| work.contains(point.0, point.1))
        .copied()
        .or_else(|| {
            works
                .iter()
                .copied()
                .min_by_key(|work| work.distance_squared(point.0, point.1))
        })
}

pub fn clamp(value: i32, low: i32, high: i32) -> i32 {
    if high < low {
        low
    } else {
        value.max(low).min(high)
    }
}

/// Clamp a window of `size` into a work area.
pub fn clamp_window(position: (i32, i32), size: (i32, i32), work: Rect) -> (i32, i32) {
    let x = clamp(position.0, work.x, work.right().saturating_sub(size.0));
    let y = clamp(position.1, work.y, work.bottom().saturating_sub(size.1));
    (x, y)
}

/// Expand a panel from the floating ball.
///
/// Default direction is right/bottom (panel's top-left offset from the ball).
/// When the panel would cross the work area's right edge it flips left; when it
/// would cross the bottom edge it flips up. The result is clamped last so a
/// panel larger than the work area stays aligned to the top-left.
pub fn place_panel(ball: Rect, panel: (i32, i32), gap: i32, works: &[Rect]) -> Rect {
    let (cx, cy) = ball.center();
    let work = work_area_for((cx, cy), works).unwrap_or(Rect::new(
        0,
        0,
        panel.0.max(ball.width),
        panel.1.max(ball.height),
    ));

    let mut x = ball.right() + gap;
    if x + panel.0 > work.right() {
        x = ball.x - gap - panel.0;
    }
    let mut y = ball.bottom() + gap;
    if y + panel.1 > work.bottom() {
        y = ball.y - gap - panel.1;
    }

    let (x, y) = clamp_window((x, y), panel, work);
    Rect::new(x, y, panel.0, panel.1)
}

/// Place the floating ball, keeping its full footprint inside the work area.
pub fn place_ball(desired: (i32, i32), size: (i32, i32), works: &[Rect]) -> (i32, i32) {
    let work = work_area_for(desired, works).unwrap_or_else(|| {
        works
            .first()
            .copied()
            .unwrap_or(Rect::new(0, 0, 1920, 1080))
    });
    clamp_window(desired, size, work)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIMARY: Rect = Rect::new(0, 0, 2560, 1440);
    const LEFT: Rect = Rect::new(-1920, 0, 1920, 900);
    const UPPER_DPI: Rect = Rect::new(2560, 0, 1920, 1080);

    #[test]
    fn panel_opens_right_and_below_the_ball() {
        let ball = Rect::new(100, 100, 72, 72);
        let panel = place_panel(ball, (360, 600), 8, &[PRIMARY]);
        assert_eq!(panel.x, 180);
        assert_eq!(panel.y, 180);
    }

    #[test]
    fn panel_flips_left_near_right_edge() {
        let ball = Rect::new(2480, 200, 72, 72);
        let panel = place_panel(ball, (360, 600), 8, &[PRIMARY]);
        assert!(panel.right() <= PRIMARY.right());
        assert!(panel.x < ball.x);
    }

    #[test]
    fn panel_flips_up_near_bottom_edge() {
        let ball = Rect::new(100, 1350, 72, 72);
        let panel = place_panel(ball, (360, 600), 8, &[PRIMARY]);
        assert!(panel.bottom() <= PRIMARY.bottom());
        assert!(panel.y < ball.y);
    }

    #[test]
    fn panel_uses_negative_origin_monitor() {
        let ball = Rect::new(-1880, 100, 72, 72);
        let panel = place_panel(ball, (360, 600), 8, &[PRIMARY, LEFT]);
        assert!(panel.x >= LEFT.x);
        assert!(panel.right() <= LEFT.right());
        assert!(panel.y >= LEFT.y);
        assert!(panel.bottom() <= LEFT.bottom());
    }

    #[test]
    fn panel_uses_mixed_dpi_monitor_to_the_right() {
        let ball = Rect::new(2600, 100, 72, 72);
        let panel = place_panel(ball, (360, 600), 8, &[PRIMARY, UPPER_DPI]);
        assert_eq!(panel.x, 2680);
        assert!(panel.right() <= UPPER_DPI.right());
    }

    #[test]
    fn panel_smaller_work_area_is_top_left_clamped() {
        let tiny = Rect::new(0, 0, 200, 200);
        let ball = Rect::new(10, 10, 72, 72);
        let panel = place_panel(ball, (360, 600), 8, &[tiny]);
        assert_eq!((panel.x, panel.y), (0, 0));
    }

    #[test]
    fn ball_clamps_into_work_area() {
        let position = place_ball((-100, -50), (72, 72), &[PRIMARY]);
        assert_eq!(position, (0, 0));
        let position = place_ball((2500, 1400), (72, 72), &[PRIMARY]);
        assert_eq!(position, (2488, 1368));
    }

    #[test]
    fn work_area_picks_nearest_for_off_screen_point() {
        let area = work_area_for((-3000, 100), &[PRIMARY, LEFT]).unwrap();
        assert_eq!(area, LEFT);
    }
}
