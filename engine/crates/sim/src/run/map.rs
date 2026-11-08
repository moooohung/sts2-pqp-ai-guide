//! Frozen-r7 standard act-map generation.
//!
//! The point graph is built only at act entry. Run stepping reads the compact
//! coordinate/edge vectors and does not allocate.

use std::collections::{BTreeMap, VecDeque};

use crate::rng::Rng;
use crate::tables::MapPointType;

#[path = "spoils_map.rs"]
mod spoils_map;
#[cfg(test)]
mod segment_tests;
pub use spoils_map::generate_spoils_act_map;

pub const MAP_WIDTH: i32 = 7;
const MAP_ITERATIONS: usize = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Coord {
    pub col: i32,
    pub row: i32,
}

impl Coord {
    pub const fn new(col: i32, row: i32) -> Self {
        Self { col, row }
    }
}

#[derive(Clone, Debug)]
pub struct MapPoint {
    pub coord: Coord,
    pub point_type: MapPointType,
    pub children: Vec<u8>,
    pub parents: Vec<u8>,
    pub can_be_modified: bool,
    // SpoilsMap.AfterMapGenerated attaches the deck instances at SpoilsCoord.
    pub(super) spoils_quests: Vec<u32>,
    removed: bool,
}

impl MapPoint {
    pub fn is_removed(&self) -> bool { self.removed }

    fn new(coord: Coord) -> Self {
        Self {
            coord,
            point_type: MapPointType::Unassigned,
            children: Vec::with_capacity(3),
            parents: Vec::with_capacity(3),
            can_be_modified: true,
            spoils_quests: Vec::new(),
            removed: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ActMap {
    pub map_length: i32,
    pub points: Vec<MapPoint>,
    pub start: u8,
    pub boss: u8,
    pub second_boss: Option<u8>,
}

impl ActMap {
    /// FurCoat.cs AddMarkedRooms; ActMap.GetAllMapPoints is column-major.
    /// Rng(Player, ModelId) seeds a private stream from run seed + slot + ID hash.
    pub(super) fn fur_coat_coords(&self,seed:u64,owner:usize,relics:&[crate::relic::Relic])->Vec<(i32,i32)>{
        let mut candidates:Vec<_>=self.points.iter().filter(|p|!p.removed
            && matches!(p.point_type,MapPointType::Monster|MapPointType::Elite))
            .filter(|p|!relics.iter().any(|r|r.id==crate::tables::RelicId::FurCoat
                &&r.fur_coat_has_coord((p.coord.col,p.coord.row))))
            .map(|p|(p.coord.col,p.coord.row)).collect();
        candidates.sort_unstable();
        let mut rng=Rng::new(seed.wrapping_add(owner as u64)
            .wrapping_add(crate::rng::deterministic_hash64("FUR_COAT")));
        rng.shuffle(&mut candidates);
        candidates.truncate(8);candidates
    }
    fn new(num_rooms: usize) -> Self {
        Self {
            map_length: num_rooms as i32 + 1,
            points: Vec::with_capacity(96),
            start: 0,
            boss: 0,
            second_boss: None,
        }
    }

    fn get_index(&self, coord: Coord) -> Option<u8> {
        self.points
            .iter()
            .enumerate()
            .find(|(_, p)| !p.removed && p.coord == coord)
            .map(|(i, _)| i as u8)
    }

    fn get_or_create(&mut self, coord: Coord) -> u8 {
        if let Some(index) = self.get_index(coord) {
            return index;
        }
        let index = self.points.len();
        assert!(index < u8::MAX as usize, "r7 map point capacity");
        self.points.push(MapPoint::new(coord));
        index as u8
    }

    pub fn point(&self, coord: Coord) -> Option<&MapPoint> {
        self.get_index(coord).map(|i| &self.points[i as usize])
    }

    pub fn point_index(&self, coord: Coord) -> Option<u8> {
        self.get_index(coord)
    }

    pub fn row_indices(&self, row: i32) -> Vec<u8> {
        let mut out: Vec<u8> = self
            .points
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.removed && p.coord.row == row)
            .map(|(i, _)| i as u8)
            .collect();
        out.sort_by_key(|&i| self.points[i as usize].coord.col);
        out
    }

    fn room_indices(&self) -> Vec<u8> {
        self.points
            .iter()
            .enumerate()
            .filter(|(i, p)| {
                !p.removed
                    && *i as u8 != self.start
                    && *i as u8 != self.boss
                    && self.second_boss != Some(*i as u8)
            })
            .map(|(i, _)| i as u8)
            .collect()
    }

    fn add_child(&mut self, parent: u8, child: u8) {
        if !self.points[parent as usize].children.contains(&child) {
            self.points[parent as usize].children.push(child);
        }
        if !self.points[child as usize].parents.contains(&parent) {
            self.points[child as usize].parents.push(parent);
        }
    }

    fn remove_child(&mut self, parent: u8, child: u8) {
        self.points[parent as usize]
            .children
            .retain(|&x| x != child);
        self.points[child as usize].parents.retain(|&x| x != parent);
    }

    fn siblings(&self, point: u8) -> Vec<u8> {
        let mut out = Vec::new();
        for &parent in &self.points[point as usize].parents {
            for &child in &self.points[parent as usize].children {
                if child != point && !out.contains(&child) {
                    out.push(child);
                }
            }
        }
        out
    }

    fn move_room_point(&mut self, point: u8, new_col: i32) {
        let row = self.points[point as usize].coord.row;
        if self.points[point as usize].coord.col == new_col {
            return;
        }
        assert!(
            self.get_index(Coord::new(new_col, row)).is_none(),
            "map postprocess moved into occupied cell"
        );
        self.points[point as usize].coord.col = new_col;
    }

    fn remove_point(&mut self, point: u8) {
        let children = self.points[point as usize].children.clone();
        let parents = self.points[point as usize].parents.clone();
        for child in children {
            self.remove_child(point, child);
        }
        for parent in parents {
            self.remove_child(parent, point);
        }
        self.points[point as usize].removed = true;
    }
}

fn invalid_crossover(map: &ActMap, current: u8, target_col: i32) -> bool {
    let current_col = map.points[current as usize].coord.col;
    let direction = target_col - current_col;
    if direction == 0 {
        return false;
    }
    let row = map.points[current as usize].coord.row;
    let Some(blocker) = map.get_index(Coord::new(target_col, row)) else {
        return false;
    };
    let blocker_col = map.points[blocker as usize].coord.col;
    map.points[blocker as usize]
        .children
        .iter()
        .any(|&child| map.points[child as usize].coord.col - blocker_col == -direction)
}

fn generate_next_coord(map: &ActMap, current: u8, rng: &mut Rng) -> Coord {
    let coord = map.points[current as usize].coord;
    let min_col = 0.max(coord.col - 1);
    let max_col = (MAP_WIDTH - 1).min(coord.col + 1);
    let mut directions = [-1i32, 0, 1];
    rng.shuffle(&mut directions);
    for direction in directions {
        let target_col = match direction {
            -1 => min_col,
            0 => coord.col,
            _ => max_col,
        };
        if !invalid_crossover(map, current, target_col) {
            return Coord::new(target_col, coord.row + 1);
        }
    }
    Coord::new(coord.col, coord.row + 1)
}

fn generate_path(map: &mut ActMap, start: u8, rng: &mut Rng) {
    let mut current = start;
    while map.points[current as usize].coord.row < map.map_length - 1 {
        let next_coord = generate_next_coord(map, current, rng);
        let next = map.get_or_create(next_coord);
        map.add_child(current, next);
        current = next;
    }
}

fn type_valid(map: &ActMap, point: u8, point_type: MapPointType) -> bool {
    let p = &map.points[point as usize];
    let row = p.coord.row;
    if row < 6 && matches!(point_type, MapPointType::RestSite | MapPointType::Elite) {
        return false;
    }
    if row >= map.map_length - 3 && point_type == MapPointType::RestSite {
        return false;
    }
    let restricted = matches!(
        point_type,
        MapPointType::Elite | MapPointType::RestSite | MapPointType::Treasure | MapPointType::Shop
    );
    if restricted
        && p.parents
            .iter()
            .any(|&i| map.points[i as usize].point_type == point_type)
    {
        return false;
    }
    if restricted
        && p.children
            .iter()
            .any(|&i| map.points[i as usize].point_type == point_type)
    {
        return false;
    }
    let sibling_restricted = matches!(
        point_type,
        MapPointType::RestSite
            | MapPointType::Monster
            | MapPointType::Unknown
            | MapPointType::Elite
            | MapPointType::Shop
    );
    if sibling_restricted
        && map
            .siblings(point)
            .iter()
            .any(|&i| map.points[i as usize].point_type == point_type)
    {
        return false;
    }
    true
}

fn point_type_counts(
    rng: &mut Rng,
    ascension: i32,
    act_index: usize,
) -> (usize, usize, usize, usize) {
    let elites = if ascension >= 1 { 8 } else { 5 };
    match act_index {
        0 => {
            // r7 room_generation.py: rest draw happens before unknown draw.
            let rests = rng.next_gaussian_int(7.0, 1.0, 6, 7) as usize;
            let unknowns = rng.next_gaussian_int(12.0, 1.0, 10, 14) as usize;
            (elites, 3, unknowns, rests)
        }
        1 => {
            let rests = rng.next_gaussian_int(6.0, 1.0, 6, 7) as usize;
            let unknowns = (rng.next_gaussian_int(12.0, 1.0, 10, 14) - 1) as usize;
            (elites, 3, unknowns, rests)
        }
        2 => {
            let rests = rng.next_int_exclusive(5, 7) as usize;
            let unknowns = (rng.next_gaussian_int(12.0, 1.0, 10, 14) - 1) as usize;
            (elites, 3, unknowns, rests)
        }
        _ => {
            let rests = rng.next_gaussian_int(5.0, 1.0, 3, 6) as usize;
            let unknowns = rng.next_gaussian_int(12.0, 1.0, 10, 14) as usize;
            (elites, 3, unknowns, rests)
        }
    }
}

fn assign_room_types(map: &mut ActMap, rng: &mut Rng, counts: (usize, usize, usize, usize)) {
    let (num_elites, num_shops, num_unknowns, num_rests) = counts;
    for point in map.row_indices(map.map_length - 1) {
        map.points[point as usize].point_type = MapPointType::RestSite;
        map.points[point as usize].can_be_modified = false;
    }
    let treasure_row = map.map_length - 7;
    if treasure_row > 0 {
        for point in map.row_indices(treasure_row) {
            map.points[point as usize].point_type = MapPointType::Treasure;
            map.points[point as usize].can_be_modified = false;
        }
    }
    for point in map.row_indices(1) {
        map.points[point as usize].point_type = MapPointType::Monster;
        map.points[point as usize].can_be_modified = false;
    }

    let mut pool = VecDeque::new();
    pool.extend(std::iter::repeat_n(MapPointType::RestSite, num_rests));
    pool.extend(std::iter::repeat_n(MapPointType::Shop, num_shops));
    pool.extend(std::iter::repeat_n(MapPointType::Elite, num_elites));
    pool.extend(std::iter::repeat_n(MapPointType::Unknown, num_unknowns));

    for _ in 0..3 {
        if pool.is_empty() {
            break;
        }
        let mut unassigned: Vec<u8> = map
            .room_indices()
            .into_iter()
            .filter(|&i| map.points[i as usize].point_type == MapPointType::Unassigned)
            .collect();
        unassigned.sort_by_key(|&i| {
            let c = map.points[i as usize].coord;
            (c.col, c.row)
        });
        rng.shuffle(&mut unassigned);
        for point in unassigned {
            if pool.is_empty() {
                break;
            }
            let n = pool.len();
            for _ in 0..n {
                let candidate = pool.pop_front().unwrap();
                if type_valid(map, point, candidate) {
                    map.points[point as usize].point_type = candidate;
                    break;
                }
                pool.push_back(candidate);
            }
        }
    }
    for point in map.room_indices() {
        if map.points[point as usize].point_type == MapPointType::Unassigned {
            map.points[point as usize].point_type = MapPointType::Monster;
        }
    }
    map.points[map.boss as usize].point_type = MapPointType::Boss;
    if let Some(second) = map.second_boss {
        map.points[second as usize].point_type = MapPointType::Boss;
    }
    map.points[map.start as usize].point_type = MapPointType::Ancient;
}

fn dll_type_value(t: MapPointType) -> u8 {
    match t {
        MapPointType::Unassigned => 0,
        MapPointType::Unknown => 1,
        MapPointType::Shop => 2,
        MapPointType::Treasure => 3,
        MapPointType::RestSite => 4,
        MapPointType::Monster => 5,
        MapPointType::Elite => 6,
        MapPointType::Boss => 7,
        MapPointType::Ancient => 8,
    }
}

fn valid_segment_start(map: &ActMap, point: u8) -> bool {
    map.points[point as usize].children.len() > 1 || map.points[point as usize].coord.row == 0
}

fn valid_segment_end(map: &ActMap, point: u8) -> bool {
    map.points[point as usize].parents.len() >= 2
}

fn reachable_dfs(map: &ActMap, point: u8, seen: &mut Vec<u8>, starts: &mut Vec<u8>) {
    if seen.contains(&point) {
        return;
    }
    seen.push(point);
    if valid_segment_start(map, point) {
        starts.push(point);
    }
    for &child in &map.points[point as usize].children {
        reachable_dfs(map, child, seen, starts);
    }
}

fn segment_key(map: &ActMap, path: &[u8]) -> String {
    let first = map.points[path[0] as usize].coord;
    let last = map.points[*path.last().unwrap() as usize].coord;
    let mut key = if first.row == 0 {
        format!("{}-{},{}-", first.row, last.col, last.row)
    } else {
        format!("{},{}-{},{}-", first.col, first.row, last.col, last.row)
    };
    for (index, &p) in path.iter().enumerate() {
        if index != 0 {
            key.push(',');
        }
        key.push(char::from(
            b'0' + dll_type_value(map.points[p as usize].point_type),
        ));
    }
    key
}

fn walk_segments(
    map: &ActMap,
    current: u8,
    path: &mut Vec<u8>,
    buckets: &mut BTreeMap<Vec<u8>, Vec<Vec<u8>>>,
) {
    path.push(current);
    if path.len() >= 3 && valid_segment_end(map, current) {
        // Equality needs only endpoint coordinates and the exact type sequence.
        // Format the C# ordinal-sort string once per retained duplicate bucket,
        // not on every visit along every map path.
        let first=map.points[path[0] as usize].coord;
        let last=map.points[current as usize].coord;
        let mut key=Vec::with_capacity(16+path.len());
        for coordinate in [if first.row==0 {0}else{first.col as i32},first.row as i32,last.col as i32,last.row as i32] {
            key.extend_from_slice(&coordinate.to_le_bytes());
        }
        key.extend(path.iter().map(|&p|dll_type_value(map.points[p as usize].point_type)));
        let bucket = buckets.entry(key).or_default();
        let overlaps = bucket.iter().any(|existing| {
            let limit = path.len().min(existing.len()).saturating_sub(1);
            (1..limit).any(|i| existing[i] == path[i])
        });
        if !overlaps {
            bucket.push(path.clone());
        }
    }
    for &child in &map.points[current as usize].children {
        walk_segments(map, child, path, buckets);
    }
    path.pop();
}

fn matching_segments(map: &ActMap) -> Vec<Vec<Vec<u8>>> {
    let mut seen = Vec::new();
    let mut starts = Vec::new();
    reachable_dfs(map, map.start, &mut seen, &mut starts);
    let mut buckets: BTreeMap<Vec<u8>, Vec<Vec<u8>>> = BTreeMap::new();
    for start in starts {
        walk_segments(map, start, &mut Vec::new(), &mut buckets);
    }
    let mut duplicates:Vec<_>=buckets
        .into_values()
        .filter(|bucket| bucket.len() > 1)
        .map(|bucket|(segment_key(map,&bucket[0]),bucket))
        .collect();
    duplicates.sort_by(|a,b|a.0.cmp(&b.0));
    duplicates.into_iter().map(|(_,bucket)|bucket).collect()
}

fn is_in_map(map: &ActMap, point: u8) -> bool {
    let p = &map.points[point as usize];
    if matches!(p.point_type, MapPointType::Ancient | MapPointType::Boss) {
        return true;
    }
    !p.removed && map.get_index(p.coord) == Some(point)
}

fn is_removed(map: &ActMap, point: u8) -> bool {
    if point == map.start || point == map.boss || map.second_boss == Some(point) {
        return true;
    }
    let p = &map.points[point as usize];
    p.removed || map.get_index(p.coord) != Some(point)
}

fn prune_segment(map: &mut ActMap, segment: &[u8]) -> bool {
    let mut result = false;
    for (index, &point) in segment[..segment.len() - 1].iter().enumerate() {
        if !is_in_map(map, point) {
            return true;
        }
        let children = map.points[point as usize].children.clone();
        let parents = map.points[point as usize].parents.clone();
        if children.len() > 1
            || parents.len() > 1
            || parents.iter().any(|&parent| {
                map.points[parent as usize].children.len() == 1 && !is_removed(map, parent)
            })
        {
            continue;
        }
        if segment[index..].iter().any(|&node| {
            map.points[node as usize].children.len() > 1
                && map.points[node as usize].parents.len() == 1
        }) {
            continue;
        }
        let last = *segment.last().unwrap();
        if map.points[last as usize].parents.len() == 1 {
            return false;
        }
        let outside_single_parent = children.iter().any(|&child| {
            !segment.contains(&child) && map.points[child as usize].parents.len() == 1
        });
        if !outside_single_parent {
            map.remove_point(point);
            result = true;
        }
    }
    result
}

fn prune_all_but_last(map: &mut ActMap, matches: &[Vec<u8>]) -> usize {
    let mut pruned = 0;
    for segment in matches {
        if pruned == matches.len().saturating_sub(1) {
            return pruned;
        }
        if prune_segment(map, segment) {
            pruned += 1;
        }
    }
    pruned
}

fn break_relationship(map: &mut ActMap, segment: &[u8]) -> bool {
    let mut result = false;
    for index in 0..segment.len() - 1 {
        let point = segment[index];
        if map.points[point as usize].children.len() < 2 {
            continue;
        }
        let next = segment[index + 1];
        if map.points[next as usize].parents.len() != 1 {
            map.remove_child(point, next);
            result = true;
        }
    }
    result
}

fn prune_paths(map: &mut ActMap, matching: &mut [Vec<Vec<u8>>], rng: &mut Rng) -> bool {
    for matches in matching {
        rng.shuffle(matches);
        if prune_all_but_last(map, matches) != 0 {
            return true;
        }
        for segment in matches.iter() {
            if break_relationship(map, segment) {
                return true;
            }
        }
    }
    false
}

fn prune_duplicate_segments(map: &mut ActMap, rng: &mut Rng) {
    let mut matching = matching_segments(map);
    let mut iterations = 0;
    while prune_paths(map, &mut matching, rng) {
        iterations += 1;
        assert!(iterations <= 50, "unable to prune matching r7 map segments");
        matching = matching_segments(map);
    }
}

fn repair_type(
    map: &mut ActMap,
    point_type: MapPointType,
    target_count: usize,
    rng: &mut Rng,
) -> bool {
    let current = map
        .room_indices()
        .iter()
        .filter(|&&i| map.points[i as usize].point_type == point_type)
        .count();
    let mut missing = target_count.saturating_sub(current);
    if missing == 0 {
        return false;
    }
    let mut candidates: Vec<u8> = map
        .room_indices()
        .into_iter()
        .filter(|&i| {
            let p = &map.points[i as usize];
            p.point_type == MapPointType::Monster && p.can_be_modified
        })
        .collect();
    candidates.sort_by_key(|&i| {
        let c = map.points[i as usize].coord;
        (c.col, c.row)
    });
    rng.shuffle(&mut candidates);
    let mut changed = false;
    for point in candidates {
        if missing == 0 {
            break;
        }
        if type_valid(map, point, point_type) {
            map.points[point as usize].point_type = point_type;
            missing -= 1;
            changed = true;
        }
    }
    changed
}

fn prune_and_repair(map: &mut ActMap, counts: (usize, usize, usize, usize), rng: &mut Rng) {
    let (elites, shops, unknowns, rests) = counts;
    for _ in 0..3 {
        prune_duplicate_segments(map, rng);
        let mut changed = false;
        changed |= repair_type(map, MapPointType::Shop, shops, rng);
        changed |= repair_type(map, MapPointType::Elite, elites, rng);
        changed |= repair_type(map, MapPointType::RestSite, rests, rng);
        changed |= repair_type(map, MapPointType::Unknown, unknowns, rng);
        if !changed {
            break;
        }
    }
}

fn center_grid(map: &mut ActMap) {
    let room_points = map.room_indices();
    let cols_used: Vec<i32> = room_points
        .iter()
        .map(|&i| map.points[i as usize].coord.col)
        .collect();
    let left_empty = !cols_used.contains(&0) && !cols_used.contains(&1);
    let right_empty =
        !cols_used.contains(&(MAP_WIDTH - 1)) && !cols_used.contains(&(MAP_WIDTH - 2));
    let shift = if left_empty && !right_empty {
        -1
    } else if !left_empty && right_empty {
        1
    } else {
        return;
    };
    let mut points = room_points;
    points.sort_by_key(|&i| {
        let c = map.points[i as usize].coord;
        (c.row, if shift > 0 { -c.col } else { c.col })
    });
    for point in points {
        let col = map.points[point as usize].coord.col;
        map.move_room_point(point, col + shift);
    }
}

fn allowed_positions(map: &ActMap, point: u8) -> Vec<i32> {
    let mut allowed: Vec<i32> = (0..MAP_WIDTH).collect();
    for neighbor in map.points[point as usize]
        .parents
        .iter()
        .chain(map.points[point as usize].children.iter())
    {
        let col = map.points[*neighbor as usize].coord.col;
        allowed.retain(|&candidate| (candidate - col).abs() <= 1);
    }
    allowed
}

fn compute_gap(map: &ActMap, candidate: i32, row_points: &[u8], current: u8) -> i32 {
    row_points
        .iter()
        .copied()
        .filter(|&p| p != current)
        .map(|p| (candidate - map.points[p as usize].coord.col).abs())
        .min()
        .unwrap_or(i32::MAX)
}

fn spread_adjacent(map: &mut ActMap) {
    for row in 0..map.map_length {
        let row_points = map.row_indices(row);
        let mut moved = true;
        while moved {
            moved = false;
            for &point in &row_points {
                let current = map.points[point as usize].coord.col;
                let mut best = current;
                let mut best_gap = compute_gap(map, current, &row_points, point);
                let mut candidates = allowed_positions(map, point);
                candidates.sort_unstable();
                for candidate in candidates {
                    if candidate == current {
                        continue;
                    }
                    if let Some(target) = map.get_index(Coord::new(candidate, row))
                        && target != point
                    {
                        continue;
                    }
                    let gap = compute_gap(map, candidate, &row_points, point);
                    if gap > best_gap {
                        best = candidate;
                        best_gap = gap;
                    }
                }
                if best != current {
                    map.move_room_point(point, best);
                    moved = true;
                }
            }
        }
    }
}

fn straighten_paths(map: &mut ActMap) {
    for row in 0..map.map_length {
        for col in 0..MAP_WIDTH {
            let Some(point) = map.get_index(Coord::new(col, row)) else {
                continue;
            };
            if point == map.start
                || point == map.boss
                || map.second_boss == Some(point)
                || map.points[point as usize].parents.len() != 1
                || map.points[point as usize].children.len() != 1
            {
                continue;
            }
            let parent = map.points[point as usize].parents[0];
            let child = map.points[point as usize].children[0];
            let parent_col = map.points[parent as usize].coord.col;
            let child_col = map.points[child as usize].coord.col;
            let point_col = map.points[point as usize].coord.col;
            let bends_left = point_col < child_col && point_col < parent_col;
            let bends_right = point_col > child_col && point_col > parent_col;
            if bends_left
                && col < MAP_WIDTH - 1
                && map.get_index(Coord::new(col + 1, row)).is_none()
            {
                map.move_room_point(point, col + 1);
            }
            if bends_right && col > 0 && map.get_index(Coord::new(col - 1, row)).is_none() {
                map.move_room_point(point, col - 1);
            }
        }
    }
}

/// Frozen-r7 `generate_act_map` for the standard three-act run.
pub fn generate_act_map(
    num_rooms: usize,
    rng: &mut Rng,
    ascension: i32,
    act_index: usize,
) -> ActMap {
    let counts = point_type_counts(rng, ascension, act_index);
    let mut map = ActMap::new(num_rooms);
    let start = map.get_or_create(Coord::new(3, 0));
    let boss = map.get_or_create(Coord::new(3, map.map_length));
    map.start = start;
    map.boss = boss;
    if ascension >= 10 && act_index == 2 {
        let second = map.get_or_create(Coord::new(3, map.map_length + 1));
        map.second_boss = Some(second);
        map.points[second as usize].point_type = MapPointType::Boss;
        map.add_child(boss, second);
    }

    let mut start_points = Vec::with_capacity(MAP_ITERATIONS);
    for iteration in 0..MAP_ITERATIONS {
        let mut start_point = map.get_or_create(Coord::new(rng.next_int(0, 6) as i32, 1));
        if iteration == 1 {
            let mut attempts = 0;
            while start_points.contains(&start_point) && attempts < 100 {
                start_point = map.get_or_create(Coord::new(rng.next_int(0, 6) as i32, 1));
                attempts += 1;
            }
        }
        start_points.push(start_point);
        generate_path(&mut map, start_point, rng);
    }
    for point in map.row_indices(map.map_length - 1) {
        map.add_child(point, boss);
    }
    for point in map.row_indices(1) {
        map.add_child(start, point);
    }

    assign_room_types(&mut map, rng, counts);
    prune_and_repair(&mut map, counts, rng);
    center_grid(&mut map);
    spread_adjacent(&mut map);
    straighten_paths(&mut map);
    map
}
