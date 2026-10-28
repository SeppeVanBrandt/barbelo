use serde::Deserialize;

use crate::pane::PaneId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Rect {
    pub fn right(&self) -> u16 {
        self.x + self.w
    }

    pub fn bottom(&self) -> u16 {
        self.y + self.h
    }

    /// The rect shrunk by one cell on every side.
    fn inset(&self) -> Rect {
        Rect {
            x: self.x + 1,
            y: self.y + 1,
            w: self.w.saturating_sub(2),
            h: self.h.saturating_sub(2),
        }
    }
}

/// How panes are separated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BorderStyle {
    /// One line between neighbouring panes, shared by both.
    #[default]
    Shared,
    /// A box around every pane; neighbours' boxes touch.
    Boxed,
}

/// How a split arranges its two children.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Split {
    /// Children side by side, separated by a vertical border.
    Horizontal,
    /// Children stacked on top of each other, separated by a horizontal border.
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// Binary tree of panes. Every split divides its area evenly.
#[derive(Debug)]
pub enum Node {
    Leaf(PaneId),
    Split {
        split: Split,
        first: Box<Node>,
        second: Box<Node>,
    },
}

/// A one-cell-wide line separating two panes.
#[derive(Clone, Copy, Debug)]
pub enum Border {
    Vertical { x: u16, y: u16, len: u16 },
    Horizontal { x: u16, y: u16, len: u16 },
}

pub struct Layout {
    pub panes: Vec<(PaneId, Rect)>,
    pub borders: Vec<Border>,
    /// Box around each pane, with boxed borders.
    pub frames: Vec<(PaneId, Rect)>,
    /// Cells between the contents of two neighbouring panes.
    pub gap: u16,
}

impl Layout {
    pub fn rect_of(&self, id: PaneId) -> Option<Rect> {
        self.panes.iter().find(|(p, _)| *p == id).map(|(_, r)| *r)
    }
}

impl Node {
    pub fn layout(&self, area: Rect, style: BorderStyle) -> Layout {
        let mut out = Layout {
            panes: Vec::new(),
            borders: Vec::new(),
            frames: Vec::new(),
            gap: 1,
        };
        match style {
            BorderStyle::Shared => self.layout_into(area, &mut out, true),
            BorderStyle::Boxed => {
                // Split without separators, then draw a box inside each part.
                self.layout_into(area, &mut out, false);
                out.gap = 2;
                for (id, rect) in &mut out.panes {
                    out.frames.push((*id, *rect));
                    *rect = rect.inset();
                }
            }
        }
        out
    }

    fn layout_into(&self, area: Rect, out: &mut Layout, separators: bool) {
        match self {
            Node::Leaf(id) => out.panes.push((*id, area)),
            Node::Split {
                split,
                first,
                second,
            } => {
                let (a, b, border) = split_rect(area, *split, separators);
                first.layout_into(a, out, separators);
                second.layout_into(b, out, separators);
                if separators {
                    out.borders.push(border);
                }
            }
        }
    }

    /// Replaces the leaf `target` with a split containing `target` and `new`.
    pub fn split(&mut self, target: PaneId, new: PaneId, split: Split) -> bool {
        match self {
            Node::Leaf(id) if *id == target => {
                *self = Node::Split {
                    split,
                    first: Box::new(Node::Leaf(target)),
                    second: Box::new(Node::Leaf(new)),
                };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { first, second, .. } => {
                first.split(target, new, split) || second.split(target, new, split)
            }
        }
    }

    /// Removes the leaf `target`, collapsing its parent split into the
    /// sibling. Returns `None` when nothing is left.
    pub fn remove(self, target: PaneId) -> Option<Node> {
        match self {
            Node::Leaf(id) if id == target => None,
            Node::Leaf(_) => Some(self),
            Node::Split {
                split,
                first,
                second,
            } => match (first.remove(target), second.remove(target)) {
                (Some(a), Some(b)) => Some(Node::Split {
                    split,
                    first: Box::new(a),
                    second: Box::new(b),
                }),
                (Some(n), None) | (None, Some(n)) => Some(n),
                (None, None) => None,
            },
        }
    }

    pub fn first_leaf(&self) -> PaneId {
        match self {
            Node::Leaf(id) => *id,
            Node::Split { first, .. } => first.first_leaf(),
        }
    }
}

/// Divides `area` in two, leaving a column or row for the border between
/// them if `separator` is set.
fn split_rect(area: Rect, split: Split, separator: bool) -> (Rect, Rect, Border) {
    let gap = u16::from(separator);
    match split {
        Split::Horizontal => {
            let avail = area.w.saturating_sub(gap);
            let w1 = avail.div_ceil(2);
            let w2 = avail - w1;
            let a = Rect { w: w1, ..area };
            let b = Rect {
                x: area.x + w1 + gap,
                w: w2,
                ..area
            };
            let border = Border::Vertical {
                x: area.x + w1,
                y: area.y,
                len: area.h,
            };
            (a, b, border)
        }
        Split::Vertical => {
            let avail = area.h.saturating_sub(gap);
            let h1 = avail.div_ceil(2);
            let h2 = avail - h1;
            let a = Rect { h: h1, ..area };
            let b = Rect {
                y: area.y + h1 + gap,
                h: h2,
                ..area
            };
            let border = Border::Horizontal {
                x: area.x,
                y: area.y + h1,
                len: area.w,
            };
            (a, b, border)
        }
    }
}

/// Finds the pane adjacent to `from` in direction `dir`. When several panes
/// border it, the one overlapping `hint` (a row or column inside `from`) wins.
pub fn neighbor(layout: &Layout, from: PaneId, dir: Direction, hint: u16) -> Option<PaneId> {
    let a = layout.rect_of(from)?;
    let gap = layout.gap;
    let overlap = |lo1: u16, hi1: u16, lo2: u16, hi2: u16| lo1 < hi2 && lo2 < hi1;
    let candidates: Vec<(PaneId, Rect)> = layout
        .panes
        .iter()
        .copied()
        .filter(|(id, r)| {
            *id != from
                && match dir {
                    Direction::Left => r.right() + gap == a.x && overlap(r.y, r.bottom(), a.y, a.bottom()),
                    Direction::Right => a.right() + gap == r.x && overlap(r.y, r.bottom(), a.y, a.bottom()),
                    Direction::Up => r.bottom() + gap == a.y && overlap(r.x, r.right(), a.x, a.right()),
                    Direction::Down => a.bottom() + gap == r.y && overlap(r.x, r.right(), a.x, a.right()),
                }
        })
        .collect();
    let contains_hint = |r: &Rect| match dir {
        Direction::Left | Direction::Right => (r.y..r.bottom()).contains(&hint),
        Direction::Up | Direction::Down => (r.x..r.right()).contains(&hint),
    };
    candidates
        .iter()
        .find(|(_, r)| contains_hint(r))
        .or(candidates.first())
        .map(|(id, _)| *id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect {
        x: 0,
        y: 0,
        w: 81,
        h: 24,
    };

    #[test]
    fn split_and_remove() {
        let mut root = Node::Leaf(0);
        assert!(root.split(0, 1, Split::Horizontal));
        assert!(root.split(1, 2, Split::Vertical));
        let layout = root.layout(AREA, BorderStyle::Shared);
        assert_eq!(layout.rect_of(0), Some(Rect { x: 0, y: 0, w: 40, h: 24 }));
        assert_eq!(layout.rect_of(1), Some(Rect { x: 41, y: 0, w: 40, h: 12 }));
        assert_eq!(layout.rect_of(2), Some(Rect { x: 41, y: 13, w: 40, h: 11 }));
        assert_eq!(layout.borders.len(), 2);

        let root = root.remove(1).unwrap();
        let layout = root.layout(AREA, BorderStyle::Shared);
        assert_eq!(layout.rect_of(2), Some(Rect { x: 41, y: 0, w: 40, h: 24 }));
        assert!(root.remove(0).unwrap().remove(2).is_none());
    }

    #[test]
    fn neighbors() {
        let mut root = Node::Leaf(0);
        root.split(0, 1, Split::Horizontal);
        root.split(1, 2, Split::Vertical);
        let layout = root.layout(AREA, BorderStyle::Shared);
        assert_eq!(neighbor(&layout, 0, Direction::Right, 20), Some(2));
        assert_eq!(neighbor(&layout, 0, Direction::Right, 3), Some(1));
        assert_eq!(neighbor(&layout, 2, Direction::Up, 50), Some(1));
        assert_eq!(neighbor(&layout, 2, Direction::Left, 15), Some(0));
        assert_eq!(neighbor(&layout, 0, Direction::Left, 0), None);
    }

    #[test]
    fn boxed_panes() {
        let mut root = Node::Leaf(0);
        root.split(0, 1, Split::Horizontal);
        root.split(1, 2, Split::Vertical);
        let layout = root.layout(AREA, BorderStyle::Boxed);
        assert!(layout.borders.is_empty());
        assert_eq!(layout.frames[0], (0, Rect { x: 0, y: 0, w: 41, h: 24 }));
        assert_eq!(layout.rect_of(0), Some(Rect { x: 1, y: 1, w: 39, h: 22 }));
        assert_eq!(layout.rect_of(1), Some(Rect { x: 42, y: 1, w: 38, h: 10 }));
        assert_eq!(layout.rect_of(2), Some(Rect { x: 42, y: 13, w: 38, h: 10 }));
        assert_eq!(neighbor(&layout, 0, Direction::Right, 20), Some(2));
        assert_eq!(neighbor(&layout, 1, Direction::Down, 50), Some(2));
        assert_eq!(neighbor(&layout, 2, Direction::Left, 15), Some(0));
    }
}
