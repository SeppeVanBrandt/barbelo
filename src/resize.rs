//! Resize mode: the points where borders meet are nodes, and moving a node
//! moves the borders running through it.

use crate::layout::{Border, Edge, Layout, Rect, SplitPath};

/// A point on the borders that can be moved.
#[derive(Clone, Debug, PartialEq)]
pub struct ResizeNode {
    /// Where the node is drawn.
    pub x: u16,
    pub y: u16,
    /// Where the borders meet, for finding borders that line up.
    at: (u16, u16),
    /// Splits with a vertical border through the node, moved by h and l.
    pub vertical: Vec<SplitPath>,
    /// Splits with a horizontal border through the node, moved by j and k.
    pub horizontal: Vec<SplitPath>,
}

/// The nodes of `layout`.
pub fn layout_nodes(layout: &Layout) -> Vec<ResizeNode> {
    nodes(&layout.edges, !layout.frames.is_empty())
}

/// Nodes where a vertical and a horizontal border meet, plus one halfway
/// along every border that meets no other, ordered top to bottom. With
/// `boxed` borders, a node is drawn on the frame its border belongs to.
pub fn nodes(edges: &[Edge], boxed: bool) -> Vec<ResizeNode> {
    let mut nodes: Vec<ResizeNode> = Vec::new();
    let mut met = vec![false; edges.len()];
    for (i, v) in edges.iter().enumerate() {
        let Border::Vertical { x: vx, y: vy, len: vlen } = v.border else {
            continue;
        };
        for (j, h) in edges.iter().enumerate() {
            let Border::Horizontal { x: hx, y: hy, len: hlen } = h.border else {
                continue;
            };
            // A border that ends where the other one runs counts as meeting
            // it, from either side.
            if vx + 1 >= hx && vx <= hx + hlen && hy + 1 >= vy && hy <= vy + vlen {
                // A boxed edge is the first cell of the second half, so a
                // border ending at it from the left or above belongs one
                // cell back.
                let x = if boxed && hx + hlen == vx { vx - 1 } else { vx };
                let y = if boxed && vy + vlen == hy { hy - 1 } else { hy };
                add(&mut nodes, (vx, hy), (x, y), Some(&v.path), Some(&h.path));
                met[i] = true;
                met[j] = true;
            }
        }
    }
    for (edge, _) in edges.iter().zip(met).filter(|(_, met)| !met) {
        match edge.border {
            Border::Vertical { x, y, len } => {
                let at = (x, y + len / 2);
                add(&mut nodes, at, at, Some(&edge.path), None)
            }
            Border::Horizontal { x, y, len } => {
                let at = (x + len / 2, y);
                add(&mut nodes, at, at, None, Some(&edge.path))
            }
        }
    }
    nodes.sort_by_key(|n| (n.y, n.x));
    nodes
}

/// Adds the splits to the node where borders meet `at`, creating it if
/// needed. Borders that happen to line up share a node, and move together.
fn add(
    nodes: &mut Vec<ResizeNode>,
    at: (u16, u16),
    (x, y): (u16, u16),
    v: Option<&SplitPath>,
    h: Option<&SplitPath>,
) {
    let i = match nodes.iter().position(|n| n.at == at) {
        Some(i) => i,
        None => {
            nodes.push(ResizeNode {
                x,
                y,
                at,
                vertical: Vec::new(),
                horizontal: Vec::new(),
            });
            nodes.len() - 1
        }
    };
    let node = &mut nodes[i];
    for (paths, path) in [(&mut node.vertical, v), (&mut node.horizontal, h)] {
        if let Some(path) = path
            && !paths.contains(path)
        {
            paths.push(path.clone());
        }
    }
}

/// The node being moved. It is remembered by its splits rather than its
/// position, which changes as it moves.
pub struct ResizeMode {
    pub vertical: Vec<SplitPath>,
    pub horizontal: Vec<SplitPath>,
}

impl ResizeMode {
    /// Starts at the node nearest the middle of `active`.
    pub fn new(nodes: &[ResizeNode], active: Rect) -> Option<Self> {
        let (cx, cy) = (active.x + active.w / 2, active.y + active.h / 2);
        let node = nodes
            .iter()
            .min_by_key(|n| n.x.abs_diff(cx) as u32 + n.y.abs_diff(cy) as u32)?;
        Some(Self::of(node))
    }

    fn of(node: &ResizeNode) -> Self {
        Self {
            vertical: node.vertical.clone(),
            horizontal: node.horizontal.clone(),
        }
    }

    /// Index of the selected node in `nodes`. If borders have come to line
    /// up, that is the node they now share.
    pub fn selected(&self, nodes: &[ResizeNode]) -> Option<usize> {
        nodes.iter().position(|n| {
            self.vertical.iter().all(|p| n.vertical.contains(p))
                && self.horizontal.iter().all(|p| n.horizontal.contains(p))
        })
    }

    /// Selects the next node, or the previous one.
    pub fn cycle(&mut self, nodes: &[ResizeNode], forward: bool) {
        if nodes.is_empty() {
            return;
        }
        let next = match (self.selected(nodes), forward) {
            (Some(i), true) => (i + 1) % nodes.len(),
            (Some(i), false) => (i + nodes.len() - 1) % nodes.len(),
            (None, _) => 0,
        };
        *self = Self::of(&nodes[next]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{BorderStyle, Node, Split};

    const AREA: Rect = Rect {
        x: 0,
        y: 0,
        w: 81,
        h: 24,
    };

    fn layout_nodes(root: &Node, style: BorderStyle) -> Vec<ResizeNode> {
        nodes(&root.layout(AREA, style).edges, style == BorderStyle::Boxed)
    }

    #[test]
    fn lone_border_gets_a_node_halfway() {
        let mut root = Node::Leaf(0);
        root.split(0, 1, Split::Horizontal);
        let nodes = layout_nodes(&root, BorderStyle::Shared);
        assert_eq!(nodes.len(), 1);
        assert_eq!((nodes[0].x, nodes[0].y), (40, 12));
        assert_eq!(nodes[0].vertical, [Vec::<bool>::new()]);
        assert!(nodes[0].horizontal.is_empty());
    }

    #[test]
    fn junctions_combine_both_borders() {
        // Left pane, and the right half split into top and bottom: ├ at the
        // junction.
        let mut root = Node::Leaf(0);
        root.split(0, 1, Split::Horizontal);
        root.split(1, 2, Split::Vertical);
        for style in [BorderStyle::Shared, BorderStyle::Boxed] {
            let nodes = layout_nodes(&root, style);
            assert_eq!(nodes.len(), 1, "{style:?}");
            assert_eq!(nodes[0].vertical, [vec![]]);
            assert_eq!(nodes[0].horizontal, [vec![true]]);
        }
    }

    #[test]
    fn aligned_borders_share_a_node() {
        // Both halves split into top and bottom: ┼ in the middle.
        let mut root = Node::Leaf(0);
        root.split(0, 1, Split::Horizontal);
        root.split(0, 2, Split::Vertical);
        root.split(1, 3, Split::Vertical);
        for style in [BorderStyle::Shared, BorderStyle::Boxed] {
            let nodes = layout_nodes(&root, style);
            assert_eq!(nodes.len(), 1, "{style:?}");
            assert_eq!(nodes[0].horizontal.len(), 2);
        }
    }

    #[test]
    fn selection_follows_the_splits() {
        let mut root = Node::Leaf(0);
        root.split(0, 1, Split::Horizontal);
        root.split(1, 2, Split::Vertical);
        root.split(0, 3, Split::Vertical);
        // Move the right half's border up, so the two don't line up.
        root.move_split(&[true], -7, AREA, BorderStyle::Shared);
        let nodes = layout_nodes(&root, BorderStyle::Shared);
        assert_eq!(nodes.len(), 2);
        assert_eq!((nodes[0].x, nodes[0].y), (40, 5));
        let mut mode = ResizeMode::new(&nodes, Rect { x: 0, y: 0, w: 40, h: 12 }).unwrap();
        assert_eq!(mode.selected(&nodes), Some(0));
        mode.cycle(&nodes, true);
        assert_eq!(mode.selected(&nodes), Some(1));
        mode.cycle(&nodes, true);
        assert_eq!(mode.selected(&nodes), Some(0));
        mode.cycle(&nodes, false);
        assert_eq!(mode.selected(&nodes), Some(1));
    }
}
