//! The usual node-graph arithmetic and logic: one node per operation and
//! value type (`float/add`, `vec2/add`, `vec3/add`, ...), so every port has
//! a single kind. Results that are not finite (a division by zero) come out
//! as zero.

use std::ops::{Add, Div, Mul, Neg, Sub};

use bevy::prelude::*;

use crate::model::{FlowValue, PortKind};
use crate::registry::{FlowGraphAppExt, FlowNodeType};

/// A value type math nodes are made for.
trait Value:
    Copy
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
    + Neg<Output = Self>
    + Send
    + Sync
    + 'static
{
    /// The kinds' prefix, and the library category: `float`, `vec2`, `vec3`.
    const CATEGORY: &'static str;
    const KIND: PortKind;
    fn read(value: &FlowValue) -> Self;
    fn write(self) -> FlowValue;
    fn min(self, other: Self) -> Self;
    fn max(self, other: Self) -> Self;
    fn abs(self) -> Self;
    fn lerp(self, other: Self, t: f32) -> Self;
    /// Every component alue.
    fn splat(value: f32) -> Self;
}

impl Value for f32 {
    const CATEGORY: &'static str = "float";
    const KIND: PortKind = PortKind::Float;
    fn read(value: &FlowValue) -> Self {
        value.as_f32()
    }
    fn write(self) -> FlowValue {
        FlowValue::Float(if self.is_finite() { self } else { 0.0 })
    }
    fn min(self, other: Self) -> Self {
        f32::min(self, other)
    }
    fn max(self, other: Self) -> Self {
        f32::max(self, other)
    }
    fn abs(self) -> Self {
        f32::abs(self)
    }
    fn lerp(self, other: Self, t: f32) -> Self {
        self + (other - self) * t
    }
    fn splat(value: f32) -> Self {
        value
    }
}

impl Value for Vec2 {
    const CATEGORY: &'static str = "vec2";
    const KIND: PortKind = PortKind::Vec2;
    fn read(value: &FlowValue) -> Self {
        value.as_vec2()
    }
    fn write(self) -> FlowValue {
        FlowValue::Vec2(if self.is_finite() { self } else { Vec2::ZERO })
    }
    fn min(self, other: Self) -> Self {
        Vec2::min(self, other)
    }
    fn max(self, other: Self) -> Self {
        Vec2::max(self, other)
    }
    fn abs(self) -> Self {
        Vec2::abs(self)
    }
    fn lerp(self, other: Self, t: f32) -> Self {
        Vec2::lerp(self, other, t)
    }
    fn splat(value: f32) -> Self {
        Vec2::splat(value)
    }
}

impl Value for Vec3 {
    const CATEGORY: &'static str = "vec3";
    const KIND: PortKind = PortKind::Vec3;
    fn read(value: &FlowValue) -> Self {
        value.as_vec3()
    }
    fn write(self) -> FlowValue {
        FlowValue::Vec3(if self.is_finite() { self } else { Vec3::ZERO })
    }
    fn min(self, other: Self) -> Self {
        Vec3::min(self, other)
    }
    fn max(self, other: Self) -> Self {
        Vec3::max(self, other)
    }
    fn abs(self) -> Self {
        Vec3::abs(self)
    }
    fn lerp(self, other: Self, t: f32) -> Self {
        Vec3::lerp(self, other, t)
    }
    fn splat(value: f32) -> Self {
        Vec3::splat(value)
    }
}

fn kind<T: Value>(operation: &str) -> String {
    format!("{}/{operation}", T::CATEGORY)
}

/// `a`, `b` → `result`.
fn binary<T: Value>(app: &mut App, operation: &str, title: &str, f: fn(T, T) -> T) {
    app.register_flow_node(FlowNodeType::compute(
        kind::<T>(operation),
        title,
        &[("a", T::KIND), ("b", T::KIND)],
        &[("result", T::KIND)],
        move |_, _, inputs| vec![f(T::read(&inputs[0]), T::read(&inputs[1])).write()],
    ));
}

/// `value` → `result`.
fn unary<T: Value>(app: &mut App, operation: &str, title: &str, f: fn(T) -> T) {
    app.register_flow_node(FlowNodeType::compute(
        kind::<T>(operation),
        title,
        &[("value", T::KIND)],
        &[("result", T::KIND)],
        move |_, _, inputs| vec![f(T::read(&inputs[0])).write()],
    ));
}

/// What every value type gets.
fn register_arithmetic<T: Value>(app: &mut App) {
    binary::<T>(app, "add", "Add", |a, b| a + b);
    binary::<T>(app, "subtract", "Subtract", |a, b| a - b);
    binary::<T>(app, "multiply", "Multiply", |a, b| a * b);
    binary::<T>(app, "divide", "Divide", |a, b| a / b);
    binary::<T>(app, "min", "Min", T::min);
    binary::<T>(app, "max", "Max", T::max);
    unary::<T>(app, "abs", "Abs", T::abs);
    unary::<T>(app, "negate", "Negate", |value| -value);
    app.register_flow_node(FlowNodeType::compute(
        kind::<T>("lerp"),
        "Lerp",
        &[("a", T::KIND), ("b", T::KIND), ("t", PortKind::Float)],
        &[("result", T::KIND)],
        |_, _, inputs| {
            let (a, b) = (T::read(&inputs[0]), T::read(&inputs[1]));
            vec![a.lerp(b, inputs[2].as_f32()).write()]
        },
    ))
    .register_flow_node(FlowNodeType::compute(
        kind::<T>("select"),
        "Select",
        &[
            ("condition", PortKind::Bool),
            ("if_true", T::KIND),
            ("if_false", T::KIND),
        ],
        &[("result", T::KIND)],
        |_, _, inputs| {
            vec![if inputs[0].as_bool() {
                inputs[1].clone()
            } else {
                inputs[2].clone()
            }]
        },
    ));
}

const FLOATS: &[(&str, PortKind)] = &[("a", PortKind::Float), ("b", PortKind::Float)];

fn register_float(app: &mut App) {
    register_arithmetic::<f32>(app);
    app.register_flow_node(FlowNodeType::compute(
        "float/clamp",
        "Clamp",
        &[
            ("value", PortKind::Float),
            ("min", PortKind::Float),
            ("max", PortKind::Float),
        ],
        &[("result", PortKind::Float)],
        |_, _, inputs| {
            let (value, min, max) = (inputs[0].as_f32(), inputs[1].as_f32(), inputs[2].as_f32());
            // Not `clamp`, which panics on inverted limits.
            vec![value.max(min).min(max).write()]
        },
    ))
    .register_flow_node(FlowNodeType::compute(
        "float/greater",
        "Greater",
        FLOATS,
        &[("result", PortKind::Bool)],
        |_, _, inputs| vec![FlowValue::Bool(inputs[0].as_f32() > inputs[1].as_f32())],
    ))
    .register_flow_node(FlowNodeType::compute(
        "float/less",
        "Less",
        FLOATS,
        &[("result", PortKind::Bool)],
        |_, _, inputs| vec![FlowValue::Bool(inputs[0].as_f32() < inputs[1].as_f32())],
    ));
}

/// What vectors get on top of arithmetic.
fn register_vector<T: Value>(
    app: &mut App,
    length: fn(T) -> f32,
    normalize: fn(T) -> T,
    dot: fn(T, T) -> f32,
) {
    app.register_flow_node(FlowNodeType::compute(
        kind::<T>("scale"),
        "Scale",
        &[("vector", T::KIND), ("factor", PortKind::Float)],
        &[("result", T::KIND)],
        |_, _, inputs| {
            let (vector, factor) = (T::read(&inputs[0]), inputs[1].as_f32());
            vec![(vector * T::splat(factor)).write()]
        },
    ))
    .register_flow_node(FlowNodeType::compute(
        kind::<T>("length"),
        "Length",
        &[("vector", T::KIND)],
        &[("length", PortKind::Float)],
        move |_, _, inputs| vec![length(T::read(&inputs[0])).write()],
    ))
    .register_flow_node(FlowNodeType::compute(
        kind::<T>("dot"),
        "Dot",
        &[("a", T::KIND), ("b", T::KIND)],
        &[("result", PortKind::Float)],
        move |_, _, inputs| vec![dot(T::read(&inputs[0]), T::read(&inputs[1])).write()],
    ));
    unary::<T>(app, "normalize", "Normalize", normalize);
}

fn register_vec2(app: &mut App) {
    register_arithmetic::<Vec2>(app);
    register_vector::<Vec2>(app, Vec2::length, Vec2::normalize_or_zero, Vec2::dot);
    app.register_flow_node(FlowNodeType::compute(
        "vec2/split",
        "Split",
        &[("vector", PortKind::Vec2)],
        &[("x", PortKind::Float), ("y", PortKind::Float)],
        |_, _, inputs| {
            let vector = inputs[0].as_vec2();
            vec![FlowValue::Float(vector.x), FlowValue::Float(vector.y)]
        },
    ));
}

fn register_vec3(app: &mut App) {
    register_arithmetic::<Vec3>(app);
    register_vector::<Vec3>(app, Vec3::length, Vec3::normalize_or_zero, Vec3::dot);
    app.register_flow_node(FlowNodeType::compute(
        "vec3/cross",
        "Cross",
        &[("a", PortKind::Vec3), ("b", PortKind::Vec3)],
        &[("result", PortKind::Vec3)],
        |_, _, inputs| vec![inputs[0].as_vec3().cross(inputs[1].as_vec3()).write()],
    ))
    .register_flow_node(FlowNodeType::compute(
        "vec3/make",
        "Make Vector 3",
        &[
            ("x", PortKind::Float),
            ("y", PortKind::Float),
            ("z", PortKind::Float),
        ],
        &[("vector", PortKind::Vec3)],
        |_, _, inputs| {
            let [x, y, z] = [0, 1, 2].map(|i| inputs[i].as_f32());
            vec![Vec3::new(x, y, z).write()]
        },
    ))
    .register_flow_node(FlowNodeType::compute(
        "vec3/split",
        "Split",
        &[("vector", PortKind::Vec3)],
        &[
            ("x", PortKind::Float),
            ("y", PortKind::Float),
            ("z", PortKind::Float),
        ],
        |_, _, inputs| {
            let vector = inputs[0].as_vec3();
            vector.to_array().map(FlowValue::Float).to_vec()
        },
    ));
}

const BOOLS: &[(&str, PortKind)] = &[("a", PortKind::Bool), ("b", PortKind::Bool)];

fn register_logic(app: &mut App) {
    app.register_flow_node(FlowNodeType::compute(
        "logic/and",
        "And",
        BOOLS,
        &[("result", PortKind::Bool)],
        |_, _, inputs| vec![FlowValue::Bool(inputs[0].as_bool() && inputs[1].as_bool())],
    ))
    .register_flow_node(FlowNodeType::compute(
        "logic/or",
        "Or",
        BOOLS,
        &[("result", PortKind::Bool)],
        |_, _, inputs| vec![FlowValue::Bool(inputs[0].as_bool() || inputs[1].as_bool())],
    ))
    .register_flow_node(FlowNodeType::compute(
        "logic/not",
        "Not",
        &[("value", PortKind::Bool)],
        &[("result", PortKind::Bool)],
        |_, _, inputs| vec![FlowValue::Bool(!inputs[0].as_bool())],
    ));
}

pub(crate) fn register(app: &mut App) {
    register_float(app);
    register_vec2(app);
    register_vec3(app);
    register_logic(app);
}
