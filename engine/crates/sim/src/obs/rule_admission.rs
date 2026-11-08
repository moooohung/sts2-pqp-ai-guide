use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

const EVIDENCE_TSV: &str = include_str!("rule_admission.tsv");

const HEADER: [&str; 12] = [
    "kind",
    "source",
    "comparable",
    "matches",
    "distinct_seeds",
    "counterexamples",
    "not_comparable",
    "unclassified_gameplay_differences",
    "unclassified_wire_differences",
    "natural_count",
    "targeted_count",
    "proof_revision",
];

const KNOWN_KINDS: [&str; 10] = [
    "run_selector",
    "run_selector_inventory",
    "run_selector_transplant_v1",
    "run_selector_transplant_inventory_v1",
    "event_option_table",
    "public_power_counter",
    "combat_choice_prompt",
    "extra_reward",
    sts2_wire::vocab::SPECIAL_CARD_EVIDENCE,
    sts2_wire::vocab::NEOWS_FURY_EVIDENCE,
];

const MIN_COMPARABLE: u64 = 200;
const MIN_DISTINCT_SEEDS: u64 = 50;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmissionRow {
    pub kind: String,
    pub source: String,
    pub comparable: u64,
    pub matches: u64,
    pub distinct_seeds: u64,
    pub counterexamples: u64,
    pub not_comparable: u64,
    pub unclassified_gameplay_differences: u64,
    pub unclassified_wire_differences: u64,
    pub natural_count: u64,
    pub targeted_count: u64,
    pub proof_revision: String,
}

impl AdmissionRow {
    fn qualifies(&self) -> bool {
        self.comparable >= MIN_COMPARABLE
            && self.distinct_seeds >= MIN_DISTINCT_SEEDS
            && self.counterexamples == 0
            && self.unclassified_gameplay_differences == 0
            && self.unclassified_wire_differences == 0
            && self.matches == self.comparable
            && self.natural_count.saturating_add(self.targeted_count) == self.comparable
    }
}

#[derive(Debug)]
struct AdmissionTable {
    rows: Vec<AdmissionRow>,
    by_key: HashMap<(String, String), usize>,
}

impl AdmissionTable {
    fn admitted(&self, kind: &str, source: &str) -> bool {
        // Transplanting into the legacy encoder cannot certify r8-only actions.
        if matches!(kind, "run_selector_transplant_v1" | "run_selector_transplant_inventory_v1")
            && matches!(source, "reward:SPECIAL_CARD" | "card:NEOWS_FURY")
        {
            return false;
        }
        // A frozen-r7 wire certificate must never admit an r8-only action.
        let kind = if kind == "extra_reward" && source == "reward:SPECIAL_CARD" {
            sts2_wire::vocab::SPECIAL_CARD_EVIDENCE
        } else if kind == "combat_choice_prompt" && source == "card:NEOWS_FURY" {
            sts2_wire::vocab::NEOWS_FURY_EVIDENCE
        } else { kind };
        if !known_kind(kind) || source.is_empty() {
            return false;
        }
        self.by_key
            .get(&(kind.to_owned(), source.to_owned()))
            .and_then(|&index| self.rows.get(index))
            .is_some_and(AdmissionRow::qualifies)
    }
}

fn known_kind(kind: &str) -> bool {
    KNOWN_KINDS.contains(&kind)
}

fn parse_u64(value: &str) -> Result<u64, ()> {
    value.parse::<u64>().map_err(|_| ())
}

fn parse_table(tsv: &str) -> Result<AdmissionTable, ()> {
    let mut data_lines = tsv
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'));

    let header = data_lines.next().ok_or(())?;
    let header_columns = header.split('\t').collect::<Vec<_>>();
    if header_columns.as_slice() != HEADER {
        return Err(());
    }

    let mut rows = Vec::new();
    let mut by_key = HashMap::new();
    let mut seen = HashSet::new();

    for line in data_lines {
        let columns = line.split('\t').map(str::trim).collect::<Vec<_>>();
        if columns.len() != HEADER.len() {
            return Err(());
        }

        let kind = columns[0];
        let source = columns[1];
        let proof_revision = columns[11];
        if !known_kind(kind) || source.is_empty() || proof_revision.is_empty() {
            return Err(());
        }

        let key = (kind.to_owned(), source.to_owned());
        if !seen.insert(key.clone()) {
            return Err(());
        }

        let row = AdmissionRow {
            kind: key.0.clone(),
            source: key.1.clone(),
            comparable: parse_u64(columns[2])?,
            matches: parse_u64(columns[3])?,
            distinct_seeds: parse_u64(columns[4])?,
            counterexamples: parse_u64(columns[5])?,
            not_comparable: parse_u64(columns[6])?,
            unclassified_gameplay_differences: parse_u64(columns[7])?,
            unclassified_wire_differences: parse_u64(columns[8])?,
            natural_count: parse_u64(columns[9])?,
            targeted_count: parse_u64(columns[10])?,
            proof_revision: proof_revision.to_owned(),
        };

        by_key.insert(key, rows.len());
        rows.push(row);
    }

    Ok(AdmissionTable { rows, by_key })
}

fn table() -> Option<&'static AdmissionTable> {
    static TABLE: OnceLock<Result<AdmissionTable, ()>> = OnceLock::new();
    TABLE
        .get_or_init(|| parse_table(EVIDENCE_TSV))
        .as_ref()
        .ok()
}

pub fn admitted(kind: &str, source: &str) -> bool {
    table().is_some_and(|table| table.admitted(kind, source))
}

pub fn parsed_rows() -> Option<&'static [AdmissionRow]> {
    table().map(|table| table.rows.as_slice())
}

/// Held-out quarantine episodes are diagnosis-only, never admission inputs.
pub const HELD_OUT_SEED_START:u64=20_261_007_000_000;
pub const HELD_OUT_SEED_END:u64=20_261_007_999_999;

pub fn admission_seed_allowed(seed:u64)->bool {
    !(HELD_OUT_SEED_START..=HELD_OUT_SEED_END).contains(&seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_out_census_seed_interval_is_excluded_inclusively() {
        assert!(admission_seed_allowed(HELD_OUT_SEED_START-1));
        assert!(!admission_seed_allowed(HELD_OUT_SEED_START));
        assert!(!admission_seed_allowed(HELD_OUT_SEED_END));
        assert!(admission_seed_allowed(HELD_OUT_SEED_END+1));
    }

    const TEST_HEADER: &str = "kind\tsource\tcomparable\tmatches\tdistinct_seeds\tcounterexamples\tnot_comparable\tunclassified_gameplay_differences\tunclassified_wire_differences\tnatural_count\ttargeted_count\tproof_revision\n";

    fn row(comparable: u64, distinct_seeds: u64, counterexamples: u64) -> String {
        format!(
            "run_selector\tevent:TEST\t{comparable}\t{comparable}\t{distinct_seeds}\t{counterexamples}\t0\t0\t0\t{comparable}\t0\ttest-revision\n"
        )
    }

    fn admitted_from(tsv: &str, kind: &str, source: &str) -> bool {
        parse_table(tsv)
            .ok()
            .is_some_and(|table| table.admitted(kind, source))
    }

    #[test]
    fn valid_200_50_0_is_admitted() {
        let input = format!("{TEST_HEADER}{}", row(200, 50, 0));
        assert!(admitted_from(&input, "run_selector", "event:TEST"));
    }

    #[test]
    fn transplant_kinds_are_separate_and_keep_the_same_thresholds() {
        for kind in ["run_selector_transplant_v1", "run_selector_transplant_inventory_v1"] {
            for (count, seeds, errors, expected) in [
                (200, 50, 0, true), (199, 50, 0, false),
                (200, 49, 0, false), (200, 50, 1, false),
            ] {
                let input = format!("{TEST_HEADER}{}", row(count, seeds, errors)
                    .replacen("run_selector", kind, 1));
                assert_eq!(admitted_from(&input, kind, "event:TEST"), expected);
                assert!(!admitted_from(&input, "run_selector", "event:TEST"));
                let other = if kind == "run_selector_transplant_v1" {
                    "run_selector_transplant_inventory_v1"
                } else { "run_selector_transplant_v1" };
                assert!(!admitted_from(&input, other, "event:TEST"));
                for source in ["reward:SPECIAL_CARD", "card:NEOWS_FURY"] {
                    let forbidden = input.replace("event:TEST", source);
                    assert!(!admitted_from(&forbidden, kind, source));
                }
            }
        }
    }

    #[test]
    fn special_card_requires_original_game_r8_v1_evidence() {
        for (comparable, seeds, counterexamples, expected) in [
            (200,50,0,true), (199,50,0,false), (200,49,0,false), (200,50,1,false),
        ] {
            let fields=format!("reward:SPECIAL_CARD\t{comparable}\t{comparable}\t{seeds}\t{counterexamples}\t0\t0\t0\t0\t{comparable}\tdll-witness-test\n");
            let r7=format!("{TEST_HEADER}extra_reward\t{fields}");
            assert!(!admitted_from(&r7,"extra_reward","reward:SPECIAL_CARD"));
            let dll=format!("{TEST_HEADER}{}\t{fields}",sts2_wire::vocab::SPECIAL_CARD_EVIDENCE);
            assert_eq!(admitted_from(&dll,"extra_reward","reward:SPECIAL_CARD"),expected);
        }
    }

    #[test]
    fn fury_requires_original_game_v2_evidence_not_legacy_prompt_evidence() {
        for (comparable, seeds, counterexamples, expected) in [
            (200,50,0,true), (199,50,0,false), (200,49,0,false), (200,50,1,false),
        ] {
            let fields=format!("card:NEOWS_FURY\t{comparable}\t{comparable}\t{seeds}\t{counterexamples}\t0\t0\t0\t0\t{comparable}\tdll-witness-test\n");
            let legacy=format!("{TEST_HEADER}combat_choice_prompt\t{fields}");
            assert!(!admitted_from(&legacy,"combat_choice_prompt","card:NEOWS_FURY"));
            let dll=format!("{TEST_HEADER}{}\t{fields}",sts2_wire::vocab::NEOWS_FURY_EVIDENCE);
            assert_eq!(admitted_from(&dll,"combat_choice_prompt","card:NEOWS_FURY"),expected);
        }
        assert!(admitted("combat_choice_prompt","card:NEOWS_FURY"));
    }

    #[test]
    fn comparable_199_is_rejected() {
        let input = format!("{TEST_HEADER}{}", row(199, 50, 0));
        assert!(!admitted_from(&input, "run_selector", "event:TEST"));
    }

    #[test]
    fn distinct_seeds_49_is_rejected() {
        let input = format!("{TEST_HEADER}{}", row(200, 49, 0));
        assert!(!admitted_from(&input, "run_selector", "event:TEST"));
    }

    #[test]
    fn counterexample_is_rejected() {
        let input = format!("{TEST_HEADER}{}", row(200, 50, 1));
        assert!(!admitted_from(&input, "run_selector", "event:TEST"));
    }

    #[test]
    fn malformed_row_fails_closed() {
        let input = format!("{TEST_HEADER}run_selector\tevent:TEST\t200\t200\t50\t0\t0\t0\t0\t200\t0\n");
        assert!(parse_table(&input).is_err());
        assert!(!admitted_from(&input, "run_selector", "event:TEST"));
    }

    #[test]
    fn duplicate_row_fails_closed() {
        let evidence = row(200, 50, 0);
        let input = format!("{TEST_HEADER}{evidence}{evidence}");
        assert!(parse_table(&input).is_err());
        assert!(!admitted_from(&input, "run_selector", "event:TEST"));
    }

    #[test]
    fn unknown_kind_and_unlisted_source_are_rejected() {
        let input = format!("{TEST_HEADER}{}", row(200, 50, 0));
        assert!(!admitted_from(&input, "unknown_kind", "event:TEST"));
        assert!(!admitted_from(&input, "run_selector", "event:UNLISTED"));

        let unknown_row = format!(
            "{TEST_HEADER}unknown_kind\tevent:TEST\t200\t200\t50\t0\t0\t0\t0\t200\t0\ttest-revision\n"
        );
        assert!(parse_table(&unknown_row).is_err());
    }

    #[test]
    fn inconsistent_counts_fail_closed() {
        let mismatched = format!(
            "{TEST_HEADER}run_selector\tevent:TEST\t200\t199\t50\t0\t0\t0\t0\t200\t0\ttest-revision\n"
        );
        assert!(!admitted_from(&mismatched, "run_selector", "event:TEST"));
        let unclassified = format!(
            "{TEST_HEADER}run_selector\tevent:TEST\t200\t200\t50\t0\t0\t1\t0\t200\t0\ttest-revision\n"
        );
        assert!(!admitted_from(&unclassified, "run_selector", "event:TEST"));
        let unclassified_wire = format!(
            "{TEST_HEADER}run_selector\tevent:TEST\t200\t200\t50\t0\t0\t0\t1\t200\t0\ttest-revision\n"
        );
        assert!(!admitted_from(&unclassified_wire, "run_selector", "event:TEST"));
        let provenance = format!(
            "{TEST_HEADER}run_selector\tevent:TEST\t200\t200\t50\t0\t0\t0\t0\t199\t0\ttest-revision\n"
        );
        assert!(!admitted_from(&provenance, "run_selector", "event:TEST"));
    }

    #[test]
    fn embedded_admission_table_parses_and_rows_qualify() {
        let table = parse_table(EVIDENCE_TSV).expect("embedded evidence TSV must be well formed");
        assert!(!table.rows.is_empty());
        assert!(table.rows.iter().all(AdmissionRow::qualifies));
    }

    #[test]
    fn orobas_whole_screen_sources_have_qualified_admission() {
        for source in ["event.orobas.enchant", "event.orobas.add"] {
            assert!(admitted("run_selector", source), "{source}");
            assert!(admitted("run_selector_inventory", source), "{source}");
        }
    }

    #[test]
    fn reward_followups_without_whole_screen_evidence_fail_closed() {
        assert!(!admitted("extra_reward", "reward:SPECIAL_CARD"));
        for source in [
            "relic:YUMMY_COOKIE", "relic:BIIIG_HUG",
            "relic.paels_tooth.pickup", "relic:ASTROLABE",
        ] {
            assert!(!admitted("run_selector", source), "{source}");
            assert!(!admitted("run_selector_inventory", source), "{source}");
        }
    }
}
