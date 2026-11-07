//! Node types: what each `kind` in a graph file does. The crate registers a
//! few of its own; integrations (leafwing, Tnua) and games register theirs.

use std::sync::Arc;

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use crate::model::{FlowGraph, FlowNode, FlowNodeCatalog, FlowValue, PortDef, PortKind};

/// Computes a node's outputs from its inputs, reading the world as seen by
/// the entity running the graph. Source nodes read their values here.
pub type ComputeFn = Arc<dyn Fn(&World, Entity, &[FlowValue]) -> Vec<FlowValue> + Send + Sync>;
/// Like a [`ComputeFn`], for a node that remembers: also given what it
/// kept on its last step (empty on its first), to update in place. Each
/// node of a running graph has its own (see
/// [`FlowNodeMemory`](crate::FlowNodeMemory)).
pub type StepFn =
    Arc<dyn Fn(&World, Entity, &[FlowValue], &mut Vec<FlowValue>) -> Vec<FlowValue> + Send + Sync>;
/// Acts on the world with a sink node's inputs, for the running entity.
pub type ApplyFn = Arc<dyn Fn(&mut World, Entity, &[FlowValue]) + Send + Sync>;
/// The names a view offers for a text input, in a dropdown rather than a
/// field to type in: read from the world the view is in, given the node's
/// inputs as the graph evaluates there.
pub type ChoicesFn = Arc<dyn Fn(&World, &[FlowValue]) -> Vec<String> + Send + Sync>;

/// `C` on `entity` or, failing that, on its nearest ancestor that has one:
/// a graph run for a character's model reads the character's components.
pub fn nearest_component<C: Component>(world: &World, entity: Entity) -> Option<&C> {
    world.get::<C>(nearest_with::<C>(world, entity)?)
}

/// `entity` if it has `C`, else its nearest ancestor that has one.
pub(crate) fn nearest_with<C: Component>(world: &World, entity: Entity) -> Option<Entity> {
    let mut entity = entity;
    loop {
        if world.get::<C>(entity).is_some() {
            return Some(entity);
        }
        entity = world.get::<ChildOf>(entity)?.parent();
    }
}

#[derive(Clone)]
pub enum NodeRun {
    Compute(ComputeFn),
    /// A node with a memory: what a double tap or an easing needs.
    Step(StepFn),
    /// A sink: no outputs.
    Apply(ApplyFn),
}

/// A kind of node a graph may use.
#[derive(Clone)]
pub struct FlowNodeType {
    pub kind: String,
    pub title: String,
    pub inputs: Vec<PortDef>,
    pub outputs: Vec<PortDef>,
    pub run: NodeRun,
    /// The graph new nodes of the type hold inside, if they hold one.
    pub graph: Option<FlowGraph>,
    /// Whether views draw its nodes as arrows (see [`Self::arrow`]).
    pub arrow: bool,
    /// Its text inputs picked from a dropdown, by name (see
    /// [`Self::with_choices`]).
    pub choices: Vec<(String, ChoicesFn)>,
}

fn ports(ports: &[(&str, PortKind)]) -> Vec<PortDef> {
    ports
        .iter()
        .map(|(name, kind)| PortDef::new(*name, *kind))
        .collect()
}

impl FlowNodeType {
    pub fn compute(
        kind: impl Into<String>,
        title: impl Into<String>,
        inputs: &[(&str, PortKind)],
        outputs: &[(&str, PortKind)],
        compute: impl Fn(&World, Entity, &[FlowValue]) -> Vec<FlowValue> + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind: kind.into(),
            title: title.into(),
            inputs: ports(inputs),
            outputs: ports(outputs),
            run: NodeRun::Compute(Arc::new(compute)),
            graph: None,
            arrow: false,
            choices: Vec::new(),
        }
    }

    /// A node that remembers between steps: `step` reads its inputs and
    /// updates its memory, then gives its outputs.
    pub fn step(
        kind: impl Into<String>,
        title: impl Into<String>,
        inputs: &[(&str, PortKind)],
        outputs: &[(&str, PortKind)],
        step: impl Fn(&World, Entity, &[FlowValue], &mut Vec<FlowValue>) -> Vec<FlowValue>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            kind: kind.into(),
            title: title.into(),
            inputs: ports(inputs),
            outputs: ports(outputs),
            run: NodeRun::Step(Arc::new(step)),
            graph: None,
            arrow: false,
            choices: Vec::new(),
        }
    }

    pub fn apply(
        kind: impl Into<String>,
        title: impl Into<String>,
        inputs: &[(&str, PortKind)],
        apply: impl Fn(&mut World, Entity, &[FlowValue]) + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind: kind.into(),
            title: title.into(),
            inputs: ports(inputs),
            outputs: Vec::new(),
            run: NodeRun::Apply(Arc::new(apply)),
            graph: None,
            arrow: false,
            choices: Vec::new(),
        }
    }

    /// The same type with `value` as input `input`'s default: what new
    /// nodes start with, and what one reads unwired without a value of its
    /// own.
    pub fn with_default(mut self, input: &str, value: FlowValue) -> Self {
        if let Some(port) = self.inputs.iter_mut().find(|port| port.name == input) {
            port.value = Some(value);
        }
        self
    }

    /// The same type, its text input `input` picked in views from the
    /// names `choices` gives.
    pub fn with_choices(
        mut self,
        input: &str,
        choices: impl Fn(&World, &[FlowValue]) -> Vec<String> + Send + Sync + 'static,
    ) -> Self {
        self.choices.push((input.to_owned(), Arc::new(choices)));
        self
    }

    /// What input `input` is picked from, if from a dropdown.
    pub fn choices_for(&self, input: &str) -> Option<&ChoicesFn> {
        self.choices
            .iter()
            .find(|(name, _)| name == input)
            .map(|(_, choices)| choices)
    }

    /// The same type, its new nodes holding `graph` inside.
    pub fn with_graph(mut self, graph: FlowGraph) -> Self {
        self.graph = Some(graph);
        self
    }

    /// The same type, its nodes drawn as arrows from the node wired into
    /// its first `State` input to the one its first `State` output leads to,
    /// with its other ports on a pill halfway: a state machine's
    /// transitions. Wiring a state's `State` output straight into another's
    /// `State` input on a canvas puts one in between.
    pub fn arrow(mut self) -> Self {
        self.arrow = true;
        self
    }

    /// A graph node of this type.
    pub fn instantiate(&self, id: impl Into<String>, position: Vec2) -> FlowNode {
        FlowNode {
            id: id.into(),
            kind: self.kind.clone(),
            title: self.title.clone(),
            position,
            inputs: self.inputs.clone(),
            outputs: self.outputs.clone(),
            graph: self.graph.clone(),
            samples: Vec::new(),
            axes: None,
        }
    }
}

/// Every registered node type, by kind.
#[derive(Resource, Default, Clone)]
pub struct FlowNodeTypes(HashMap<String, FlowNodeType>);

impl FlowNodeTypes {
    pub fn get(&self, kind: &str) -> Option<&FlowNodeType> {
        self.0.get(kind)
    }

    pub fn insert(&mut self, node_type: FlowNodeType) {
        self.0.insert(node_type.kind.clone(), node_type);
    }

    pub fn iter(&self) -> impl Iterator<Item = &FlowNodeType> {
        self.0.values()
    }

    /// Every type as a template node, sorted by kind.
    pub fn catalog(&self) -> FlowNodeCatalog {
        let mut nodes: Vec<FlowNode> = self
            .iter()
            .map(|node_type| node_type.instantiate("", Vec2::ZERO))
            .collect();
        nodes.sort_by(|a, b| a.kind.cmp(&b.kind));
        let mut arrows: Vec<String> = self
            .iter()
            .filter(|node_type| node_type.arrow)
            .map(|node_type| node_type.kind.clone())
            .collect();
        arrows.sort();
        FlowNodeCatalog {
            nodes,
            arrows,
            ..default()
        }
    }
}

pub trait FlowGraphAppExt {
    /// Make `node_type` available to graphs, replacing any of the same kind.
    fn register_flow_node(&mut self, node_type: FlowNodeType) -> &mut Self;
}

impl FlowGraphAppExt for App {
    fn register_flow_node(&mut self, node_type: FlowNodeType) -> &mut Self {
        self.world_mut()
            .get_resource_or_init::<FlowNodeTypes>()
            .insert(node_type);
        self
    }
}
