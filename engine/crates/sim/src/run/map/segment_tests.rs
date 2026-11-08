use super::*;

fn reference(map:&ActMap)->Vec<Vec<Vec<u8>>> {
    fn walk(map:&ActMap,current:u8,path:&mut Vec<u8>,buckets:&mut BTreeMap<String,Vec<Vec<u8>>>) {
        path.push(current);
        if path.len()>=3 && valid_segment_end(map,current) {
            let bucket=buckets.entry(segment_key(map,path)).or_default();
            if !bucket.iter().any(|existing| {
                let limit=path.len().min(existing.len()).saturating_sub(1);
                (1..limit).any(|i|existing[i]==path[i])
            }) {bucket.push(path.clone());}
        }
        for &child in &map.points[current as usize].children {walk(map,child,path,buckets);}
        path.pop();
    }
    let mut starts=vec![];
    reachable_dfs(map,map.start,&mut vec![],&mut starts);
    let mut buckets=BTreeMap::new();
    for start in starts {walk(map,start,&mut vec![],&mut buckets);}
    buckets.into_values().filter(|bucket|bucket.len()>1).collect()
}

#[test]
fn compact_segment_buckets_preserve_ordinal_order_and_exact_path_membership() {
    for seed in 0..64 {
        let mut rng=Rng::new(20261008000000+seed);
        let mut map=ActMap::new(15);
        map.start=map.get_or_create(Coord::new(3,0));
        let mut previous=vec![map.start];
        for row in 1..=8 {
            let current:Vec<_>=(0..3).map(|col|map.get_or_create(Coord::new(col,row))).collect();
            for &parent in &previous {for &child in &current {
                if rng.next_int(0,2)!=0 {map.add_child(parent,child);}
            }}
            for &point in &current {
                map.points[point as usize].point_type=if rng.next_int(0,1)==0 {MapPointType::Monster}else{MapPointType::Unknown};
            }
            previous=current;
        }
        assert_eq!(matching_segments(&map),reference(&map),"seed {seed}");
    }
}
