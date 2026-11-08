use super::*;
use crate::run::{RunManager, RunOptions};

#[test]
fn instanced_listeners_interleave_with_other_classes_in_application_order() {
    let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    let first = c.apply_power_to(0, PowerId::Cacophony, 66, Some(0)).unwrap();
    let second = c.apply_power_to(0, PowerId::Cacophony, 99, Some(0)).unwrap();
    let other = c.apply_power_to(0, PowerId::Pagestorm, 1, Some(0)).unwrap();
    assert_eq!(c.listeners_of(0, Hook::OnCardDrawn).as_slice(), &[first, second, other]);
    c.remove_power(0, first);
    assert_eq!(c.listeners_of(0, Hook::OnCardDrawn).as_slice(), &[second, other]);
    assert_eq!(c.fault, None);
}

#[test]
fn removing_primary_instance_does_not_move_promoted_instance_before_older_power() {
    let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    let first = c.apply_power_to(0, PowerId::Cacophony, 66, Some(0)).unwrap();
    let other = c.apply_power_to(0, PowerId::Pagestorm, 1, Some(0)).unwrap();
    let second = c.apply_power_to(0, PowerId::Cacophony, 99, Some(0)).unwrap();
    c.remove_power(0, first);
    assert!(c.creatures[0].extra.is_empty());
    assert_eq!(c.creatures[0].power_list().as_slice(), &[other, second]);
    assert_eq!(c.listeners_of(0, Hook::OnCardDrawn).as_slice(), &[other, second]);
}
