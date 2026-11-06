use std::sync::{Arc, Mutex};

use bevy::prelude::*;

use super::*;
use crate::blend::{BlendPreviewAssets, PreviewClip, SyncCycles};

/// The graph the game wires its movement with, sources and sink stubbed.
fn movement_graph() -> FlowGraph {
    FlowGraph {
        nodes: vec![
            FlowNode {
                id: "move".into(),
                kind: "test/move".into(),
                title: "Move".into(),
                position: Vec2::new(40.0, 60.0),
                inputs: vec![],
                outputs: vec![
                    PortDef::new("x", PortKind::Float),
                    PortDef::new("y", PortKind::Float),
                ],
                ..default()
            },
            FlowNode {
                id: "make".into(),
                kind: nodes::MAKE_VECTOR2.into(),
                title: "Make Vector".into(),
                position: Vec2::new(260.0, 60.0),
                inputs: vec![
                    PortDef::new("x", PortKind::Float),
                    PortDef::new("y", PortKind::Float),
                ],
                outputs: vec![PortDef::new("vector", PortKind::Vec2)],
                ..default()
            },
            FlowNode {
                id: "sink".into(),
                kind: "test/sink".into(),
                title: "Sink".into(),
                position: Vec2::new(480.0, 60.0),
                inputs: vec![PortDef::new("vector", PortKind::Vec2)],
                outputs: vec![],
                ..default()
            },
        ],
        links: vec![
            FlowLink::new("move", "x", "make", "x"),
            FlowLink::new("move", "y", "make", "y"),
            FlowLink::new("make", "vector", "sink", "vector"),
        ],
        ..default()
    }
}

/// What the stub sink received, in order.
#[derive(Resource, Default, Clone)]
struct Received(Arc<Mutex<Vec<Vec2>>>);

/// The stub move source outputs this component's value.
#[derive(Component)]
struct Stick(Vec2);

fn app() -> (App, Received) {
    let received = Received::default();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), FlowGraphPlugin));
    let sink = received.clone();
    app.register_flow_node(FlowNodeType::compute(
        "test/move",
        "Move",
        &[],
        &[("x", PortKind::Float), ("y", PortKind::Float)],
        |world, entity, _| {
            let stick = world.get::<Stick>(entity).map_or(Vec2::ZERO, |s| s.0);
            vec![FlowValue::Float(stick.x), FlowValue::Float(stick.y)]
        },
    ))
    .register_flow_node(FlowNodeType::apply(
        "test/sink",
        "Sink",
        &[("vector", PortKind::Vec2)],
        move |_, _, inputs| sink.0.lock().unwrap().push(inputs[0].as_vec2()),
    ));
    (app, received)
}

fn evaluate_for(app: &mut App, graph: &FlowGraph, stick: Vec2) -> Evaluation {
    let entity = app.world_mut().spawn(Stick(stick)).id();
    let world = app.world();
    evaluate(
        world,
        entity,
        graph,
        world.resource::<FlowNodeTypes>(),
        &FlowVariables::default(),
        &FlowNodeMemory::default(),
    )
}

#[test]
fn values_flow_from_sources_to_sinks() {
    let (mut app, _) = app();
    let evaluation = evaluate_for(&mut app, &movement_graph(), Vec2::new(0.25, 1.0));
    assert_eq!(evaluation.applies.len(), 1);
    let sink = &evaluation.states[2];
    assert_eq!(sink.inputs, vec![FlowValue::Vec2(Vec2::new(0.25, 1.0))]);
    assert!(evaluation.states.iter().all(FlowNodeState::is_active));
}

#[test]
fn idle_input_leaves_every_node_inactive() {
    let (mut app, _) = app();
    let evaluation = evaluate_for(&mut app, &movement_graph(), Vec2::ZERO);
    assert!(!evaluation.states.iter().any(FlowNodeState::is_active));
}

#[test]
fn a_cut_wire_gives_the_rest_value() {
    let (mut app, _) = app();
    let mut graph = movement_graph();
    graph.links.retain(|link| link.input != "y");
    let evaluation = evaluate_for(&mut app, &graph, Vec2::new(0.25, 1.0));
    assert_eq!(
        evaluation.states[2].inputs,
        vec![FlowValue::Vec2(Vec2::new(0.25, 0.0))]
    );
}

#[test]
fn an_unwired_input_reads_its_value() {
    let (mut app, _) = app();
    let mut graph = movement_graph();
    graph.links.retain(|link| link.input != "y");
    let make = &mut graph.nodes[1];
    make.inputs[1].value = Some(FlowValue::Float(2.0));
    // Wired: the wire wins.
    make.inputs[0].value = Some(FlowValue::Float(9.0));
    let evaluation = evaluate_for(&mut app, &graph, Vec2::new(0.25, 1.0));
    assert_eq!(
        evaluation.states[2].inputs,
        vec![FlowValue::Vec2(Vec2::new(0.25, 2.0))]
    );
    // A value of another kind than the port's reads as rest.
    graph.nodes[1].inputs[1].value = Some(FlowValue::Bool(true));
    let evaluation = evaluate_for(&mut app, &graph, Vec2::new(0.25, 1.0));
    assert_eq!(
        evaluation.states[2].inputs,
        vec![FlowValue::Vec2(Vec2::new(0.25, 0.0))]
    );
}

#[test]
fn a_wire_between_mismatched_ports_gives_the_rest_value() {
    let (mut app, _) = app();
    let mut graph = movement_graph();
    // Make Vector's x fed by its own vector output's type: a Vec2 on a Float.
    graph.links[0] = FlowLink::new("make", "vector", "make", "x");
    let evaluation = evaluate_for(&mut app, &graph, Vec2::new(0.25, 1.0));
    assert_eq!(
        evaluation.states[2].inputs,
        vec![FlowValue::Vec2(Vec2::new(0.0, 1.0))]
    );
}

#[test]
fn unknown_kinds_and_cycles_do_not_hang_or_panic() {
    let (mut app, _) = app();
    let mut graph = movement_graph();
    graph.nodes[0].kind = "missing/kind".into();
    graph
        .links
        .push(FlowLink::new("make", "vector", "make", "y"));
    let evaluation = evaluate_for(&mut app, &graph, Vec2::ONE);
    assert_eq!(evaluation.states.len(), 3);
}

#[test]
fn bsn_round_trip_keeps_the_graph() {
    let (app, _) = app();
    let registry = app.world().resource::<AppTypeRegistry>().read();
    let mut graph = movement_graph();
    graph.ties.push(FlowTie {
        position: Vec2::new(120.0, -40.5),
        angle: 0.25,
        links: graph.links[..2].to_vec(),
        color: Some(Color::srgb(0.35, 0.6, 0.95)),
    });
    graph.nodes[1].inputs[1].value = Some(FlowValue::Float(2.0));
    graph.nodes[2].inputs[0].value = Some(FlowValue::Vec2(Vec2::new(0.5, -1.0)));
    graph.nodes[2].inputs.push(PortDef {
        value: Some(FlowValue::Easing(Easing::ease_in_out(0.4))),
        ..PortDef::new("easing", PortKind::Easing)
    });
    graph.nodes[2].inputs.push(PortDef {
        value: Some(FlowValue::Text("Player \"one\"".into())),
        ..PortDef::new("target", PortKind::Text)
    });
    let text = flow_graph_to_bsn(&graph, &registry);
    let read = parse_flow_graph(&text, &registry).unwrap_or_else(|err| panic!("{err}\n{text}"));
    assert_eq!(read, graph, "{text}");
}

#[test]
fn graphs_saved_before_ties_still_read() {
    let (app, _) = app();
    let registry = app.world().resource::<AppTypeRegistry>().read();
    let graph = movement_graph();
    let text = flow_graph_to_bsn(&graph, &registry);
    let untied: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim_start().starts_with("ties:"))
        .collect();
    let untied = untied.join("\n");
    assert_ne!(untied, text, "the file names its ties");
    let read = parse_flow_graph(&untied, &registry).unwrap_or_else(|err| panic!("{err}\n{untied}"));
    assert_eq!(read, graph);
}

/// The movement graph holding a node with a graph inside, and variables:
/// `stick_x` set from the move source; the inner graph reads it and sets
/// `doubled`.
fn nested_graph() -> FlowGraph {
    let stick_x = FlowVariable::new("stick_x", PortKind::Float);
    let doubled = FlowVariable {
        value: Some(FlowValue::Float(-1.0)),
        ..FlowVariable::new("doubled", PortKind::Float)
    };
    let mut graph = movement_graph();
    graph.variables = vec![stick_x.clone(), doubled.clone()];
    graph
        .nodes
        .push(stick_x.set_node("set_x", Vec2::new(260.0, 200.0)));
    graph
        .nodes
        .push(stick_x.get_node("get_x", Vec2::new(480.0, 200.0)));
    graph
        .links
        .push(FlowLink::new("move", "x", "set_x", "value"));
    let mut add = FlowNode {
        id: "add".into(),
        kind: "float/add".into(),
        title: "Add".into(),
        inputs: vec![
            PortDef::new("a", PortKind::Float),
            PortDef::new("b", PortKind::Float),
        ],
        outputs: vec![PortDef::new("result", PortKind::Float)],
        ..default()
    };
    add.position = Vec2::new(200.0, 0.0);
    let inner = FlowGraph {
        nodes: vec![
            stick_x.get_node("get_x", Vec2::ZERO),
            add,
            doubled.set_node("set_doubled", Vec2::new(400.0, 0.0)),
        ],
        links: vec![
            FlowLink::new("get_x", "value", "add", "a"),
            FlowLink::new("get_x", "value", "add", "b"),
            FlowLink::new("add", "result", "set_doubled", "value"),
        ],
        ..default()
    };
    graph.nodes.push(FlowNode {
        id: "machine".into(),
        kind: "test/machine".into(),
        title: "Machine".into(),
        graph: Some(inner),
        ..default()
    });
    graph
}

#[test]
fn variables_carry_values_without_wires_into_inner_graphs() {
    let (mut app, _) = app();
    let graph = nested_graph();
    let evaluation = evaluate_for(&mut app, &graph, Vec2::new(0.25, 0.0));
    let state = |id: &str| {
        evaluation
            .states
            .iter()
            .find(|state| state.id == id)
            .unwrap_or_else(|| panic!("no state {id}"))
    };
    // The get node beside its set node reads it the same step.
    assert_eq!(state("get_x").outputs, [FlowValue::Float(0.25)]);
    // Inner nodes play back under the holder's id, and see the outer set.
    assert_eq!(state("machine/get_x").outputs, [FlowValue::Float(0.25)]);
    assert_eq!(state("machine/add").outputs, [FlowValue::Float(0.5)]);
    assert_eq!(
        evaluation.variables.get("stick_x"),
        Some(FlowValue::Float(0.25))
    );
    assert_eq!(
        evaluation.variables.get("doubled"),
        Some(FlowValue::Float(0.5))
    );
}

#[test]
fn unset_variables_read_the_game_value_or_the_declared_one() {
    let (app, _) = app();
    let world = app.world();
    let declared = FlowVariable {
        value: Some(FlowValue::Bool(true)),
        ..FlowVariable::new("armed", PortKind::Bool)
    };
    let set_by_game = FlowVariable::new("speed", PortKind::Float);
    let graph = FlowGraph {
        nodes: vec![
            declared.get_node("armed", Vec2::ZERO),
            set_by_game.get_node("speed", Vec2::ZERO),
        ],
        variables: vec![declared, set_by_game],
        ..default()
    };
    let mut game = FlowVariables::default();
    game.set("speed", FlowValue::Float(3.5));
    let types = world.resource::<FlowNodeTypes>();
    let evaluation = evaluate(
        world,
        Entity::PLACEHOLDER,
        &graph,
        types,
        &game,
        &FlowNodeMemory::default(),
    );
    assert_eq!(evaluation.states[0].outputs, [FlowValue::Bool(true)]);
    assert_eq!(evaluation.states[1].outputs, [FlowValue::Float(3.5)]);
}

#[test]
fn a_runner_keeps_its_variables_between_steps() {
    let (mut app, _) = app();
    let graph = app
        .world_mut()
        .resource_mut::<Assets<FlowGraph>>()
        .add(nested_graph());
    let entity = app
        .world_mut()
        .spawn((Stick(Vec2::new(1.0, 0.0)), FlowGraphRunner { graph }))
        .id();
    app.world_mut().run_system_cached(run_flow_graphs).unwrap();
    let variables = app.world().get::<FlowVariables>(entity).unwrap();
    assert_eq!(variables.get("doubled"), Some(FlowValue::Float(2.0)));
    let playback = app.world().get::<FlowGraphPlayback>(entity).unwrap();
    assert!(playback.node("machine/set_doubled").is_some());
}

#[test]
fn bsn_round_trip_keeps_inner_graphs_and_variables() {
    let (app, _) = app();
    let registry = app.world().resource::<AppTypeRegistry>().read();
    let graph = nested_graph();
    let text = flow_graph_to_bsn(&graph, &registry);
    let read = parse_flow_graph(&text, &registry).unwrap_or_else(|err| panic!("{err}\n{text}"));
    assert_eq!(read, graph, "{text}");
    assert_eq!(
        read.node_at("machine/add").map(|n| n.id.as_str()),
        Some("add")
    );
    assert!(read.graph_at("machine").is_some());
}

#[test]
fn graphs_saved_before_inner_graphs_still_read() {
    let (app, _) = app();
    let registry = app.world().resource::<AppTypeRegistry>().read();
    let graph = movement_graph();
    let text = flow_graph_to_bsn(&graph, &registry);
    let old: Vec<&str> = text
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            !line.starts_with("graph:") && !line.starts_with("variables:")
        })
        .collect();
    let old = old.join("\n");
    assert_ne!(old, text);
    let read = parse_flow_graph(&old, &registry).unwrap_or_else(|err| panic!("{err}\n{old}"));
    assert_eq!(read, graph);
}

/// The outputs of registered type `kind` for `inputs`.
fn compute(app: &App, kind: &str, inputs: &[FlowValue]) -> Vec<FlowValue> {
    let types = app.world().resource::<FlowNodeTypes>();
    let node_type = types
        .get(kind)
        .unwrap_or_else(|| panic!("{kind} is registered"));
    assert_eq!(node_type.inputs.len(), inputs.len(), "{kind} inputs");
    let NodeRun::Compute(compute) = &node_type.run else {
        panic!("{kind} computes");
    };
    compute(app.world(), Entity::PLACEHOLDER, inputs)
}

#[test]
fn math_nodes_compute_and_stay_finite() {
    use FlowValue::{Bool, Float, Vec2 as V2, Vec3 as V3};
    let (app, _) = app();
    assert_eq!(
        compute(&app, "float/add", &[Float(1.0), Float(2.0)]),
        [Float(3.0)]
    );
    assert_eq!(
        compute(&app, "float/divide", &[Float(1.0), Float(0.0)]),
        [Float(0.0)]
    );
    assert_eq!(
        compute(&app, "float/lerp", &[Float(2.0), Float(4.0), Float(0.25)]),
        [Float(2.5)]
    );
    // Inverted limits do not panic.
    assert_eq!(
        compute(&app, "float/clamp", &[Float(5.0), Float(3.0), Float(1.0)]),
        [Float(1.0)]
    );
    assert_eq!(
        compute(&app, "float/greater", &[Float(2.0), Float(1.0)]),
        [Bool(true)]
    );
    assert_eq!(
        compute(&app, "vec2/scale", &[V2(Vec2::new(1.0, -2.0)), Float(3.0)]),
        [V2(Vec2::new(3.0, -6.0))]
    );
    assert_eq!(
        compute(&app, "vec2/normalize", &[V2(Vec2::ZERO)]),
        [V2(Vec2::ZERO)]
    );
    assert_eq!(
        compute(&app, "vec3/split", &[V3(Vec3::new(1.0, 2.0, 3.0))]),
        [Float(1.0), Float(2.0), Float(3.0)]
    );
    assert_eq!(
        compute(&app, "vec3/cross", &[V3(Vec3::X), V3(Vec3::Y)]),
        [V3(Vec3::Z)]
    );
    assert_eq!(
        compute(
            &app,
            "vec3/select",
            &[Bool(false), V3(Vec3::X), V3(Vec3::Y)]
        ),
        [V3(Vec3::Y)]
    );
    assert_eq!(compute(&app, "logic/not", &[Bool(false)]), [Bool(true)]);
}

#[test]
fn every_type_computes_one_value_per_output() {
    let (app, _) = app();
    let types = app.world().resource::<FlowNodeTypes>();
    for node_type in types.iter() {
        let NodeRun::Compute(compute) = &node_type.run else {
            continue;
        };
        let inputs: Vec<FlowValue> = node_type
            .inputs
            .iter()
            .map(|port| FlowValue::zero(port.kind))
            .collect();
        let outputs = compute(app.world(), Entity::PLACEHOLDER, &inputs);
        let kinds: Vec<PortKind> = outputs.iter().map(|value| value.kind()).collect();
        let declared: Vec<PortKind> = node_type.outputs.iter().map(|port| port.kind).collect();
        assert_eq!(kinds, declared, "{}", node_type.kind);
    }
}

#[test]
fn the_catalog_lists_every_type_and_round_trips() {
    let (app, _) = app();
    let mut catalog = app.world().resource::<FlowNodeTypes>().catalog();
    // What blend spaces preview with comes along.
    catalog.preview = BlendPreviewAssets {
        model: "character/model.gltf".into(),
        clips: vec![PreviewClip {
            kind: "clips/walk".into(),
            library: "animations/clips.glb".into(),
            animation: "walk".into(),
            duration: 1.2,
            sync: SyncCycles::new(2.0, 0.25),
        }],
    };
    let kinds: Vec<&str> = catalog
        .nodes
        .iter()
        .map(|node| node.kind.as_str())
        .collect();
    for kind in [
        nodes::MAKE_VECTOR2,
        "float/add",
        "vec3/make",
        "logic/and",
        "test/move",
    ] {
        assert!(kinds.contains(&kind), "{kind} in {kinds:?}");
    }
    assert!(kinds.is_sorted());
    assert!(catalog.nodes.iter().all(|node| node.id.is_empty()));

    let dir = std::env::temp_dir().join(format!("flowcatalog-test-{}", std::process::id()));
    let path = dir.join("flow").join(format!("nodes.{CATALOG_EXTENSION}"));
    let registry = app.world().resource::<AppTypeRegistry>().read();
    assert!(write_catalog_file(&path, &catalog, &registry).unwrap());
    // Unchanged: not rewritten, so file watchers see nothing.
    assert!(!write_catalog_file(&path, &catalog, &registry).unwrap());
    assert_eq!(read_catalog_file(&path, &registry).unwrap(), catalog);
    _ = std::fs::remove_dir_all(dir);
}

/// The crate's types come with it; the game's catalog file lists its own.
#[test]
fn the_project_catalog_leaves_out_the_builtins() {
    let (mut app, _) = app();
    let kinds = |catalog: &FlowNodeCatalog| -> Vec<String> {
        catalog.nodes.iter().map(|node| node.kind.clone()).collect()
    };
    let builtins = kinds(&builtin_catalog());
    assert!(builtins.contains(&"float/add".to_string()));
    assert!(builtins.contains(&nodes::MAKE_VECTOR2.to_string()));
    assert!(!builtins.contains(&"test/move".to_string()));

    // A built-in the game registers differently is the game's.
    app.register_flow_node(FlowNodeType::compute(
        "float/add",
        "Add Three",
        &[
            ("a", PortKind::Float),
            ("b", PortKind::Float),
            ("c", PortKind::Float),
        ],
        &[("sum", PortKind::Float)],
        |_, _, inputs| vec![FlowValue::Float(inputs.iter().map(|v| v.as_f32()).sum())],
    ));
    let project = kinds(&project_catalog(app.world().resource::<FlowNodeTypes>()));
    assert_eq!(project, ["float/add", "test/move", "test/sink"]);
}

/// A runner loads its graph file through the asset server, runs its sink
/// every fixed step and records the playback under the file's path.
#[test]
fn a_runner_runs_its_graph_file() {
    let (app, received) = app();
    let dir = std::env::temp_dir().join(format!("flowgraph-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let text = {
        let registry = app.world().resource::<AppTypeRegistry>().read();
        flow_graph_to_bsn(&movement_graph(), &registry)
    };
    std::fs::write(dir.join("movement.flowgraph.bsn"), text).unwrap();
    // Rebuilt on that directory: the asset root is read when the plugin builds.
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: dir.to_string_lossy().into_owned(),
            ..default()
        },
        FlowGraphPlugin,
    ));
    let sink = received.clone();
    app.register_flow_node(FlowNodeType::compute(
        "test/move",
        "Move",
        &[],
        &[("x", PortKind::Float), ("y", PortKind::Float)],
        |_, _, _| vec![FlowValue::Float(0.5), FlowValue::Float(-1.0)],
    ))
    .register_flow_node(FlowNodeType::apply(
        "test/sink",
        "Sink",
        &[("vector", PortKind::Vec2)],
        move |_, _, inputs| sink.0.lock().unwrap().push(inputs[0].as_vec2()),
    ));
    let graph = app
        .world()
        .resource::<AssetServer>()
        .load("movement.flowgraph.bsn");
    let runner = app.world_mut().spawn(FlowGraphRunner { graph }).id();

    let mut playback = None;
    for _ in 0..200 {
        app.update();
        std::thread::sleep(std::time::Duration::from_millis(5));
        playback = app.world().get::<FlowGraphPlayback>(runner).cloned();
        if playback.is_some() && !received.0.lock().unwrap().is_empty() {
            break;
        }
    }
    let playback = playback.expect("the graph ran");
    assert_eq!(playback.graph, "movement.flowgraph.bsn");
    assert!(playback.node("sink").is_some_and(FlowNodeState::is_active));
    assert_eq!(
        received.0.lock().unwrap().last(),
        Some(&Vec2::new(0.5, -1.0))
    );
    _ = std::fs::remove_dir_all(dir);
}

#[cfg(feature = "leafwing")]
mod leafwing {
    use ::leafwing_input_manager::prelude::*;

    use super::*;
    use crate::leafwing::{LeafwingFlowNodesPlugin, action_kind};

    #[derive(Actionlike, Reflect, Clone, Copy, PartialEq, Eq, Hash, Debug)]
    enum Action {
        Jump,
        #[actionlike(DualAxis)]
        Move,
        #[actionlike(Axis)]
        Zoom,
    }

    /// Every action becomes a node with no code beyond adding the plugin,
    /// its outputs following the action's control kind.
    #[test]
    fn every_action_becomes_a_node() {
        let (mut app, _) = app();
        app.add_plugins(LeafwingFlowNodesPlugin::<Action>::default());
        let types = app.world().resource::<FlowNodeTypes>();
        let outputs = |action: Action| {
            let node = types.get(&action_kind(&action)).expect("registered");
            node.outputs
                .iter()
                .map(|p| (p.name.as_str(), p.kind))
                .collect::<Vec<_>>()
        };
        assert_eq!(action_kind(&Action::Move), "leafwing/Action::Move");
        assert_eq!(outputs(Action::Jump), [("pressed", PortKind::Bool)]);
        assert_eq!(outputs(Action::Zoom), [("value", PortKind::Float)]);
        assert_eq!(
            outputs(Action::Move),
            [
                ("x", PortKind::Float),
                ("y", PortKind::Float),
                ("vector", PortKind::Vec2)
            ]
        );
    }

    #[test]
    fn an_action_node_reads_the_entity_action_state() {
        let (mut app, _) = app();
        app.add_plugins(LeafwingFlowNodesPlugin::<Action>::default());
        let mut state = ActionState::<Action>::default();
        state.set_axis_pair(&Action::Move, Vec2::new(0.5, 1.0));
        let entity = app.world_mut().spawn(state).id();
        let world = app.world();
        let types = world.resource::<FlowNodeTypes>();
        let node = types
            .get(&action_kind(&Action::Move))
            .unwrap()
            .instantiate("move", Vec2::ZERO);
        let graph = FlowGraph {
            nodes: vec![node],
            links: vec![],
            ..default()
        };
        let evaluation = evaluate(
            world,
            entity,
            &graph,
            types,
            &FlowVariables::default(),
            &FlowNodeMemory::default(),
        );
        assert_eq!(
            evaluation.states[0].outputs[2],
            FlowValue::Vec2(Vec2::new(0.5, 1.0))
        );
    }
}

#[test]
fn remembering_nodes_keep_their_memory_between_steps() {
    let (mut app, _) = app();
    app.register_flow_node(FlowNodeType::step(
        "test/count",
        "Count",
        &[],
        &[("count", PortKind::Float)],
        |_, _, _, memory| {
            let count = memory.first().map_or(0.0, |v| v.as_f32()) + 1.0;
            *memory = vec![FlowValue::Float(count)];
            vec![FlowValue::Float(count)]
        },
    ));
    let types = app.world().resource::<FlowNodeTypes>().clone();
    let counter = |id: &str| types.get("test/count").unwrap().instantiate(id, Vec2::ZERO);
    let mut holder = counter("outer");
    holder.graph = Some(FlowGraph {
        nodes: vec![counter("inner")],
        ..default()
    });
    let graph = app
        .world_mut()
        .resource_mut::<Assets<FlowGraph>>()
        .add(FlowGraph {
            nodes: vec![holder],
            ..default()
        });
    let entity = app.world_mut().spawn(FlowGraphRunner { graph }).id();
    for _ in 0..3 {
        app.world_mut().run_system_cached(run_flow_graphs).unwrap();
    }
    let playback = app.world().get::<FlowGraphPlayback>(entity).unwrap();
    for id in ["outer", "outer/inner"] {
        assert_eq!(playback.node(id).unwrap().outputs, [FlowValue::Float(3.0)]);
    }
}

#[test]
fn easings_run_from_zero_to_one_over_their_duration() {
    let close = |a: f32, b: f32| (a - b).abs() < 1e-3;
    for easing in [
        Easing::LINEAR,
        Easing::ease_in_out(1.0),
        Easing::ease_out(1.0),
    ] {
        assert!(close(easing.at(0.0), 0.0) && close(easing.at(1.0), 1.0));
        assert!((1..10).all(|i| easing.at(i as f32 / 10.0) < easing.at((i + 1) as f32 / 10.0)));
    }
    assert!(close(Easing::LINEAR.at(0.3), 0.3));
    assert!(close(Easing::ease_in_out(1.0).at(0.5), 0.5));
    assert!(Easing::ease_in_out(1.0).at(0.2) < 0.2);
    assert!(Easing::ease_out(1.0).at(0.2) > 0.2);
    let half_second = Easing::ease_in_out(0.5);
    assert!(close(half_second.after(0.25), 0.5) && close(half_second.after(2.0), 1.0));
    assert!(
        close(Easing::LINEAR.after(0.0), 1.0),
        "no duration: at once"
    );
}
