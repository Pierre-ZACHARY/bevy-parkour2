//! Paths around nodes, as react-flow-smart-edge finds them: A* on a grid
//! over the area, cells inside a node's rectangle blocked, then shortened
//! to the points that still see each other.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use bevy::math::{IVec2, Rect, Vec2};

/// Grid cell, in graph units.
const CELL: f32 = 10.0;
/// Most cells a search uses: larger areas get coarser cells.
const MAX_CELLS: f32 = 30_000.0;

/// Whether the segment `a`–`b` stays out of every rectangle.
pub fn clear(a: Vec2, b: Vec2, obstacles: &[Rect]) -> bool {
    let steps = ((b - a).length() / (CELL * 0.5)).ceil().max(1.0) as usize;
    (0..=steps).all(|step| {
        let point = a.lerp(b, step as f32 / steps as f32);
        !obstacles.iter().any(|rect| rect.contains(point))
    })
}

/// A path from `from` to `to`, both included, whose legs stay out of
/// `obstacles` (except where `from` or `to` lie inside one). `None` when the
/// search finds no way.
pub fn find_path(from: Vec2, to: Vec2, obstacles: &[Rect]) -> Option<Vec<Vec2>> {
    // A rectangle holding an end does not stop the wire leaving it.
    let obstacles: Vec<Rect> = obstacles
        .iter()
        .filter(|rect| !rect.contains(from) && !rect.contains(to))
        .copied()
        .collect();
    let obstacles = obstacles.as_slice();
    if clear(from, to, obstacles) {
        return Some(vec![from, to]);
    }
    let bounds = obstacles
        .iter()
        .fold(Rect::from_corners(from, to), |bounds, rect| {
            bounds.union(*rect)
        })
        .inflate(CELL * 4.0);
    let mut cell = CELL;
    while (bounds.width() / cell).ceil() * (bounds.height() / cell).ceil() > MAX_CELLS {
        cell *= 1.5;
    }
    let size = (bounds.size() / cell).ceil().as_ivec2().max(IVec2::ONE);
    let center = |c: IVec2| bounds.min + (c.as_vec2() + 0.5) * cell;
    let cell_of = |p: Vec2| {
        ((p - bounds.min) / cell)
            .floor()
            .as_ivec2()
            .clamp(IVec2::ZERO, size - 1)
    };
    let index = |c: IVec2| (c.y * size.x + c.x) as usize;
    let (start, goal) = (cell_of(from), cell_of(to));
    let mut blocked: Vec<bool> = (0..size.y)
        .flat_map(|y| (0..size.x).map(move |x| IVec2::new(x, y)))
        .map(|c| obstacles.iter().any(|rect| rect.contains(center(c))))
        .collect();
    // Their cells may still touch one.
    blocked[index(start)] = false;
    blocked[index(goal)] = false;
    let free = |c: IVec2| c.cmpge(IVec2::ZERO).all() && c.cmplt(size).all() && !blocked[index(c)];

    // Costs in tenths of a cell: 10 straight, 14 diagonal.
    let estimate = |c: IVec2| {
        let d = (c - goal).abs();
        (10 * d.max_element() + 4 * d.min_element()) as u32
    };
    let cells = (size.x * size.y) as usize;
    let mut cost = vec![u32::MAX; cells];
    let mut came_from = vec![usize::MAX; cells];
    let mut open = BinaryHeap::new();
    cost[index(start)] = 0;
    open.push(Reverse((estimate(start), index(start))));
    while let Some(Reverse((guess, current))) = open.pop() {
        let c = IVec2::new(current as i32 % size.x, current as i32 / size.x);
        if c == goal {
            break;
        }
        if guess > cost[current].saturating_add(estimate(c)) {
            continue;
        }
        for step in [
            IVec2::X,
            IVec2::NEG_X,
            IVec2::Y,
            IVec2::NEG_Y,
            IVec2::ONE,
            IVec2::NEG_ONE,
            IVec2::new(1, -1),
            IVec2::new(-1, 1),
        ] {
            let next = c + step;
            let diagonal = step.x != 0 && step.y != 0;
            // No cutting a blocked corner.
            if !free(next)
                || (diagonal
                    && !(free(c + IVec2::new(step.x, 0)) && free(c + IVec2::new(0, step.y))))
            {
                continue;
            }
            let reached = cost[current] + if diagonal { 14 } else { 10 };
            if reached < cost[index(next)] {
                cost[index(next)] = reached;
                came_from[index(next)] = current;
                open.push(Reverse((reached + estimate(next), index(next))));
            }
        }
    }
    if cost[index(goal)] == u32::MAX {
        return None;
    }
    let mut cells_back = vec![index(goal)];
    while let Some(&last) = cells_back.last()
        && last != index(start)
    {
        cells_back.push(came_from[last]);
    }
    let mut points: Vec<Vec2> = cells_back
        .iter()
        .rev()
        .map(|&i| center(IVec2::new(i as i32 % size.x, i as i32 / size.x)))
        .collect();
    // The ends where asked, not at their cells' centers.
    points[0] = from;
    *points.last_mut()? = to;
    if points.len() == 1 {
        points.push(to);
    }
    Some(shortcut(&points, obstacles))
}

/// From each point, straight to the farthest later one it sees.
fn shortcut(points: &[Vec2], obstacles: &[Rect]) -> Vec<Vec2> {
    let mut kept = vec![points[0]];
    let mut at = 0;
    while at + 1 < points.len() {
        let mut to = points.len() - 1;
        while to > at + 1 && !clear(points[at], points[to], obstacles) {
            to -= 1;
        }
        kept.push(points[to]);
        at = to;
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legs_clear(path: &[Vec2], obstacles: &[Rect]) -> bool {
        path.windows(2).all(|leg| clear(leg[0], leg[1], obstacles))
    }

    #[test]
    fn a_clear_line_is_kept() {
        let path = find_path(Vec2::ZERO, Vec2::new(200.0, 0.0), &[]).unwrap();
        assert_eq!(path, [Vec2::ZERO, Vec2::new(200.0, 0.0)]);
    }

    #[test]
    fn paths_go_around_a_node_with_few_points() {
        let node = Rect::new(80.0, -60.0, 140.0, 60.0);
        let (from, to) = (Vec2::ZERO, Vec2::new(240.0, 0.0));
        let path = find_path(from, to, &[node]).unwrap();
        assert_eq!((path[0], *path.last().unwrap()), (from, to));
        assert!(legs_clear(&path, &[node]), "{path:?}");
        assert!(path.len() <= 4, "{path:?}");
    }

    #[test]
    fn walled_in_ends_have_no_path() {
        let wall = Rect::new(-50.0, -50.0, 50.0, 50.0);
        let ring = [
            Rect::new(-200.0, -200.0, 200.0, -100.0),
            Rect::new(-200.0, 100.0, 200.0, 200.0),
            Rect::new(-200.0, -200.0, -100.0, 200.0),
            Rect::new(100.0, -200.0, 200.0, 200.0),
        ];
        assert!(find_path(Vec2::ZERO, Vec2::new(300.0, 0.0), &ring).is_none());
        // An end inside a rectangle still gets out of it.
        let path = find_path(Vec2::ZERO, Vec2::new(300.0, 0.0), &[wall]).unwrap();
        assert_eq!(*path.last().unwrap(), Vec2::new(300.0, 0.0));
    }
}
