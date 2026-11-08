//! Bit-exact check of `sts2_sim::rng` against vectors exported from the frozen r7 rng.py
//! (tools/r7_rng_vectors.py).
use serde_json::Value;
use sts2_sim::rng::{deterministic_hash64, deterministic_hash_code_old, DotNetRandom, Rng};

fn vectors() -> Value {
    serde_json::from_str(include_str!("data/rng_vectors.json")).unwrap()
}

#[test]
fn name_hashes_match_r7() {
    let v = vectors();
    for (name, expected) in v["hash64"].as_object().unwrap() {
        let want: u64 = expected.as_str().unwrap().parse().unwrap();
        assert_eq!(deterministic_hash64(name), want, "hash64 {name:?}");
    }
    for (name, expected) in v["hash_old"].as_object().unwrap() {
        assert_eq!(deterministic_hash_code_old(name) as i64, expected.as_i64().unwrap(), "hash_old {name:?}");
    }
}

#[test]
fn streams_match_r7() {
    let v = vectors();
    for stream in v["streams"].as_array().unwrap() {
        let seed: u64 = stream["seed"].as_str().unwrap().parse().unwrap();
        let mut r = match stream["name"].as_str() {
            Some(name) => Rng::named(seed, name),
            None => Rng::new(seed),
        };
        for (i, op) in stream["seq"].as_array().unwrap().iter().enumerate() {
            let op = op.as_array().unwrap();
            let ctx = format!("seed {seed} step {i} {op:?}");
            match op[0].as_str().unwrap() {
                "int" => assert_eq!(r.next_int(op[1].as_i64().unwrap(), op[2].as_i64().unwrap()), op[3].as_i64().unwrap(), "{ctx}"),
                "bool" => assert_eq!(r.next_bool(), op[1].as_bool().unwrap(), "{ctx}"),
                "double" => {
                    let want: f64 = op[1].as_str().unwrap().parse().unwrap();
                    assert_eq!(r.next_double(0.0, 1.0).to_bits(), want.to_bits(), "{ctx}");
                }
                "float_range" => {
                    let want: f64 = op[3].as_str().unwrap().parse().unwrap();
                    let got = r.next_float_range(op[1].as_f64().unwrap(), op[2].as_f64().unwrap());
                    assert_eq!(got.to_bits(), want.to_bits(), "{ctx}");
                }
                "shuffle10" => {
                    let mut items: Vec<i64> = (0..10).collect();
                    r.shuffle(&mut items);
                    let want: Vec<i64> = op[1].as_array().unwrap().iter().map(|x| x.as_i64().unwrap()).collect();
                    assert_eq!(items, want, "{ctx}");
                }
                "gauss" => assert_eq!(r.next_gaussian_int(5.0, 2.0, 0, 10), op[1].as_i64().unwrap(), "{ctx}"),
                other => panic!("unknown op {other}"),
            }
        }
        assert_eq!(r.counter, stream["counter"].as_u64().unwrap(), "counter seed {seed}");
        let state: Vec<u64> = stream["state"].as_array().unwrap().iter().map(|w| w.as_str().unwrap().parse().unwrap()).collect();
        assert_eq!(r.inner.s.to_vec(), state, "state seed {seed}");
    }
}

#[test]
fn dotnet_random_matches_r7() {
    let v = vectors();
    for case in v["dotnet"].as_array().unwrap() {
        let mut d = DotNetRandom::new(case["seed"].as_i64().unwrap() as i32);
        for want in case["samples"].as_array().unwrap() {
            assert_eq!(d.internal_sample() as i64, want.as_i64().unwrap(), "dotnet seed {}", case["seed"]);
        }
    }
}
