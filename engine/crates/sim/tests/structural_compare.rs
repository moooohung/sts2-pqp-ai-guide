#[path = "support/structural_compare.rs"]
mod structural_compare;
use structural_compare::*;

#[test]
fn every_boundary_and_failure_category_survives() {
    let frame = Frame { actions: vec![1, 2], obs: vec![3], state: vec![4], rng: vec![5, 0] };
    let a = Run { frames: vec![frame], outcome: Outcome::Match };
    assert_eq!(compare(&a, &a.clone()), Ok(()));
    for field in ["actions", "obs", "state", "rng"] {
        let mut b = a.clone();
        match field {
            "actions" => b.frames[0].actions.push(0),
            "obs" => b.frames[0].obs[0] ^= 1,
            "state" => b.frames[0].state.clear(),
            _ => b.frames[0].rng[1] = 1,
        }
        assert!(matches!(&compare(&a, &b).unwrap_err()[0], Difference::Bytes { field: f, .. } if *f == field));
    }
    let outcomes = [
        Outcome::Match,
        Outcome::Fault { decision: 1, reason: "unported".into() },
        Outcome::Mismatch { decision: 1, reason: "state".into() },
        Outcome::CSharp { decision: 1, reason: "source".into() },
    ];
    for left in &outcomes { for right in &outcomes {
        let mut x = a.clone(); x.outcome = left.clone();
        let mut y = a.clone(); y.outcome = right.clone();
        assert_eq!(compare(&x, &y).is_ok(), left == right);
    }}
    let mut b = a.clone(); b.frames.clear();
    assert!(matches!(compare(&a, &b).unwrap_err()[0], Difference::FrameCount { .. }));
}
