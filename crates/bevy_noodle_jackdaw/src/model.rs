//! What a graph file holds: nodes, their ports and the links between them.

use bevy::prelude::*;

/// What a port carries.
#[derive(Reflect, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[reflect(Default)]
pub enum PortKind {
    #[default]
    Float,
    Bool,
    Vec2,
    Vec3,
    /// Which state of a state machine is current: wires between states and
    /// their transitions. A state input takes any number of wires.
    State,
    /// An easing curve and how long it takes: see [`Easing`].
    Easing,
    /// A name: of an entity, of a bone. Typed on the node; wires carry it
    /// too.
    Text,
    /// An entity of the world the graph runs in, as a node found it (see
    /// [`crate::nodes`]: Entity, Skeleton, Bone). Wires carry it; it has no
    /// value of its own to save.
    Entity,
}

/// A value on a port.
#[derive(Reflect, Clone, PartialEq, Debug)]
#[reflect(Default)]
pub enum FlowValue {
    Float(f32),
    Bool(bool),
    Vec2(Vec2),
    Vec3(Vec3),
    /// Whether the state, or the transition, is the current one.
    State(bool),
    Easing(Easing),
    Text(String),
    /// `None` at rest: nothing found.
    Entity(Option<Entity>),
}

impl Default for FlowValue {
    fn default() -> Self {
        Self::Float(0.0)
    }
}

impl FlowValue {
    /// The rest value of a port: zero, or false.
    pub fn zero(kind: PortKind) -> Self {
        match kind {
            PortKind::Float => Self::Float(0.0),
            PortKind::Bool => Self::Bool(false),
            PortKind::Vec2 => Self::Vec2(Vec2::ZERO),
            PortKind::Vec3 => Self::Vec3(Vec3::ZERO),
            PortKind::State => Self::State(false),
            PortKind::Easing => Self::Easing(Easing::LINEAR),
            PortKind::Text => Self::Text(String::new()),
            PortKind::Entity => Self::Entity(None),
        }
    }

    pub fn kind(&self) -> PortKind {
        match self {
            Self::Float(_) => PortKind::Float,
            Self::Bool(_) => PortKind::Bool,
            Self::Vec2(_) => PortKind::Vec2,
            Self::Vec3(_) => PortKind::Vec3,
            Self::State(_) => PortKind::State,
            Self::Easing(_) => PortKind::Easing,
            Self::Text(_) => PortKind::Text,
            Self::Entity(_) => PortKind::Entity,
        }
    }

    /// Whether the value is away from rest: what a view lights up.
    pub fn is_active(&self) -> bool {
        *self != Self::zero(self.kind())
    }

    pub fn as_f32(&self) -> f32 {
        match self {
            Self::Float(value) => *value,
            _ => 0.0,
        }
    }

    pub fn as_bool(&self) -> bool {
        matches!(self, Self::Bool(true))
    }

    pub fn as_state(&self) -> bool {
        matches!(self, Self::State(true))
    }

    pub fn as_vec2(&self) -> Vec2 {
        match self {
            Self::Vec2(value) => *value,
            _ => Vec2::ZERO,
        }
    }

    pub fn as_vec3(&self) -> Vec3 {
        match self {
            Self::Vec3(value) => *value,
            _ => Vec3::ZERO,
        }
    }

    pub fn as_easing(&self) -> Easing {
        match self {
            Self::Easing(value) => *value,
            _ => Easing::LINEAR,
        }
    }

    pub fn as_text(&self) -> &str {
        match self {
            Self::Text(value) => value,
            _ => "",
        }
    }

    pub fn as_entity(&self) -> Option<Entity> {
        match self {
            Self::Entity(value) => *value,
            _ => None,
        }
    }
}

/// How a value goes from one level to another over `duration` seconds: a
/// cubic Bézier from (0, 0) to (1, 1) through two handles, time across and
/// progress up, as CSS's `cubic-bezier(x1, y1, x2, y2)`.
#[derive(Reflect, Clone, Copy, PartialEq, Debug)]
#[reflect(Default)]
pub struct Easing {
    pub duration: f32,
    /// The handle leaving (0, 0).
    pub start: Vec2,
    /// The handle reaching (1, 1).
    pub end: Vec2,
}

impl Default for Easing {
    fn default() -> Self {
        Self::LINEAR
    }
}

impl Easing {
    /// Straight, and instant.
    pub const LINEAR: Self = Self {
        duration: 0.0,
        start: Vec2::new(1.0 / 3.0, 1.0 / 3.0),
        end: Vec2::new(2.0 / 3.0, 2.0 / 3.0),
    };

    /// CSS's `ease-in-out` over `duration`.
    pub fn ease_in_out(duration: f32) -> Self {
        Self::new(duration, Vec2::new(0.42, 0.0), Vec2::new(0.58, 1.0))
    }

    /// CSS's `ease-out` over `duration`: fast first, settling.
    pub fn ease_out(duration: f32) -> Self {
        Self::new(duration, Vec2::new(0.0, 0.0), Vec2::new(0.58, 1.0))
    }

    pub fn new(duration: f32, start: Vec2, end: Vec2) -> Self {
        Self {
            duration,
            start,
            end,
        }
    }

    /// The progress, 0 to 1 (or past, for handles above 1), at `time`, 0
    /// to 1 across the duration.
    pub fn at(&self, time: f32) -> f32 {
        let time = time.clamp(0.0, 1.0);
        // Handles held within the time span keep the curve a function of
        // time.
        let (x1, x2) = (self.start.x.clamp(0.0, 1.0), self.end.x.clamp(0.0, 1.0));
        let bezier = |a: f32, b: f32, t: f32| {
            let u = 1.0 - t;
            3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
        };
        // x(t) rises with t: halve towards the t giving `time`.
        let (mut low, mut high) = (0.0, 1.0);
        for _ in 0..24 {
            let mid = (low + high) / 2.0;
            if bezier(x1, x2, mid) < time {
                low = mid;
            } else {
                high = mid;
            }
        }
        bezier(self.start.y, self.end.y, (low + high) / 2.0)
    }

    /// The progress `elapsed` seconds in: 1 once the duration is over, or
    /// at once for none.
    pub fn after(&self, elapsed: f32) -> f32 {
        if self.duration <= 0.0 {
            return self.at(1.0);
        }
        self.at(elapsed / self.duration)
    }
}

/// A named port.
#[derive(Reflect, Clone, PartialEq, Debug, Default)]
#[reflect(Default)]
pub struct PortDef {
    pub name: String,
    pub kind: PortKind,
    /// On a node's input: what it reads while unwired, instead of the rest
    /// value. Set in the view's value fields. On a node type's input, the
    /// default for nodes whose copy has none.
    #[reflect(default)]
    pub value: Option<FlowValue>,
}

impl PortDef {
    pub fn new(name: impl Into<String>, kind: PortKind) -> Self {
        Self {
            name: name.into(),
            kind,
            value: None,
        }
    }

    /// What the port reads unwired: its value if it has one of its kind,
    /// else the rest value.
    pub fn unwired_value(&self) -> FlowValue {
        self.value
            .as_ref()
            .filter(|value| value.kind() == self.kind)
            .cloned()
            .unwrap_or_else(|| FlowValue::zero(self.kind))
    }
}

/// One node of a graph.
///
/// The title and ports are a copy of the node type's, so a tool that does
/// not know the type (the editor extension knows no game types) can still
/// draw it. The game runs the ports its registered type declares.
#[derive(Reflect, Clone, PartialEq, Debug, Default)]
#[reflect(Default)]
pub struct FlowNode {
    /// Unique in the graph; links name nodes by it.
    pub id: String,
    /// The registered node type, e.g. `core/make_vector2`.
    pub kind: String,
    pub title: String,
    /// Where the node sits on the canvas, in graph units.
    pub position: Vec2,
    pub inputs: Vec<PortDef>,
    pub outputs: Vec<PortDef>,
    /// The graph inside the node, for nodes that hold one (a state
    /// machine's sub-machine). It runs with the graph holding it; its nodes
    /// play back under `{id}/`.
    #[reflect(default)]
    pub graph: Option<FlowGraph>,
    /// A blend space's samples (see [`crate::blend`]); none for other
    /// nodes.
    #[reflect(default)]
    pub samples: Vec<BlendSample>,
}

/// One sample of a blend space: what it plays, and where it sits.
#[derive(Reflect, Clone, PartialEq, Debug, Default)]
#[reflect(Default)]
pub struct BlendSample {
    /// The node kind it plays (a clip's, usually).
    pub kind: String,
    /// That kind's title, a copy for tools that do not know the kind.
    pub title: String,
    /// Where it sits: on the axis (`x`) in 1D, on the plane in 2D.
    pub at: Vec2,
}

impl FlowNode {
    /// What a library groups the node under: its kind's prefix (`float` for
    /// `float/add`).
    pub fn category(&self) -> &str {
        self.kind
            .split_once('/')
            .map_or("", |(category, _)| category)
    }
}

/// The node types a game offers, as nodes with no id: what an editor that
/// cannot see the game's registry lists in its node library. Written by
/// [`FlowNodeCatalogPlugin`](crate::FlowNodeCatalogPlugin).
#[derive(Reflect, Clone, PartialEq, Debug, Default)]
#[reflect(Default)]
pub struct FlowNodeCatalog {
    pub nodes: Vec<FlowNode>,
    /// The kinds views draw as arrows (see
    /// [`FlowNodeType::arrow`](crate::FlowNodeType::arrow)).
    #[reflect(default)]
    pub arrows: Vec<String>,
    /// What blend spaces preview their samples with.
    pub preview: crate::blend::BlendPreviewAssets,
}

/// A wire from an output port to an input port.
#[derive(Reflect, Clone, PartialEq, Eq, Debug, Default)]
#[reflect(Default)]
pub struct FlowLink {
    pub from: String,
    pub output: String,
    pub to: String,
    pub input: String,
}

impl FlowLink {
    pub fn new(
        from: impl Into<String>,
        output: impl Into<String>,
        to: impl Into<String>,
        input: impl Into<String>,
    ) -> Self {
        Self {
            from: from.into(),
            output: output.into(),
            to: to.into(),
            input: input.into(),
        }
    }
}

/// A graph, loaded from a `.flowgraph.bsn` file.
#[derive(Asset, Reflect, Clone, PartialEq, Debug, Default)]
#[reflect(Default)]
pub struct FlowGraph {
    pub nodes: Vec<FlowNode>,
    pub links: Vec<FlowLink>,
    /// How the view bundles links; running the graph ignores them.
    #[reflect(default)]
    pub ties: Vec<FlowTie>,
    /// Named values its nodes, and those of the graphs inside them, read
    /// and write: see [`FlowVariable`].
    #[reflect(default)]
    pub variables: Vec<FlowVariable>,
}

/// Where get and set nodes name the variable they read or write, after
/// their kind's prefix: `var/get/speed`.
pub const VAR_GET: &str = "var/get/";
pub const VAR_SET: &str = "var/set/";

/// A named value of a graph. A get node (kind [`VAR_GET`]`name`) reads it
/// and a set node ([`VAR_SET`]`name`) writes it, in the graph declaring it
/// or any graph inside it, without wires between them; the game may set it
/// too ([`FlowVariables`](crate::FlowVariables)).
#[derive(Reflect, Clone, PartialEq, Debug, Default)]
#[reflect(Default)]
pub struct FlowVariable {
    pub name: String,
    pub kind: PortKind,
    /// Its value until something sets it; else the rest value.
    #[reflect(default)]
    pub value: Option<FlowValue>,
}

impl FlowVariable {
    pub fn new(name: impl Into<String>, kind: PortKind) -> Self {
        Self {
            name: name.into(),
            kind,
            value: None,
        }
    }

    /// A node reading the variable.
    pub fn get_node(&self, id: impl Into<String>, position: Vec2) -> FlowNode {
        FlowNode {
            id: id.into(),
            kind: format!("{VAR_GET}{}", self.name),
            title: self.name.clone(),
            position,
            outputs: vec![PortDef::new("value", self.kind)],
            ..default()
        }
    }

    /// A node writing the variable.
    pub fn set_node(&self, id: impl Into<String>, position: Vec2) -> FlowNode {
        FlowNode {
            id: id.into(),
            kind: format!("{VAR_SET}{}", self.name),
            title: format!("Set {}", self.name),
            position,
            inputs: vec![PortDef::new("value", self.kind)],
            ..default()
        }
    }
}

/// A zip tie: links drawn bundled through one point, side by side.
#[derive(Reflect, Clone, PartialEq, Debug, Default)]
#[reflect(Default)]
pub struct FlowTie {
    /// Where the bundle passes, in graph units.
    pub position: Vec2,
    /// Which way the wires run through it (output to input), in radians: 0
    /// is left to right.
    pub angle: f32,
    /// The bundled links, in order across the tie.
    pub links: Vec<FlowLink>,
    /// Drawn in this colour; `None` for the default.
    #[reflect(default)]
    pub color: Option<Color>,
}

impl FlowTie {
    /// The way the wires run through the tie.
    pub fn direction(&self) -> Vec2 {
        Vec2::from_angle(self.angle)
    }
}

impl FlowGraph {
    pub fn node(&self, id: &str) -> Option<&FlowNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    /// The node at `path`: ids from this graph down through the graphs
    /// inside nodes, joined by `/` (`grounded/idle`).
    pub fn node_at(&self, path: &str) -> Option<&FlowNode> {
        match path.split_once('/') {
            None => self.node(path),
            Some((outer, inner)) => self.node(outer)?.graph.as_ref()?.node_at(inner),
        }
    }

    /// The graph inside the node at `path`; this graph for an empty path.
    pub fn graph_at(&self, path: &str) -> Option<&FlowGraph> {
        if path.is_empty() {
            return Some(self);
        }
        self.node_at(path)?.graph.as_ref()
    }

    /// The link feeding `node`'s input `port`, if any.
    pub fn link_into(&self, node: &str, port: &str) -> Option<&FlowLink> {
        self.links
            .iter()
            .find(|link| link.to == node && link.input == port)
    }
}
