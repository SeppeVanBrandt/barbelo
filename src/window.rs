use std::collections::HashMap;

use anyhow::Result;

use crate::layout::{Direction, Layout, Node, Rect, Split, neighbor};
use crate::pane::{Pane, PaneContext, PaneId};

pub struct Window {
    root: Node,
    pub panes: HashMap<PaneId, Pane>,
    pub active: PaneId,
}

impl Window {
    pub fn new(id: PaneId, area: Rect, ctx: PaneContext) -> Result<Self> {
        let pane = Pane::spawn(id, area.h, area.w, ctx)?;
        Ok(Self {
            root: Node::Leaf(id),
            panes: HashMap::from([(id, pane)]),
            active: id,
        })
    }

    pub fn layout(&self, area: Rect) -> Layout {
        self.root.layout(area)
    }

    pub fn active_pane(&mut self) -> &mut Pane {
        self.panes.get_mut(&self.active).expect("active pane exists")
    }

    pub fn split(&mut self, id: PaneId, split: Split, area: Rect, ctx: PaneContext) -> Result<()> {
        self.root.split(self.active, id, split);
        let rect = self.layout(area).rect_of(id).expect("new pane in layout");
        match Pane::spawn(id, rect.h, rect.w, ctx) {
            Ok(pane) => {
                self.panes.insert(id, pane);
                self.active = id;
                self.resize(area);
                Ok(())
            }
            Err(e) => {
                self.take_root(|root| root.remove(id));
                Err(e)
            }
        }
    }

    /// Closes a pane. Returns `true` when the window has no panes left.
    pub fn remove_pane(&mut self, id: PaneId, area: Rect) -> bool {
        self.panes.remove(&id);
        let empty = self.take_root(|root| root.remove(id));
        if empty {
            return true;
        }
        if self.active == id {
            self.active = self.root.first_leaf();
        }
        self.resize(area);
        false
    }

    pub fn resize(&mut self, area: Rect) {
        for (id, r) in self.layout(area).panes {
            if let Some(pane) = self.panes.get_mut(&id) {
                pane.resize(r.h, r.w);
            }
        }
    }

    pub fn focus(&mut self, dir: Direction, area: Rect) {
        let layout = self.layout(area);
        let Some(rect) = layout.rect_of(self.active) else {
            return;
        };
        let (row, col) = self.active_pane().parser.lock().unwrap().screen().cursor_position();
        let hint = match dir {
            Direction::Left | Direction::Right => rect.y + row,
            Direction::Up | Direction::Down => rect.x + col,
        };
        if let Some(id) = neighbor(&layout, self.active, dir, hint) {
            self.active = id;
        }
    }

    pub fn focus_next(&mut self, area: Rect) {
        let ids: Vec<PaneId> = self.layout(area).panes.iter().map(|(id, _)| *id).collect();
        if let Some(pos) = ids.iter().position(|id| *id == self.active) {
            self.active = ids[(pos + 1) % ids.len()];
        }
    }

    pub fn name(&self) -> String {
        self.panes
            .get(&self.active)
            .and_then(Pane::process_name)
            .unwrap_or_else(|| "shell".into())
    }

    /// Applies `f` to the tree; returns `true` if the tree became empty.
    fn take_root(&mut self, f: impl FnOnce(Node) -> Option<Node>) -> bool {
        let root = std::mem::replace(&mut self.root, Node::Leaf(PaneId::MAX));
        match f(root) {
            Some(root) => {
                self.root = root;
                false
            }
            None => true,
        }
    }
}
