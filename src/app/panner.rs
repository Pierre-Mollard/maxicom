#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PanStatus {
    pub left: usize,
    pub right: usize,
    pub has_unexplored_right: bool,
}

impl PanStatus {
    pub fn is_panned(&self) -> bool {
        self.left > 0 || self.right > 0
    }
}

// TODO: use this for pan displays
