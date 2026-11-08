use super::*;

// C# Map/SpoilsActMap.cs. Unlike StandardActMap, the named RNG is independent
// of act_N_map, paths converge at one fixed treasure, and no centering,
// spreading or straightening postprocess is applied.
pub fn generate_spoils_act_map(
    num_rooms: usize,
    run_seed: u64,
    ascension: i32,
    act_index: usize,
) -> ActMap {
    let mut rng = Rng::named(run_seed, "spoils_map");
    let counts = point_type_counts(&mut rng, ascension, act_index);
    let mut map = ActMap::new(num_rooms);
    map.start = map.get_or_create(Coord::new(3, 0));
    map.boss = map.get_or_create(Coord::new(3, map.map_length));
    let treasure_row = map.map_length - 7;
    assert!(treasure_row > 0 && treasure_row < map.map_length);
    let mut starts = Vec::new();
    for iteration in 0..7 {
        let mut point = map.get_or_create(Coord::new(rng.next_int_exclusive(0, 7) as i32, 1));
        if iteration == 1 {
            while starts.contains(&point) {
                point = map.get_or_create(Coord::new(rng.next_int_exclusive(0, 7) as i32, 1));
            }
        }
        starts.push(point);
        while map.points[point as usize].coord.row < map.map_length - 1 {
            let next = next_coord(&map, point, treasure_row, &mut rng);
            let child = map.get_or_create(next);
            map.add_child(point, child);
            point = child;
        }
    }
    let treasure = map.get_or_create(Coord::new(3, treasure_row));
    map.points[treasure as usize].point_type = MapPointType::Treasure;
    map.points[treasure as usize].can_be_modified = false;
    // RedirectToTreasure transfers both sets of edges before dropping the cell.
    for stray in map.row_indices(treasure_row) {
        if stray == treasure { continue; }
        for parent in map.points[stray as usize].parents.clone() {
            map.add_child(parent, treasure);
        }
        for child in map.points[stray as usize].children.clone() {
            map.add_child(treasure, child);
        }
        map.remove_point(stray);
    }
    for point in map.row_indices(map.map_length - 1) {
        map.add_child(point, map.boss);
    }
    for point in map.row_indices(1) {
        map.add_child(map.start, point);
    }
    // AssignPointTypes and its validity restrictions match the standard helper;
    // the only treasure cell is already fixed by GenerateHourglassMap.
    assign_room_types(&mut map, &mut rng, counts);
    prune_and_repair(&mut map, counts, &mut rng);
    map
}

fn next_coord(map: &ActMap, current: u8, treasure_row: i32, rng: &mut Rng) -> Coord {
    let coord = map.points[current as usize].coord;
    let row = coord.row + 1;
    // SpoilsActMap.GetAllowedColumnsForRow.
    let distance = (row - treasure_row).abs();
    let tip_width = 3.min((map.map_length - 1 - row).max(0) + 1);
    let radius = 3.min(distance.min(tip_width));
    let (min_col, max_col) = ((3 - radius).max(0), (3 + radius).min(6));
    let rows_to_treasure = treasure_row - coord.row;
    let mut directions = vec![-1, 0, 1];
    if rows_to_treasure > 0 && rows_to_treasure <= 3 {
        // BuildCenteredPriorityList: centerward, straight, away; when already
        // centered, straight, left, right. No RNG draw in this region.
        let toward = (3 - coord.col).signum();
        directions.clear();
        if toward != 0 { directions.push(toward); }
        directions.push(0);
        if toward != 0 { directions.push(-toward); }
        if !directions.contains(&-1) { directions.push(-1); }
        if !directions.contains(&1) { directions.push(1); }
    } else {
        rng.shuffle(&mut directions);
    }
    for direction in directions {
        let col = (coord.col + direction).clamp(0, 6);
        if col < min_col || col > max_col || invalid_crossover(map, current, col) {
            continue;
        }
        let target = map.get_index(Coord::new(col, row));
        let accepts_parent = target.is_none_or(|point| {
            let parents = &map.points[point as usize].parents;
            parents.contains(&current) || parents.len() < 3
        });
        let children = &map.points[current as usize].children;
        let accepts_child = current == map.start || children.len() < 3
            || target.is_some_and(|point| children.contains(&point));
        if accepts_parent && accepts_child {
            assert!((col - coord.col).abs() <= 1);
            return Coord::new(col, row);
        }
    }
    // GenerateNextCoord's exact fallback, including its crossover check.
    let mut col = 3.clamp(min_col, max_col);
    if (col - coord.col).abs() > 1 {
        col = (coord.col + (col - coord.col).signum()).clamp(min_col, max_col);
    }
    if invalid_crossover(map, current, col) {
        col = coord.col.clamp(min_col, max_col);
    }
    assert!((col - coord.col).abs() <= 1);
    Coord::new(col, row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spoils_seed_4100134_matches_the_frozen_hourglass_start_row() {
        // Independent frozen generator.py::generate_spoils_act_map output:
        // run seed 2658962003667727030, Hive/A10, 14 rooms. The standard map's
        // [0,2,4,6] caused the first run difference at decision 319.
        let map = generate_spoils_act_map(14, 2658962003667727030, 10, 1);
        let columns: Vec<_> = map.row_indices(1).iter()
            .map(|&i| map.points[i as usize].coord.col).collect();
        assert_eq!(columns, [0, 3, 5, 6]);
    }

    #[test]
    fn spoils_map_has_csharp_hourglass_constraints_without_standard_postprocess() {
        for seed in 0..64 {
            let map = generate_spoils_act_map(14, seed, 10, 1);
            let treasure_row = map.map_length - 7;
            let treasure = map.row_indices(treasure_row);
            assert_eq!(treasure.len(), 1);
            let point = &map.points[treasure[0] as usize];
            assert_eq!(point.coord, Coord::new(3, treasure_row));
            assert_eq!(point.point_type, MapPointType::Treasure);
            assert!(!point.can_be_modified);
            for i in map.room_indices() {
                let point = &map.points[i as usize];
                let row = point.coord.row;
                let radius = 3.min((row - treasure_row).abs()
                    .min(3.min((map.map_length - 1 - row).max(0) + 1)));
                assert!((point.coord.col - 3).abs() <= radius);
                for &child in &point.children {
                    let next = &map.points[child as usize];
                    assert_eq!(next.coord.row, row + 1);
                    assert!((point.coord.col - next.coord.col).abs() <= 1);
                    assert!(next.parents.contains(&i));
                }
                if row == 1 { assert_eq!(point.point_type, MapPointType::Monster); }
                if row == map.map_length - 1 {
                    assert_eq!(point.point_type, MapPointType::RestSite);
                    assert!(point.children.contains(&map.boss));
                }
            }
        }
    }
}
