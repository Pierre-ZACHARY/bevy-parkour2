//! Running graphs: each entity with a [`FlowGraphRunner`] evaluates its
//! graph every fixed step (every frame with [`RunEveryFrame`]) and records
//! what flowed in [`FlowGraphPlayback`].

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use crate::model::{FlowGraph, FlowValue, PortDef, VAR_GET, VAR_SET};
use crate::registry::{ApplyFn, FlowNodeTypes, NodeRun};

/// Runs a graph for this entity: source nodes read it, sink nodes act on it.
///
/// Not reflected on purpose: Jackdaw's play-in-editor streams reflected
/// components to the editor, and a handle means nothing there.
#[derive(Component, Clone, Debug)]
pub struct FlowGraphRunner {
    pub graph: Handle<FlowGraph>,
}

/// Runs the entity's [`FlowGraphRunner`] every frame, in `PostUpdate` before
/// transforms propagate, rather than every fixed step: for what follows the
/// frame and not physics (a camera, reading the mouse's motion of the
/// frame).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct RunEveryFrame;

/// What the last evaluation carried through each node, for views to light
/// up. Reflected, so Jackdaw streams it from the game to the editor while
/// playing (the runner's entity needs a `Transform` for that).
#[derive(Component, Reflect, Clone, PartialEq, Debug, Default)]
#[reflect(Component, Default)]
pub struct FlowGraphPlayback {
    /// The graph's asset path, so a view knows the playback is for its file.
    pub graph: String,
    /// Every node's, those of inner graphs under their path
    /// (`grounded/idle`).
    pub nodes: Vec<FlowNodeState>,
}

impl FlowGraphPlayback {
    pub fn node(&self, id: &str) -> Option<&FlowNodeState> {
        self.nodes.iter().find(|node| node.id == id)
    }
}

/// The values of a runner's graph variables (see
/// [`FlowVariable`](crate::FlowVariable)), kept from step to step: what set
/// nodes wrote, or the game. Reflected, so Jackdaw streams them while
/// playing.
#[derive(Component, Reflect, Clone, PartialEq, Debug, Default)]
#[reflect(Component, Default)]
pub struct FlowVariables {
    /// By name, in the order first set.
    pub values: Vec<(String, FlowValue)>,
}

impl FlowVariables {
    pub fn get(&self, name: &str) -> Option<FlowValue> {
        self.values
            .iter()
            .find(|(own, _)| own == name)
            .map(|(_, value)| value.clone())
    }

    pub fn set(&mut self, name: &str, value: FlowValue) {
        match self.values.iter_mut().find(|(own, _)| own == name) {
            Some((_, own)) => *own = value,
            None => self.values.push((name.to_owned(), value)),
        }
    }
}

/// What a runner's remembering nodes ([`NodeRun::Step`]) kept from their
/// last step, by playback id (`grounded/idle`). A node whose id changes
/// starts over.
#[derive(Component, Clone, PartialEq, Debug, Default)]
pub struct FlowNodeMemory {
    pub nodes: HashMap<String, Vec<FlowValue>>,
}

#[derive(Reflect, Clone, PartialEq, Debug, Default)]
#[reflect(Default)]
pub struct FlowNodeState {
    pub id: String,
    pub inputs: Vec<FlowValue>,
    pub outputs: Vec<FlowValue>,
}

impl FlowNodeState {
    /// Whether the node gave anything other than rest values: its outputs,
    /// or a sink's inputs. A node fed only an input's value but giving
    /// nothing (`y × 2` while `y` is 0) stays dark.
    pub fn is_active(&self) -> bool {
        if self.outputs.is_empty() {
            self.inputs.iter().any(|v| v.is_active())
        } else {
            self.outputs.iter().any(|v| v.is_active())
        }
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum FlowGraphSystems {
    /// Graph evaluation, in `FixedUpdate`. Integrations order it against
    /// what their sinks feed (Tnua: its user-controls set).
    Run,
    /// Evaluation of the [`RunEveryFrame`] graphs, in `PostUpdate`.
    RunFrame,
}

/// One evaluation of a graph for one entity.
pub struct Evaluation {
    /// Every node's, those of inner graphs under their path.
    pub states: Vec<FlowNodeState>,
    /// Sinks to apply, in node order, with their inputs.
    pub applies: Vec<(ApplyFn, Vec<FlowValue>)>,
    /// The variables after the step: as they were, then as set nodes set
    /// them.
    pub variables: FlowVariables,
    /// What remembering nodes kept, for the next step.
    pub memory: FlowNodeMemory,
}

/// Evaluate every node of `graph` for `entity`, pulling each input from the
/// output wired into it. An unwired input reads its value
/// ([`PortDef::value`](crate::PortDef::value)), or its type's, or the rest
/// value; wires between mismatched ports, nodes of an unknown kind and cycles
/// give rest values.
///
/// Then the graphs inside its nodes, the same way, outer graphs first. A
/// variable's get node reads what its set node in the same graph is fed this
/// step, if it has one; else the variable as set before (by an outer graph
/// this step, by the game, by an earlier step), else its declared value.
/// Remembering nodes start from what they kept in `memory`.
pub fn evaluate(
    world: &World,
    entity: Entity,
    graph: &FlowGraph,
    types: &FlowNodeTypes,
    variables: &FlowVariables,
    memory: &FlowNodeMemory,
) -> Evaluation {
    let mut evaluation = Evaluation {
        states: Vec::new(),
        applies: Vec::new(),
        variables: FlowVariables::default(),
        memory: FlowNodeMemory::default(),
    };
    declare(graph, &mut evaluation.variables);
    for (name, value) in &variables.values {
        evaluation.variables.set(name, value.clone());
    }
    evaluate_graph(world, entity, graph, types, memory, "", &mut evaluation);
    evaluation
}

/// Every variable declared in `graph` or inside it, at its declared value.
fn declare(graph: &FlowGraph, variables: &mut FlowVariables) {
    for variable in &graph.variables {
        let value = variable
            .value
            .clone()
            .filter(|value| value.kind() == variable.kind)
            .unwrap_or_else(|| FlowValue::zero(variable.kind));
        variables.set(&variable.name, value);
    }
    for inner in graph.nodes.iter().filter_map(|node| node.graph.as_ref()) {
        declare(inner, variables);
    }
}

/// Evaluate `graph`, its nodes playing back under `prefix`, then the graphs
/// inside its nodes.
fn evaluate_graph(
    world: &World,
    entity: Entity,
    graph: &FlowGraph,
    types: &FlowNodeTypes,
    memory: &FlowNodeMemory,
    prefix: &str,
    evaluation: &mut Evaluation,
) {
    let mut evaluator = Evaluator {
        world,
        entity,
        graph,
        types,
        variables: &evaluation.variables,
        prefix,
        memory,
        kept: Vec::new(),
        done: vec![None; graph.nodes.len()],
        visiting: vec![false; graph.nodes.len()],
    };
    for index in 0..graph.nodes.len() {
        evaluator.node(index);
    }
    evaluation.memory.nodes.extend(evaluator.kept);
    let done = evaluator.done;
    for (index, node) in graph.nodes.iter().enumerate() {
        let (inputs, outputs) = done[index].clone().unwrap_or_default();
        if let Some(NodeRun::Apply(apply)) = types.get(&node.kind).map(|t| &t.run) {
            evaluation.applies.push((apply.clone(), inputs.clone()));
        }
        if let (Some(name), Some(value)) = (node.kind.strip_prefix(VAR_SET), inputs.first()) {
            evaluation.variables.set(name, value.clone());
        }
        evaluation.states.push(FlowNodeState {
            id: format!("{prefix}{}", node.id),
            inputs,
            outputs,
        });
    }
    for node in &graph.nodes {
        if let Some(inner) = &node.graph {
            let prefix = format!("{prefix}{}/", node.id);
            evaluate_graph(world, entity, inner, types, memory, &prefix, evaluation);
        }
    }
}

struct Evaluator<'a> {
    world: &'a World,
    entity: Entity,
    graph: &'a FlowGraph,
    types: &'a FlowNodeTypes,
    /// As set before this graph ran.
    variables: &'a FlowVariables,
    /// What this graph's nodes play back under.
    prefix: &'a str,
    /// What remembering nodes kept from the last step, and what they keep
    /// from this one.
    memory: &'a FlowNodeMemory,
    kept: Vec<(String, Vec<FlowValue>)>,
    /// Per node: inputs and outputs once evaluated.
    done: Vec<Option<(Vec<FlowValue>, Vec<FlowValue>)>>,
    visiting: Vec<bool>,
}

impl Evaluator<'_> {
    /// The outputs of node `index`, evaluating it (and what feeds it) first.
    fn node(&mut self, index: usize) -> Vec<FlowValue> {
        if let Some((_, outputs)) = &self.done[index] {
            return outputs.clone();
        }
        let node = &self.graph.nodes[index];
        if let Some(name) = node.kind.strip_prefix(VAR_GET) {
            return self.get_variable(index, name);
        }
        if node.kind.starts_with(VAR_SET) {
            self.set_variable(index);
            return Vec::new();
        }
        let Some(node_type) = self.types.get(&node.kind) else {
            self.done[index] = Some((Vec::new(), Vec::new()));
            return Vec::new();
        };
        if self.visiting[index] {
            // A cycle: what is upstream of itself reads as rest.
            return rest(&node_type.outputs);
        }
        self.visiting[index] = true;
        let inputs: Vec<FlowValue> = node_type
            .inputs
            .iter()
            .map(|port| self.port_input(index, port))
            .collect();
        let mut outputs = match &node_type.run {
            NodeRun::Compute(compute) => compute(self.world, self.entity, &inputs),
            NodeRun::Step(step) => {
                let id = format!("{}{}", self.prefix, node.id);
                let mut memory = self.memory.nodes.get(&id).cloned().unwrap_or_default();
                let outputs = step(self.world, self.entity, &inputs, &mut memory);
                self.kept.push((id, memory));
                outputs
            }
            NodeRun::Apply(_) => Vec::new(),
        };
        if !matches!(node_type.run, NodeRun::Apply(_)) {
            // Hold the node to the ports it declares.
            outputs.resize(node_type.outputs.len(), FlowValue::default());
            for (value, port) in outputs.iter_mut().zip(&node_type.outputs) {
                if value.kind() != port.kind {
                    *value = FlowValue::zero(port.kind);
                }
            }
        }
        self.visiting[index] = false;
        self.done[index] = Some((inputs, outputs.clone()));
        outputs
    }

    /// What node `index` reads on input `port` (as its type, or its own
    /// copy, declares it): the wire into it, else its own value, else the
    /// type's.
    fn port_input(&mut self, index: usize, port: &PortDef) -> FlowValue {
        let node = &self.graph.nodes[index];
        let value = if self.graph.link_into(&node.id, &port.name).is_none() {
            // The node's copy of the port holds its value.
            let own = node.inputs.iter().find(|own| own.name == port.name);
            own.and_then(|own| own.value.clone())
                .or_else(|| port.value.clone())
        } else {
            let id = node.id.clone();
            self.input(&id, &port.name)
        };
        value
            .filter(|value| value.kind() == port.kind)
            .unwrap_or(FlowValue::zero(port.kind))
    }

    /// A get node's output: what this graph's set node of `name` is fed, or
    /// the variable as set before.
    fn get_variable(&mut self, index: usize, name: &str) -> Vec<FlowValue> {
        let setter = self
            .graph
            .nodes
            .iter()
            .position(|node| node.kind.strip_prefix(VAR_SET) == Some(name));
        let fed = setter.and_then(|setter| {
            if self.visiting[index] {
                // A cycle through the variable.
                return None;
            }
            self.visiting[index] = true;
            self.set_variable(setter);
            self.visiting[index] = false;
            self.done[setter]
                .as_ref()
                .and_then(|(inputs, _)| inputs.first().cloned())
        });
        let value = fed.or_else(|| self.variables.get(name));
        let outputs: Vec<FlowValue> = self.graph.nodes[index]
            .outputs
            .first()
            .map(|port| {
                value
                    .filter(|value| value.kind() == port.kind)
                    .unwrap_or(FlowValue::zero(port.kind))
            })
            .into_iter()
            .collect();
        self.done[index] = Some((Vec::new(), outputs.clone()));
        outputs
    }

    /// Evaluate a set node: its one input, read like any node's.
    fn set_variable(&mut self, index: usize) {
        if self.done[index].is_some() || self.visiting[index] {
            return;
        }
        self.visiting[index] = true;
        let port = self.graph.nodes[index].inputs.first().cloned();
        let inputs: Vec<FlowValue> = port
            .map(|port| self.port_input(index, &port))
            .into_iter()
            .collect();
        self.visiting[index] = false;
        self.done[index] = Some((inputs, Vec::new()));
    }

    /// The value wired into `node`'s input `port`, if it is wired.
    fn input(&mut self, node: &str, port: &str) -> Option<FlowValue> {
        let link = self.graph.link_into(node, port)?;
        let source = self.graph.nodes.iter().position(|n| n.id == link.from)?;
        let source_node = &self.graph.nodes[source];
        let outputs = match self.types.get(&source_node.kind) {
            Some(source_type) => &source_type.outputs,
            // Get nodes declare their port themselves.
            None => &source_node.outputs,
        };
        let output = outputs.iter().position(|p| p.name == link.output)?;
        self.node(source).get(output).cloned()
    }
}

/// Rest values for `ports`.
fn rest(ports: &[PortDef]) -> Vec<FlowValue> {
    ports.iter().map(|p| FlowValue::zero(p.kind)).collect()
}

/// Evaluate every fixed-step runner's graph, apply its sinks and record its
/// playback, variables and node memory.
pub fn run_flow_graphs(world: &mut World) {
    let runners: Vec<(Entity, Handle<FlowGraph>)> = world
        .query_filtered::<(Entity, &FlowGraphRunner), Without<RunEveryFrame>>()
        .iter(world)
        .map(|(entity, runner)| (entity, runner.graph.clone()))
        .collect();
    run_graphs(world, runners);
}

/// [`run_flow_graphs`] for the [`RunEveryFrame`] runners.
pub fn run_frame_flow_graphs(world: &mut World) {
    let runners: Vec<(Entity, Handle<FlowGraph>)> = world
        .query_filtered::<(Entity, &FlowGraphRunner), With<RunEveryFrame>>()
        .iter(world)
        .map(|(entity, runner)| (entity, runner.graph.clone()))
        .collect();
    run_graphs(world, runners);
}

fn run_graphs(world: &mut World, runners: Vec<(Entity, Handle<FlowGraph>)>) {
    for (entity, handle) in runners {
        let Some(graph) = world.resource::<Assets<FlowGraph>>().get(&handle).cloned() else {
            continue;
        };
        let variables = world
            .get::<FlowVariables>(entity)
            .cloned()
            .unwrap_or_default();
        let memory = world
            .get::<FlowNodeMemory>(entity)
            .cloned()
            .unwrap_or_default();
        let evaluation = evaluate(
            world,
            entity,
            &graph,
            world.resource::<FlowNodeTypes>(),
            &variables,
            &memory,
        );
        for (apply, inputs) in &evaluation.applies {
            apply(world, entity, inputs);
        }
        let playback = FlowGraphPlayback {
            graph: handle.path().map(|p| p.to_string()).unwrap_or_default(),
            nodes: evaluation.states,
        };
        let mut runner = world.entity_mut(entity);
        match runner.get_mut::<FlowGraphPlayback>() {
            Some(mut current) => {
                current.set_if_neq(playback);
            }
            None => {
                runner.insert(playback);
            }
        }
        match runner.get_mut::<FlowVariables>() {
            Some(mut current) => {
                current.set_if_neq(evaluation.variables);
            }
            None => {
                runner.insert(evaluation.variables);
            }
        }
        runner.insert(evaluation.memory);
    }
}
