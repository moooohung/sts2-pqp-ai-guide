use super::*;

struct Point {
    coord: sts2_sim::run::Coord,
    kind: MapPointType,
    removed: bool,
    children: Vec<u8>,
}

#[derive(Clone, Copy)]
pub(super) struct RouteCount {
    pub count: i32,
    pub nearest: u16,
    pub farthest: u16,
}

pub(super) struct MapCache {
    points: Vec<Point>,
    pub order: Vec<usize>,
    pub canonical: [bool; 256],
    pub visible: [bool; 256],
    pub children: Vec<Vec<u8>>,
    routes: Vec<Option<Vec<RouteCount>>>,
}

impl Default for MapCache {
    fn default() -> Self {
        Self { points: Vec::new(), order: Vec::new(), canonical: [false; 256],
            visible: [false; 256], children: Vec::new(), routes: Vec::new() }
    }
}

impl MapCache {
    pub fn prepare(&mut self, map: &ActMap) {
        if self.points.len() == map.points.len() && self.points.iter().zip(&map.points).all(|(a, b)|
            a.coord == b.coord && a.kind == b.point_type && a.removed == b.is_removed() && a.children == b.children) {
            return;
        }
        self.points = map.points.iter().map(|p| Point { coord: p.coord, kind: p.point_type,
            removed: p.is_removed(), children: p.children.clone() }).collect();
        self.order.clear();
        self.order.extend(0..map.points.len());
        self.order.sort_unstable_by_key(|&i| (map.points[i].coord.row, map.points[i].coord.col));
        self.canonical.fill(false);
        self.visible.fill(false);
        for (i, point) in map.points.iter().enumerate() {
            if let Some(canonical) = map.point(point.coord) {
                self.visible[i] = true;
                self.canonical[i] = std::ptr::eq(canonical, point);
            }
        }
        self.children = map.points.iter().map(|p| {
            let mut children = p.children.clone();
            children.sort_unstable_by_key(|&i| {
                let coord = map.points[usize::from(i)].coord;
                (coord.row, coord.col)
            });
            children
        }).collect();
        self.routes.clear();
        self.routes.resize_with(map.points.len(), || None);
    }

    pub fn route(&mut self, map: &ActMap, start: usize) -> &[RouteCount] {
        self.routes[start].get_or_insert_with(|| {
            let mut distance = [u16::MAX; 256];
            let mut queue = [0usize; 256];
            let mut head = 0;
            let mut tail = 1;
            queue[0] = start;
            distance[start] = 0;
            while head < tail {
                let from = queue[head];
                head += 1;
                for &child in &map.points[from].children {
                    let child = usize::from(child);
                    if !self.canonical[child] || distance[child] != u16::MAX { continue; }
                    distance[child] = distance[from] + 1;
                    queue[tail] = child;
                    tail += 1;
                }
            }
            MapPointType::ALL.into_iter().map(|kind| {
                let mut result = RouteCount { count: 0, nearest: u16::MAX, farthest: 0 };
                for (index, point) in map.points.iter().enumerate() {
                    if point.point_type != kind || distance[index] == u16::MAX { continue; }
                    result.count += 1;
                    result.nearest = result.nearest.min(distance[index]);
                    result.farthest = result.farthest.max(distance[index]);
                }
                result
            }).collect()
        })
    }
}
