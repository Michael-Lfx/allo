//! The flow document: nodes, connections, and the operations the editor
//! performs on them.
//!
//! The model is deliberately independent of gpui. Every rule that could lose a
//! user's work, such as rejecting a duplicate connection or removing incident
//! edges with a deleted node, lives here where it can be tested directly.

use crate::geometry::{EdgeGeometry, Rect, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fmt;

/// The width of every node in world units.
pub const NODE_WIDTH: f32 = 260.0;
/// The height of every node in world units.
pub const NODE_HEIGHT: f32 = 132.0;

/// A stable node identifier.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
)]
pub struct NodeId(pub u64);

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "N{}", self.0)
    }
}

/// A stable connection identifier.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
)]
pub struct EdgeId(pub u64);

impl fmt::Display for EdgeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "E{}", self.0)
    }
}

/// The role a node plays in a flow.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// Starts a flow in response to an event.
    Trigger,
    /// Performs one step.
    Action,
    /// Branches on a predicate.
    Condition,
    /// Waits before continuing.
    Delay,
    /// Reshapes data between steps.
    Transform,
    /// Ends a flow.
    Output,
}

impl NodeKind {
    /// Every kind, in the order the library presents them.
    pub const ALL: [Self; 6] = [
        Self::Trigger,
        Self::Action,
        Self::Condition,
        Self::Delay,
        Self::Transform,
        Self::Output,
    ];

    /// A human-readable name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Trigger => "Trigger",
            Self::Action => "Action",
            Self::Condition => "Condition",
            Self::Delay => "Delay",
            Self::Transform => "Transform",
            Self::Output => "Output",
        }
    }

    /// A two-letter abbreviation used by compact node badges.
    #[must_use]
    pub const fn abbreviation(self) -> &'static str {
        match self {
            Self::Trigger => "TR",
            Self::Action => "AC",
            Self::Condition => "IF",
            Self::Delay => "DL",
            Self::Transform => "TX",
            Self::Output => "OUT",
        }
    }

    /// A short explanation shown in the library.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Trigger => "Start when an event happens",
            Self::Action => "Do a unit of work",
            Self::Condition => "Branch on a rule",
            Self::Delay => "Wait before continuing",
            Self::Transform => "Map or reshape data",
            Self::Output => "Deliver the final result",
        }
    }

    /// The title a new node of this kind receives.
    #[must_use]
    pub const fn default_title(self) -> &'static str {
        match self {
            Self::Trigger => "New trigger",
            Self::Action => "New action",
            Self::Condition => "New condition",
            Self::Delay => "New delay",
            Self::Transform => "New transform",
            Self::Output => "New output",
        }
    }

    /// The description a new node of this kind receives.
    #[must_use]
    pub const fn default_description(self) -> &'static str {
        match self {
            Self::Trigger => "Choose the event that starts this flow.",
            Self::Action => "Describe the work this step performs.",
            Self::Condition => "Describe the rule that decides the route.",
            Self::Delay => "Choose how long the flow waits.",
            Self::Transform => "Describe how data changes here.",
            Self::Output => "Choose where the result is delivered.",
        }
    }
}

/// One step in a flow.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// Stable identifier.
    pub id: NodeId,
    /// The node's role.
    pub kind: NodeKind,
    /// The user-facing title.
    pub title: String,
    /// A longer explanation of the step.
    #[serde(default)]
    pub description: String,
    /// Top-left corner in world coordinates.
    pub position: Vec2,
}

impl Node {
    /// Creates a node with the default title and description for its kind.
    #[must_use]
    pub fn new(id: NodeId, kind: NodeKind, position: Vec2) -> Self {
        Self {
            id,
            kind,
            title: kind.default_title().to_string(),
            description: kind.default_description().to_string(),
            position,
        }
    }

    /// Returns the node's world-space rectangle.
    #[must_use]
    pub fn bounds(&self) -> Rect {
        Rect::new(self.position.x, self.position.y, NODE_WIDTH, NODE_HEIGHT)
    }

    /// Returns the center of the node.
    #[must_use]
    pub fn center(&self) -> Vec2 {
        self.bounds().center()
    }

    /// Returns the world position of the incoming port.
    #[must_use]
    pub fn input_port(&self) -> Vec2 {
        Vec2::new(self.position.x, self.position.y + NODE_HEIGHT / 2.0)
    }

    /// Returns the world position of the outgoing port.
    #[must_use]
    pub fn output_port(&self) -> Vec2 {
        Vec2::new(
            self.position.x + NODE_WIDTH,
            self.position.y + NODE_HEIGHT / 2.0,
        )
    }
}

/// A directed connection between two nodes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    /// Stable identifier.
    pub id: EdgeId,
    /// The node data flows from.
    pub from: NodeId,
    /// The node data flows to.
    pub to: NodeId,
}

/// A copy of selected nodes and the connections between them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ClipboardData {
    /// Nodes, positioned relative to the top-left of the copied selection.
    pub nodes: Vec<ClipboardNode>,
    /// Connections whose endpoints are both in `nodes`, as indices into it.
    pub edges: Vec<ClipboardEdge>,
}

/// One node in a [`ClipboardData`] payload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClipboardNode {
    /// The node's role.
    pub kind: NodeKind,
    /// The user-facing title.
    pub title: String,
    /// The longer explanation.
    pub description: String,
    /// Position relative to the copied selection.
    pub position: Vec2,
}

/// One connection in a [`ClipboardData`] payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipboardEdge {
    /// Index of the source node.
    pub from: usize,
    /// Index of the destination node.
    pub to: usize,
}

/// Why a connection could not be created or changed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GraphError {
    /// A node referenced by the operation does not exist.
    #[error("node {0} does not exist")]
    MissingNode(NodeId),
    /// A flow cannot connect a node to itself.
    #[error("a node cannot connect to itself")]
    SelfLoop,
    /// The connection already exists.
    #[error("these nodes are already connected")]
    DuplicateEdge,
}

/// A structural problem found by [`Document::validate`].
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    /// Two nodes share an identifier.
    #[error("duplicate node id {0}")]
    DuplicateNode(NodeId),
    /// Two connections share an identifier.
    #[error("duplicate edge id {0}")]
    DuplicateEdge(EdgeId),
    /// A connection refers to a node that is not in the document.
    #[error("edge {edge} refers to missing node {node}")]
    MissingEndpoint {
        /// The connection.
        edge: EdgeId,
        /// The missing endpoint.
        node: NodeId,
    },
    /// A node is connected to itself.
    #[error("edge {0} is a self-loop")]
    SelfLoop(EdgeId),
    /// Two connections join the same pair of nodes.
    #[error("nodes {from} and {to} are connected more than once")]
    DuplicateConnection {
        /// Source.
        from: NodeId,
        /// Destination.
        to: NodeId,
    },
    /// A coordinate is not finite.
    #[error("node {0} has a non-finite position")]
    InvalidPosition(NodeId),
}

/// How serious an [`Insight`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// A suggestion that does not prevent the flow from running.
    Info,
    /// A likely authoring mistake.
    Warning,
}

/// A statement about the shape of a flow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Insight {
    /// How serious the insight is.
    pub severity: Severity,
    /// The node the insight refers to, or `None` for a document-wide note.
    pub node: Option<NodeId>,
    /// The user-facing message.
    pub message: String,
}

/// The axis used when distributing nodes evenly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Left-to-right.
    Horizontal,
    /// Top-to-bottom.
    Vertical,
}

/// An edge of the selection bounds to align nodes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    /// Align left edges.
    Left,
    /// Align horizontal centers.
    CenterX,
    /// Align right edges.
    Right,
    /// Align top edges.
    Top,
    /// Align vertical centers.
    CenterY,
    /// Align bottom edges.
    Bottom,
}

/// A flow document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    /// The document title, shown in the title bar and the document inspector.
    pub title: String,
    /// The nodes in paint order. Later nodes render above earlier ones.
    pub nodes: Vec<Node>,
    /// The connections.
    pub edges: Vec<Edge>,
    #[serde(default)]
    next_node_id: u64,
    #[serde(default)]
    next_edge_id: u64,
}

impl Default for Document {
    fn default() -> Self {
        Self::new("Untitled flow")
    }
}

impl Document {
    /// Creates an empty document.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            nodes: Vec::new(),
            edges: Vec::new(),
            next_node_id: 1,
            next_edge_id: 1,
        }
    }

    /// Adds a node with the default title and description for `kind`.
    pub fn add_node(&mut self, kind: NodeKind, position: Vec2) -> NodeId {
        self.add_node_with(
            kind,
            kind.default_title(),
            kind.default_description(),
            position,
        )
    }

    /// Adds a node with explicit text.
    pub fn add_node_with(
        &mut self,
        kind: NodeKind,
        title: impl Into<String>,
        description: impl Into<String>,
        position: Vec2,
    ) -> NodeId {
        let id = self.allocate_node_id();
        self.nodes.push(Node {
            id,
            kind,
            title: title.into(),
            description: description.into(),
            position,
        });
        id
    }

    /// Places a node near `desired`, moving it just enough to avoid overlapping
    /// an existing node. The search is a deterministic golden-angle spiral.
    pub fn place_node(&mut self, kind: NodeKind, desired: Vec2) -> NodeId {
        let position = self.find_free_position(desired);
        self.add_node(kind, position)
    }

    /// Finds a free position near `desired`.
    #[must_use]
    pub fn find_free_position(&self, desired: Vec2) -> Vec2 {
        let desired = desired.snap(8.0);
        if self.position_is_free(desired) {
            return desired;
        }

        const GOLDEN_ANGLE: f32 = 2.399_963_1;
        for step in 1..=160 {
            let fraction = step as f32;
            let radius = 36.0 * fraction.sqrt();
            let angle = fraction * GOLDEN_ANGLE;
            let candidate = (desired
                + Vec2::new(angle.cos(), angle.sin()) * radius)
                .snap(8.0);
            if self.position_is_free(candidate) {
                return candidate;
            }
        }
        desired + Vec2::new(NODE_WIDTH + 32.0, 0.0)
    }

    fn position_is_free(&self, position: Vec2) -> bool {
        let candidate =
            Rect::new(position.x, position.y, NODE_WIDTH, NODE_HEIGHT)
                .expand(12.0);
        self.nodes
            .iter()
            .all(|node| !node.bounds().intersects(candidate))
    }

    /// Connects two nodes.
    ///
    /// # Errors
    ///
    /// Returns [`GraphError::MissingNode`], [`GraphError::SelfLoop`] or
    /// [`GraphError::DuplicateEdge`] when the connection is not valid.
    pub fn add_edge(
        &mut self,
        from: NodeId,
        to: NodeId,
    ) -> Result<EdgeId, GraphError> {
        if from == to {
            return Err(GraphError::SelfLoop);
        }
        if self.node(from).is_none() {
            return Err(GraphError::MissingNode(from));
        }
        if self.node(to).is_none() {
            return Err(GraphError::MissingNode(to));
        }
        if self
            .edges
            .iter()
            .any(|edge| edge.from == from && edge.to == to)
        {
            return Err(GraphError::DuplicateEdge);
        }

        let id = self.allocate_edge_id();
        self.edges.push(Edge { id, from, to });
        Ok(id)
    }

    /// Reverses an existing connection.
    ///
    /// # Errors
    ///
    /// Returns [`GraphError::DuplicateEdge`] if the reverse connection already
    /// exists.
    pub fn reverse_edge(&mut self, id: EdgeId) -> Result<(), GraphError> {
        let Some(edge) = self.edge(id).cloned() else {
            return Ok(());
        };
        if self.edges.iter().any(|other| {
            other.id != id && other.from == edge.to && other.to == edge.from
        }) {
            return Err(GraphError::DuplicateEdge);
        }
        if let Some(edge) = self.edge_mut(id) {
            (edge.from, edge.to) = (edge.to, edge.from);
        }
        Ok(())
    }

    /// Removes a connection. Returns whether it existed.
    pub fn remove_edge(&mut self, id: EdgeId) -> bool {
        let before = self.edges.len();
        self.edges.retain(|edge| edge.id != id);
        self.edges.len() != before
    }

    /// Removes nodes and every connection that touched them. Returns how many
    /// nodes were removed.
    pub fn remove_nodes(&mut self, ids: &BTreeSet<NodeId>) -> usize {
        let before = self.nodes.len();
        self.nodes.retain(|node| !ids.contains(&node.id));
        self.edges.retain(|edge| {
            !ids.contains(&edge.from) && !ids.contains(&edge.to)
        });
        before - self.nodes.len()
    }

    /// Moves nodes by `delta`.
    pub fn move_nodes(&mut self, ids: &BTreeSet<NodeId>, delta: Vec2) {
        for node in &mut self.nodes {
            if ids.contains(&node.id) {
                node.position += delta;
            }
        }
    }

    /// Sets a node's absolute position.
    pub fn set_node_position(&mut self, id: NodeId, position: Vec2) -> bool {
        let Some(node) = self.node_mut(id) else {
            return false;
        };
        node.position = position;
        true
    }

    /// Copies nodes and the connections among them.
    #[must_use]
    pub fn copy_nodes(&self, ids: &BTreeSet<NodeId>) -> ClipboardData {
        let selected: Vec<&Node> = self
            .nodes
            .iter()
            .filter(|node| ids.contains(&node.id))
            .collect();
        if selected.is_empty() {
            return ClipboardData::default();
        }

        let origin = selected.iter().fold(
            Vec2::new(f32::INFINITY, f32::INFINITY),
            |acc, node| {
                Vec2::new(
                    acc.x.min(node.position.x),
                    acc.y.min(node.position.y),
                )
            },
        );

        let index_by_id: BTreeMap<NodeId, usize> = selected
            .iter()
            .enumerate()
            .map(|(index, node)| (node.id, index))
            .collect();

        let nodes = selected
            .iter()
            .map(|node| ClipboardNode {
                kind: node.kind,
                title: node.title.clone(),
                description: node.description.clone(),
                position: node.position - origin,
            })
            .collect();

        let edges = self
            .edges
            .iter()
            .filter_map(|edge| {
                let from = *index_by_id.get(&edge.from)?;
                let to = *index_by_id.get(&edge.to)?;
                Some(ClipboardEdge { from, to })
            })
            .collect();

        ClipboardData { nodes, edges }
    }

    /// Pastes copied nodes with their top-left corner at `top_left`. Returns
    /// the new node ids.
    pub fn paste(
        &mut self,
        data: &ClipboardData,
        top_left: Vec2,
    ) -> BTreeSet<NodeId> {
        let mut ids = Vec::with_capacity(data.nodes.len());
        for node in &data.nodes {
            let id = self.add_node_with(
                node.kind,
                node.title.clone(),
                node.description.clone(),
                top_left + node.position,
            );
            ids.push(id);
        }

        for edge in &data.edges {
            if let (Some(from), Some(to)) =
                (ids.get(edge.from), ids.get(edge.to))
            {
                let _ = self.add_edge(*from, *to);
            }
        }

        ids.into_iter().collect()
    }

    /// Duplicates nodes and internal connections, offsetting the copies.
    /// Returns the ids of the copies.
    pub fn duplicate_nodes(
        &mut self,
        ids: &BTreeSet<NodeId>,
        offset: Vec2,
    ) -> BTreeSet<NodeId> {
        let data = self.copy_nodes(ids);
        let top_left = self
            .nodes
            .iter()
            .filter(|node| ids.contains(&node.id))
            .fold(Vec2::new(f32::INFINITY, f32::INFINITY), |acc, node| {
                Vec2::new(
                    acc.x.min(node.position.x),
                    acc.y.min(node.position.y),
                )
            });
        self.paste(&data, top_left + offset)
    }

    /// Looks up a node.
    #[must_use]
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.iter().find(|node| node.id == id)
    }

    /// Looks up a node mutably.
    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|node| node.id == id)
    }

    /// Looks up a connection.
    #[must_use]
    pub fn edge(&self, id: EdgeId) -> Option<&Edge> {
        self.edges.iter().find(|edge| edge.id == id)
    }

    /// Looks up a connection mutably.
    pub fn edge_mut(&mut self, id: EdgeId) -> Option<&mut Edge> {
        self.edges.iter_mut().find(|edge| edge.id == id)
    }

    /// Returns every connection touching `node`.
    pub fn edges_for(&self, node: NodeId) -> impl Iterator<Item = &Edge> {
        self.edges
            .iter()
            .filter(move |edge| edge.from == node || edge.to == node)
    }

    /// Returns the number of connections entering `node`.
    #[must_use]
    pub fn incoming_count(&self, node: NodeId) -> usize {
        self.edges.iter().filter(|edge| edge.to == node).count()
    }

    /// Returns the number of connections leaving `node`.
    #[must_use]
    pub fn outgoing_count(&self, node: NodeId) -> usize {
        self.edges.iter().filter(|edge| edge.from == node).count()
    }

    /// Returns the connection curve between two nodes, if it exists.
    #[must_use]
    pub fn edge_geometry(&self, edge: &Edge) -> Option<EdgeGeometry> {
        let from = self.node(edge.from)?;
        let to = self.node(edge.to)?;
        Some(EdgeGeometry::between(from.output_port(), to.input_port()))
    }

    /// Returns the document's content bounds, or `None` when it is empty.
    #[must_use]
    pub fn bounds(&self) -> Option<Rect> {
        let mut nodes = self.nodes.iter();
        let first = nodes.next()?;
        Some(nodes.fold(first.bounds(), |acc, node| acc.union(node.bounds())))
    }

    /// Returns the topmost node containing `point`.
    #[must_use]
    pub fn node_at(&self, point: Vec2) -> Option<NodeId> {
        self.nodes
            .iter()
            .rev()
            .find(|node| node.bounds().contains(point))
            .map(|node| node.id)
    }

    /// Returns the closest connection within `tolerance` world units.
    #[must_use]
    pub fn edge_at(&self, point: Vec2, tolerance: f32) -> Option<EdgeId> {
        self.edges
            .iter()
            .filter_map(|edge| {
                let geometry = self.edge_geometry(edge)?;
                let distance = geometry.distance_to(point);
                (distance <= tolerance).then_some((edge.id, distance))
            })
            .min_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(id, _)| id)
    }

    /// Returns the nodes whose bounds intersect `rect`.
    #[must_use]
    pub fn nodes_in_rect(&self, rect: Rect) -> BTreeSet<NodeId> {
        self.nodes
            .iter()
            .filter(|node| node.bounds().intersects(rect))
            .map(|node| node.id)
            .collect()
    }

    /// Aligns nodes to an edge of their combined bounds. Returns whether any
    /// position changed.
    pub fn align_nodes(
        &mut self,
        ids: &BTreeSet<NodeId>,
        alignment: Alignment,
    ) -> bool {
        let bounds = match self.selection_bounds(ids) {
            Some(bounds) => bounds,
            None => return false,
        };
        let mut changed = false;
        for node in &mut self.nodes {
            if !ids.contains(&node.id) {
                continue;
            }
            let before = node.position;
            match alignment {
                Alignment::Left => node.position.x = bounds.x,
                Alignment::CenterX => {
                    node.position.x = bounds.center().x - NODE_WIDTH / 2.0
                }
                Alignment::Right => {
                    node.position.x = bounds.right() - NODE_WIDTH
                }
                Alignment::Top => node.position.y = bounds.y,
                Alignment::CenterY => {
                    node.position.y = bounds.center().y - NODE_HEIGHT / 2.0
                }
                Alignment::Bottom => {
                    node.position.y = bounds.bottom() - NODE_HEIGHT
                }
            }
            changed |= node.position != before;
        }
        changed
    }

    /// Spaces three or more nodes evenly between the outermost two, preserving
    /// their order along `axis`. Returns whether any position changed.
    pub fn distribute_nodes(
        &mut self,
        ids: &BTreeSet<NodeId>,
        axis: Axis,
    ) -> bool {
        if ids.len() < 3 {
            return false;
        }

        let mut selected: Vec<usize> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| ids.contains(&node.id))
            .map(|(index, _)| index)
            .collect();
        selected.sort_by(|a, b| match axis {
            Axis::Horizontal => self.nodes[*a]
                .position
                .x
                .total_cmp(&self.nodes[*b].position.x),
            Axis::Vertical => self.nodes[*a]
                .position
                .y
                .total_cmp(&self.nodes[*b].position.y),
        });

        let first = &self.nodes[selected[0]];
        let last = &self.nodes[*selected.last().unwrap()];
        let span = match axis {
            Axis::Horizontal => last.position.x - first.position.x,
            Axis::Vertical => last.position.y - first.position.y,
        };
        let step = span / (selected.len() - 1) as f32;
        let origin = match axis {
            Axis::Horizontal => first.position.x,
            Axis::Vertical => first.position.y,
        };

        let mut changed = false;
        for (offset, index) in
            selected.iter().enumerate().skip(1).take(selected.len() - 2)
        {
            let node = &mut self.nodes[*index];
            match axis {
                Axis::Horizontal => {
                    let target = origin + step * offset as f32;
                    changed |= node.position.x != target;
                    node.position.x = target;
                }
                Axis::Vertical => {
                    let target = origin + step * offset as f32;
                    changed |= node.position.y != target;
                    node.position.y = target;
                }
            }
        }
        changed
    }

    /// Returns the combined bounds of `ids` that exist in the document.
    #[must_use]
    pub fn selection_bounds(&self, ids: &BTreeSet<NodeId>) -> Option<Rect> {
        let mut selected =
            self.nodes.iter().filter(|node| ids.contains(&node.id));
        let first = selected.next()?;
        Some(
            selected.fold(first.bounds(), |acc, node| acc.union(node.bounds())),
        )
    }

    /// Ensures the next-id counters are greater than every existing id.
    pub fn normalize_ids(&mut self) {
        let max_node =
            self.nodes.iter().map(|node| node.id.0).max().unwrap_or(0);
        let max_edge =
            self.edges.iter().map(|edge| edge.id.0).max().unwrap_or(0);
        self.next_node_id = self.next_node_id.max(max_node + 1).max(1);
        self.next_edge_id = self.next_edge_id.max(max_edge + 1).max(1);
    }

    /// Checks the invariants that malformed or hand-edited files could break.
    ///
    /// # Errors
    ///
    /// Returns every problem found, in document order.
    pub fn validate(&self) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();
        let mut node_ids = BTreeSet::new();
        for node in &self.nodes {
            if !node_ids.insert(node.id) {
                errors.push(ValidationError::DuplicateNode(node.id));
            }
            if !node.position.x.is_finite() || !node.position.y.is_finite() {
                errors.push(ValidationError::InvalidPosition(node.id));
            }
        }

        let mut edge_ids = BTreeSet::new();
        let mut pairs = BTreeSet::new();
        for edge in &self.edges {
            if !edge_ids.insert(edge.id) {
                errors.push(ValidationError::DuplicateEdge(edge.id));
            }
            if !node_ids.contains(&edge.from) {
                errors.push(ValidationError::MissingEndpoint {
                    edge: edge.id,
                    node: edge.from,
                });
            }
            if !node_ids.contains(&edge.to) {
                errors.push(ValidationError::MissingEndpoint {
                    edge: edge.id,
                    node: edge.to,
                });
            }
            if edge.from == edge.to {
                errors.push(ValidationError::SelfLoop(edge.id));
            }
            if !pairs.insert((edge.from, edge.to)) {
                errors.push(ValidationError::DuplicateConnection {
                    from: edge.from,
                    to: edge.to,
                });
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Produces actionable observations about the flow's shape.
    #[must_use]
    pub fn analyze(&self) -> Vec<Insight> {
        let mut insights = Vec::new();
        if self.nodes.is_empty() {
            insights.push(Insight {
                severity: Severity::Info,
                node: None,
                message: "This flow is empty. Add a trigger to get started."
                    .into(),
            });
            return insights;
        }

        let mut incoming = HashMap::new();
        let mut outgoing: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for edge in &self.edges {
            *incoming.entry(edge.to).or_insert(0usize) += 1;
            outgoing.entry(edge.from).or_default().push(edge.to);
        }

        let triggers: Vec<NodeId> = self
            .nodes
            .iter()
            .filter(|node| node.kind == NodeKind::Trigger)
            .map(|node| node.id)
            .collect();

        let mut reachable = BTreeSet::new();
        let mut queue: VecDeque<NodeId> = triggers.iter().copied().collect();
        reachable.extend(triggers.iter().copied());
        while let Some(node) = queue.pop_front() {
            if let Some(next) = outgoing.get(&node) {
                for target in next {
                    if reachable.insert(*target) {
                        queue.push_back(*target);
                    }
                }
            }
        }

        for node in &self.nodes {
            let in_count = incoming.get(&node.id).copied().unwrap_or(0);
            let out_count = outgoing.get(&node.id).map_or(0, Vec::len);
            let mut problems = Vec::new();
            if in_count == 0 && node.kind != NodeKind::Trigger {
                problems.push("no incoming connection");
            }
            if out_count == 0 && node.kind != NodeKind::Output {
                problems.push("no outgoing connection");
            }
            if !triggers.is_empty()
                && !reachable.contains(&node.id)
                && node.kind != NodeKind::Trigger
            {
                problems.push("not reachable from a trigger");
            }
            if !problems.is_empty() {
                insights.push(Insight {
                    severity: Severity::Warning,
                    node: Some(node.id),
                    message: format!(
                        "{}: {}.",
                        node.title,
                        problems.join("; ")
                    ),
                });
            }
        }

        if triggers.is_empty() {
            insights.push(Insight {
                severity: Severity::Warning,
                node: None,
                message: "The flow has no trigger.".into(),
            });
        }
        if !self.nodes.iter().any(|node| node.kind == NodeKind::Output) {
            insights.push(Insight {
                severity: Severity::Warning,
                node: None,
                message: "The flow has no output.".into(),
            });
        }
        if self.has_cycle() {
            insights.push(Insight {
                severity: Severity::Info,
                node: None,
                message: "The flow contains a loop.".into(),
            });
        }
        insights
    }

    fn has_cycle(&self) -> bool {
        let mut in_degree: HashMap<NodeId, usize> =
            self.nodes.iter().map(|node| (node.id, 0)).collect();
        let mut outgoing: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for edge in &self.edges {
            if let Some(degree) = in_degree.get_mut(&edge.to) {
                *degree += 1;
            }
            outgoing.entry(edge.from).or_default().push(edge.to);
        }

        let mut queue: VecDeque<NodeId> = in_degree
            .iter()
            .filter_map(|(id, degree)| (*degree == 0).then_some(*id))
            .collect();
        let mut visited = 0;
        while let Some(node) = queue.pop_front() {
            visited += 1;
            if let Some(next) = outgoing.get(&node) {
                for target in next {
                    if let Some(degree) = in_degree.get_mut(target) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push_back(*target);
                        }
                    }
                }
            }
        }
        visited != self.nodes.len()
    }

    fn allocate_node_id(&mut self) -> NodeId {
        if self.next_node_id == 0 {
            self.normalize_ids();
        }
        let id = NodeId(self.next_node_id.max(1));
        self.next_node_id = id.0 + 1;
        id
    }

    fn allocate_edge_id(&mut self) -> EdgeId {
        if self.next_edge_id == 0 {
            self.normalize_ids();
        }
        let id = EdgeId(self.next_edge_id.max(1));
        self.next_edge_id = id.0 + 1;
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(values: &[NodeId]) -> BTreeSet<NodeId> {
        values.iter().copied().collect()
    }

    fn sample_graph() -> (Document, NodeId, NodeId) {
        let mut doc = Document::new("Test");
        let a = doc.add_node(NodeKind::Trigger, Vec2::new(0.0, 0.0));
        let b = doc.add_node(NodeKind::Action, Vec2::new(320.0, 0.0));
        (doc, a, b)
    }

    #[test]
    fn connecting_nodes_creates_an_edge() {
        let (mut doc, a, b) = sample_graph();
        let edge = doc.add_edge(a, b).unwrap();
        assert_eq!(doc.edges.len(), 1);
        assert_eq!(doc.edge(edge).unwrap().from, a);
    }

    #[test]
    fn invalid_connections_are_rejected() {
        let (mut doc, a, b) = sample_graph();
        doc.add_edge(a, b).unwrap();
        assert_eq!(doc.add_edge(a, b), Err(GraphError::DuplicateEdge));
        assert_eq!(doc.add_edge(a, a), Err(GraphError::SelfLoop));
        assert_eq!(
            doc.add_edge(a, NodeId(999)),
            Err(GraphError::MissingNode(NodeId(999)))
        );
    }

    #[test]
    fn removing_a_node_removes_incident_edges() {
        let (mut doc, a, b) = sample_graph();
        doc.add_edge(a, b).unwrap();
        assert_eq!(doc.remove_nodes(&ids(&[a])), 1);
        assert!(doc.edges.is_empty());
        assert!(doc.validate().is_ok());
    }

    #[test]
    fn place_node_avoids_existing_nodes() {
        let mut doc = Document::new("Test");
        let first = doc.add_node(NodeKind::Action, Vec2::new(0.0, 0.0));
        let second = doc.place_node(NodeKind::Action, Vec2::new(0.0, 0.0));
        let a = doc.node(first).unwrap();
        let b = doc.node(second).unwrap();
        assert!(!a.bounds().intersects(b.bounds()));
    }

    #[test]
    fn copy_and_paste_preserve_internal_edges() {
        let (mut doc, a, b) = sample_graph();
        doc.add_edge(a, b).unwrap();
        let data = doc.copy_nodes(&ids(&[a, b]));
        assert_eq!(data.nodes.len(), 2);
        assert_eq!(data.edges.len(), 1);
        let pasted = doc.paste(&data, Vec2::new(600.0, 0.0));
        assert_eq!(pasted.len(), 2);
        assert_eq!(doc.nodes.len(), 4);
        assert_eq!(doc.edges.len(), 2);
        doc.normalize_ids();
        assert!(doc.validate().is_ok());
    }

    #[test]
    fn edge_hit_testing_finds_the_curve() {
        let (mut doc, a, b) = sample_graph();
        let edge = doc.add_edge(a, b).unwrap();
        let geometry = doc.edge_geometry(doc.edge(edge).unwrap()).unwrap();
        assert_eq!(doc.edge_at(geometry.point_at(0.5), 2.0), Some(edge));
        assert_eq!(doc.edge_at(Vec2::new(160.0, 400.0), 2.0), None);
    }

    #[test]
    fn aligning_and_distributing_nodes_changes_layout() {
        let mut doc = Document::new("Test");
        let a = doc.add_node(NodeKind::Action, Vec2::new(0.0, 40.0));
        let b = doc.add_node(NodeKind::Action, Vec2::new(100.0, 0.0));
        let c = doc.add_node(NodeKind::Action, Vec2::new(400.0, 80.0));
        let selection = ids(&[a, b, c]);

        assert!(doc.align_nodes(&selection, Alignment::Top));
        let tops: BTreeSet<u32> = [a, b, c]
            .into_iter()
            .map(|id| doc.node(id).unwrap().position.y as u32)
            .collect();
        assert_eq!(tops.len(), 1);

        doc.node_mut(b).unwrap().position = Vec2::new(160.0, 0.0);
        assert!(doc.distribute_nodes(&selection, Axis::Horizontal));
        let positions: Vec<f32> = [a, b, c]
            .into_iter()
            .map(|id| doc.node(id).unwrap().position.x)
            .collect();
        assert!((positions[1] - 200.0).abs() < 1.0);
    }

    #[test]
    fn analysis_reports_disconnected_and_missing_io_nodes() {
        let mut doc = Document::new("Test");
        doc.add_node(NodeKind::Action, Vec2::new(0.0, 0.0));
        let insights = doc.analyze();
        assert!(
            insights
                .iter()
                .any(|insight| insight.message.contains("no trigger"))
        );
        assert!(
            insights
                .iter()
                .any(|insight| insight.message.contains("no output"))
        );
    }
}
