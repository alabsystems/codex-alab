// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Public record contracts without the application's development dependency graph.

#[cfg(test)]
mod tests {
    use codex_alab_tui::record;
    use codex_alab_tui::record::Record;
    use serde_json::Value;
    use std::path::Path;

    fn clock_value() -> Value {
        // Codec metadata only; this fixture does not observe a native boot or clock.
        serde_json::json!({
            "bootId": "codec-fixture_boot-42",
            "continuousNs": 9_000_000_000_u64,
            "epochMs": 1_000_u64,
        })
    }

    fn record_value() -> Value {
        serde_json::json!({
            "v": 1,
            "pid": 4242,
            "version": "record-contract-fixture",
            "execGeneration": 0,
            "env": {},
            "status": "busy",
            "draftEmpty": true,
            "autopilot": "off",
            "statusUpdatedAt": 1_000,
            "updatedAt": 1_000,
            "kind": "codex",
            "entrypoint": "exec",
            "recovery": {
                "clock": clock_value(),
                "thread": "fixture-thread",
                "turnId": "failed-turn",
                "pointAt": 1_000,
                "notBefore": 1_800_000,
                "byAutopilot": true,
                "byPerson": false,
                "wall": {
                    "kind": "server",
                    "what": "at capacity",
                    "cutOff": false,
                    "retryAt": 1_700_000,
                },
            },
            "inFlightRetry": {
                "clock": clock_value(),
                "thread": "fixture-thread",
                "turnId": "waiting-native-turn",
                "notBefore": 1_800_000,
            },
        })
    }

    fn original_record() -> Record {
        let original: Record = serde_json::from_value(record_value()).expect("valid public record");
        assert!(
            original
                .recovery
                .as_ref()
                .expect("known recovery")
                .clock
                .is_some()
        );
        assert!(
            original
                .in_flight_retry
                .as_ref()
                .expect("known in-flight wait")
                .clock
                .is_some()
        );
        original
    }

    fn without_clock(original: &Record, field: &str) -> Record {
        let mut expected = original.clone();
        match field {
            "recovery" => expected.recovery.as_mut().expect("known recovery").clock = None,
            "inFlightRetry" => {
                expected.in_flight_retry.as_mut().expect("known wait").clock = None;
            }
            _ => panic!("unknown fixture field: {field}"),
        }
        expected
    }

    fn without_wait(original: &Record, field: &str) -> Record {
        let mut expected = original.clone();
        match field {
            "recovery" => expected.recovery = None,
            "inFlightRetry" => expected.in_flight_retry = None,
            _ => panic!("unknown fixture field: {field}"),
        }
        expected
    }

    fn raw_parent(field: &str, parent: &str) -> String {
        let mut value = record_value();
        value[field] = Value::Null;
        let encoded = serde_json::to_string(&value).expect("full record JSON");
        let needle = format!("\"{field}\":null");
        assert_eq!(encoded.matches(&needle).count(), 1);
        // Splice after serialization, preserving duplicate members and raw numbers.
        encoded.replacen(&needle, &format!("\"{field}\":{parent}"), 1)
    }

    fn raw_clock_members(field: &str, members: &str) -> String {
        let mut parent = record_value()[field]
            .as_object()
            .expect("pacing object")
            .clone();
        assert!(parent.remove("clock").is_some());
        let parent = serde_json::to_string(&parent).expect("pacing fields");
        let rest = parent.strip_prefix('{').expect("pacing object opening");
        raw_parent(field, &["{", members, ",", rest].concat())
    }

    fn compare_readers(
        raw: &str,
        expected: Option<&Record>,
        path: &Path,
        context: &str,
        mismatches: &mut Vec<String>,
    ) {
        let direct = serde_json::from_slice::<Record>(raw.as_bytes()).ok();
        std::fs::write(path, raw).expect("private raw record");
        let file = record::read(path);
        for (reader, actual) in [("serde", direct), ("file", file)] {
            if actual.as_ref() != expected {
                mismatches.push(format!("{context}/{reader}"));
            }
        }
    }

    #[test]
    fn valid_record_and_selected_numeric_graph_are_observed() {
        let arbitrary_precision = serde_json::from_str::<Value>("1e400").is_ok();
        match std::env::var("ALAB_RECORD_EXPECT_ARBITRARY_PRECISION") {
            Ok(expected) => {
                assert!(
                    matches!(expected.as_str(), "0" | "1"),
                    "graph guard must be 0 or 1"
                );
                assert_eq!(
                    arbitrary_precision,
                    expected == "1",
                    "selected numeric graph"
                );
            }
            Err(std::env::VarError::NotPresent) => {}
            Err(error) => panic!("invalid numeric graph guard: {error}"),
        }

        let home = tempfile::tempdir().expect("private record directory");
        let original = original_record();
        let raw = serde_json::to_string(&record_value()).expect("minimal raw record");
        let mut mismatches = Vec::new();
        compare_readers(
            &raw,
            Some(&original),
            &home.path().join("valid.json"),
            "valid full record",
            &mut mismatches,
        );
        assert!(
            mismatches.is_empty(),
            "valid record must retain both clocks: {mismatches:?}"
        );
    }

    #[test]
    fn duplicate_clock_keys_and_members_preserve_known_waits() {
        let home = tempfile::tempdir().expect("private record directory");
        let path = home.path().join("duplicate-clocks.json");
        let original = original_record();
        let clock_value = clock_value();
        let clock = serde_json::to_string(&clock_value).expect("valid raw clock");
        let mut other = clock_value.clone();
        other["epochMs"] = serde_json::json!(1_001_u64);
        let other = serde_json::to_string(&other).expect("another valid raw clock");
        let mut mismatches = Vec::new();
        let mut mutants = 0;
        for field in ["recovery", "inFlightRetry"] {
            let valid = raw_clock_members(field, &format!("\"clock\":{clock}"));
            compare_readers(&valid, Some(&original), &path, field, &mut mismatches);
            let expected = without_clock(&original, field);
            for (name, conflicting) in [
                ("bootId", serde_json::json!("different-fixture_boot-43")),
                ("continuousNs", serde_json::json!(9_000_000_001_u64)),
                ("epochMs", serde_json::json!(1_001_u64)),
            ] {
                for (case, duplicate) in [
                    ("identical", clock_value[name].clone()),
                    ("conflicting", conflicting),
                ] {
                    let prefix = clock.strip_suffix('}').expect("clock object closing");
                    let value = serde_json::to_string(&duplicate).expect("duplicate clock value");
                    let raw_clock = format!("{prefix},\"{name}\":{value}}}");
                    assert_eq!(raw_clock.matches(&format!("\"{name}\":")).count(), 2);
                    let raw = raw_clock_members(field, &format!("\"clock\":{raw_clock}"));
                    compare_readers(
                        &raw,
                        Some(&expected),
                        &path,
                        &format!("{field}/{name}/{case}"),
                        &mut mismatches,
                    );
                    mutants += 1;
                }
            }
            for (case, second) in [("identical", &clock), ("conflicting", &other)] {
                let raw =
                    raw_clock_members(field, &format!("\"clock\":{clock},\"clock\":{second}"));
                compare_readers(
                    &raw,
                    Some(&expected),
                    &path,
                    &format!("{field}/repeated clock/{case}"),
                    &mut mismatches,
                );
                mutants += 1;
            }
        }
        assert_eq!(mutants, 16);
        assert!(
            mismatches.is_empty(),
            "ambiguous clocks must retain known waits: {mismatches:?}"
        );
    }

    #[test]
    fn nonclock_fields_keep_legacy_value_semantics() {
        let home = tempfile::tempdir().expect("private record directory");
        let path = home.path().join("nonclock-fields.json");
        let original = original_record();
        let clock = serde_json::to_string(&clock_value()).expect("valid raw clock");
        let mut mismatches = Vec::new();
        for field in ["recovery", "inFlightRetry"] {
            // The original bound remains last; non-clock duplicate members still use Value.
            let raw = raw_clock_members(field, &format!("\"clock\":{clock},\"notBefore\":1"));
            assert!(serde_json::from_str::<Value>(&raw).is_ok());
            compare_readers(
                &raw,
                Some(&original),
                &path,
                &format!("{field}/last bound"),
                &mut mismatches,
            );

            for (case, unknown) in [
                ("ordinary unknown", "{\"nested\":[true,null,42]}"),
                ("large unknown", "1e400"),
            ] {
                let raw = raw_clock_members(
                    field,
                    &format!("\"clock\":{clock},\"futurePacingValue\":{unknown}"),
                );
                let expected = serde_json::from_str::<Value>(&raw)
                    .is_ok()
                    .then_some(&original);
                compare_readers(
                    &raw,
                    expected,
                    &path,
                    &format!("{field}/{case}"),
                    &mut mismatches,
                );
            }

            // A representable value of the wrong known type drops only the optional wait.
            // An unrepresentable number must retain Value's full-record refusal instead.
            let expected = without_wait(&original, field);
            for (case, bound) in [("wrong bound type", "\"later\""), ("large bound", "1e400")] {
                let mut parent = record_value()[field].clone();
                parent["notBefore"] = Value::Null;
                let parent = serde_json::to_string(&parent).expect("parent with bound placeholder");
                assert_eq!(parent.matches("\"notBefore\":null").count(), 1);
                let parent =
                    parent.replacen("\"notBefore\":null", &format!("\"notBefore\":{bound}"), 1);
                let raw = raw_parent(field, &parent);
                let expected = serde_json::from_str::<Value>(&raw)
                    .is_ok()
                    .then_some(&expected);
                compare_readers(
                    &raw,
                    expected,
                    &path,
                    &format!("{field}/{case}"),
                    &mut mismatches,
                );
            }
        }

        // Recursive non-clock maps retain their existing last-value and numeric behavior.
        let raw = raw_clock_members("recovery", &format!("\"clock\":{clock}"));
        let needle = "\"what\":\"at capacity\"";
        assert_eq!(raw.matches(needle).count(), 1);
        let repeated = raw.replacen(
            needle,
            "\"what\":\"discarded earlier\",\"what\":\"at capacity\"",
            1,
        );
        compare_readers(
            &repeated,
            Some(&original),
            &path,
            "nested wall last value",
            &mut mismatches,
        );
        let raw = raw.replacen(
            needle,
            "\"futureWallValue\":1e400,\"what\":\"at capacity\"",
            1,
        );
        let expected = serde_json::from_str::<Value>(&raw)
            .is_ok()
            .then_some(&original);
        compare_readers(
            &raw,
            expected,
            &path,
            "nested wall large unknown",
            &mut mismatches,
        );
        assert!(
            mismatches.is_empty(),
            "non-clock Value semantics changed: {mismatches:?}"
        );
    }

    #[test]
    fn unknown_wait_shapes_keep_legacy_numeric_refusals() {
        let home = tempfile::tempdir().expect("private record directory");
        let path = home.path().join("unknown-wait-shapes.json");
        let original = original_record();
        let mut mismatches = Vec::new();
        for field in ["recovery", "inFlightRetry"] {
            let discarded = without_wait(&original, field);
            for parent in ["[]", "[null]", "[1e400]", "[{\"nested\":1e400}]"] {
                let raw = raw_parent(field, parent);
                // The former Value decoder validated even a shape it would later discard.
                // Feature unification can expand that numeric domain, never bypass this check.
                let expected = serde_json::from_str::<Value>(&raw)
                    .is_ok()
                    .then_some(&discarded);
                compare_readers(
                    &raw,
                    expected,
                    &path,
                    &format!("{field}/{parent}"),
                    &mut mismatches,
                );
            }
        }
        assert!(
            mismatches.is_empty(),
            "unknown wait shapes changed Value refusals: {mismatches:?}"
        );
    }

    #[test]
    fn raw_clock_values_keep_legacy_numeric_depth_and_string_validation() {
        let home = tempfile::tempdir().expect("private record directory");
        let path = home.path().join("raw-clock-validation.json");
        let original = original_record();
        let clock = serde_json::to_string(&clock_value()).expect("valid raw clock");
        let bounded = format!("{}null{}", "[".repeat(20), "]".repeat(20));
        let excessive = format!("{}null{}", "[".repeat(256), "]".repeat(256));
        let cases = [
            ("scalar overflow", "\"clock\":1e400".to_string(), None),
            ("array overflow", "\"clock\":[1e400]".to_string(), None),
            (
                "unknown clock field overflow",
                "\"clock\":{\"futureClockValue\":1e400}".to_string(),
                None,
            ),
            (
                "known clock field overflow",
                "\"clock\":{\"bootId\":\"codec-fixture_boot-42\",\"continuousNs\":1e400,\"epochMs\":1000}".to_string(),
                None,
            ),
            (
                "overflow before repeated valid clock",
                format!("\"clock\":1e400,\"clock\":{clock}"),
                None,
            ),
            (
                "scalar lone surrogate",
                "\"clock\":\"\\ud800\"".to_string(),
                Some(false),
            ),
            (
                "known clock field lone surrogate",
                "\"clock\":{\"bootId\":\"\\ud800\",\"continuousNs\":9000000000,\"epochMs\":1000}".to_string(),
                Some(false),
            ),
            (
                "unknown clock field lone surrogate",
                "\"clock\":{\"futureClockValue\":\"\\ud800\"}".to_string(),
                Some(false),
            ),
            ("bounded clock nesting", format!("\"clock\":{bounded}"), Some(true)),
            ("excessive clock nesting", format!("\"clock\":{excessive}"), Some(false)),
        ];
        let mut mismatches = Vec::new();
        for field in ["recovery", "inFlightRetry"] {
            let valid = raw_clock_members(field, &format!("\"clock\":{clock}"));
            assert!(serde_json::from_str::<Value>(&valid).is_ok());
            compare_readers(
                &valid,
                Some(&original),
                &path,
                &format!("{field}/valid clock"),
                &mut mismatches,
            );
            let without_clock = without_clock(&original, field);
            for (case, members, precondition) in &cases {
                let raw = raw_clock_members(field, members);
                let legacy_accepts = serde_json::from_str::<Value>(&raw).is_ok();
                if let Some(expected) = precondition {
                    assert_eq!(legacy_accepts, *expected, "Value control: {field}/{case}");
                }
                // Invalid timing loses only its clock after the former Value reader's
                // validation succeeds. Numeric capability follows the selected graph.
                let expected = legacy_accepts.then_some(&without_clock);
                compare_readers(
                    &raw,
                    expected,
                    &path,
                    &format!("{field}/{case}"),
                    &mut mismatches,
                );
            }
        }
        assert!(
            mismatches.is_empty(),
            "raw clocks changed Value numeric, depth or string validation: {mismatches:?}"
        );
    }

    #[test]
    fn enclosing_waits_keep_legacy_depth_and_reserved_key_validation() {
        let home = tempfile::tempdir().expect("private record directory");
        let path = home.path().join("enclosing-wait-validation.json");
        let original = original_record();
        let clock = serde_json::to_string(&clock_value()).expect("valid raw clock");
        let arrays = |depth| format!("{}null{}", "[".repeat(depth), "]".repeat(depth));
        let mut mismatches = Vec::new();
        for field in ["recovery", "inFlightRetry"] {
            for (depth, accepted) in [(125, true), (126, false)] {
                let value = arrays(depth);
                let raw = raw_clock_members(
                    field,
                    &format!("\"clock\":{clock},\"futurePacingValue\":{value}"),
                );
                let legacy_accepts = serde_json::from_str::<Value>(&raw).is_ok();
                assert_eq!(
                    legacy_accepts, accepted,
                    "full Value object depth: {field}/{depth}"
                );
                let expected = legacy_accepts.then_some(&original);
                compare_readers(
                    &raw,
                    expected,
                    &path,
                    &format!("{field}/object depth {depth}"),
                    &mut mismatches,
                );
            }

            let discarded = without_wait(&original, field);
            for (depth, accepted) in [(126, true), (127, false)] {
                let raw = raw_parent(field, &arrays(depth));
                let legacy_accepts = serde_json::from_str::<Value>(&raw).is_ok();
                assert_eq!(
                    legacy_accepts, accepted,
                    "full Value array depth: {field}/{depth}"
                );
                let expected = legacy_accepts.then_some(&discarded);
                compare_readers(
                    &raw,
                    expected,
                    &path,
                    &format!("{field}/unknown array depth {depth}"),
                    &mut mismatches,
                );
            }

            for (key, value) in [
                ("$serde_json::private::RawValue", "null"),
                ("$serde_json::private::Number", "0"),
            ] {
                let raw =
                    raw_clock_members(field, &format!("\"{key}\":\"{value}\",\"clock\":{clock}"));
                let legacy_accepts = serde_json::from_str::<Value>(&raw).is_ok();
                if key == "$serde_json::private::RawValue" {
                    assert!(
                        !legacy_accepts,
                        "raw_value is enabled in both selected graphs"
                    );
                }
                // The Number key is ordinary unknown data without arbitrary precision.
                // With that feature, Value applies its reserved-key parsing instead.
                let expected = legacy_accepts.then_some(&original);
                compare_readers(
                    &raw,
                    expected,
                    &path,
                    &format!("{field}/first reserved key {key}"),
                    &mut mismatches,
                );
            }
        }
        assert!(
            mismatches.is_empty(),
            "enclosing waits changed Value depth or reserved-key validation: {mismatches:?}"
        );
    }

    #[test]
    fn raw_value_parent_expansions_keep_waits_but_refuse_clocks() {
        let home = tempfile::tempdir().expect("private record directory");
        let path = home.path().join("raw-value-parent-clocks.json");
        let original = original_record();
        let clock = serde_json::to_string(&clock_value()).expect("valid raw clock");
        let wrap = |payload: &str| {
            // Quote the raw payload as a string; never normalize its duplicate keys.
            let quoted = serde_json::to_string(payload).expect("quoted raw wait payload");
            format!("{{\"$serde_json::private::RawValue\":{quoted}}}")
        };
        let mut mismatches = Vec::new();
        let mut expansions = 0;
        for field in ["recovery", "inFlightRetry"] {
            let canonical =
                serde_json::to_string(&record_value()[field]).expect("canonical wait payload");
            let needle = "\"bootId\":\"codec-fixture_boot-42\"";
            assert_eq!(canonical.matches(needle).count(), 1);
            let repeated_field = canonical.replacen(
                needle,
                "\"bootId\":\"codec-fixture_boot-42\",\"bootId\":\"different-fixture_boot-43\"",
                1,
            );
            let rest = canonical.strip_prefix('{').expect("wait object opening");
            let repeated_clock = format!("{{\"clock\":{clock},{rest}");
            assert_eq!(repeated_clock.matches("\"clock\":").count(), 2);
            let nested_wrapper = wrap(&canonical);
            let expected = without_clock(&original, field);
            let mut expected_fields = record_value()[field].clone();
            expected_fields["clock"] = Value::Null;
            for (case, payload) in [
                ("canonical payload", &canonical),
                ("repeated clock field payload", &repeated_field),
                ("repeated parent clock payload", &repeated_clock),
                ("nested raw-value wrapper", &nested_wrapper),
            ] {
                let raw = raw_parent(field, &wrap(payload));
                let legacy = serde_json::from_str::<Value>(&raw)
                    .expect("legacy Value accepts the encoded wait");
                let mut legacy_fields = legacy[field].clone();
                assert!(legacy_fields.is_object(), "decoded wait: {field}/{case}");
                legacy_fields["clock"] = Value::Null;
                assert_eq!(
                    legacy_fields, expected_fields,
                    "known wait facts: {field}/{case}"
                );
                compare_readers(
                    &raw,
                    Some(&expected),
                    &path,
                    &format!("{field}/{case}"),
                    &mut mismatches,
                );
                expansions += 1;
            }

            // After an ordinary first key the same token is an unknown field,
            // and must not erase a canonical clock whose raw members were audited.
            let quoted = serde_json::to_string(&canonical).expect("quoted canonical payload");
            let raw = raw_clock_members(
                field,
                &format!("\"clock\":{clock},\"$serde_json::private::RawValue\":{quoted}"),
            );
            let legacy = serde_json::from_str::<Value>(&raw)
                .expect("legacy Value accepts the later ordinary key");
            assert_eq!(legacy[field]["clock"], clock_value());
            compare_readers(
                &raw,
                Some(&original),
                &path,
                &format!("{field}/later raw token stays ordinary"),
                &mut mismatches,
            );
        }
        assert_eq!(expansions, 8);
        assert!(
            mismatches.is_empty(),
            "raw-value expansion must retain waits without admitting clocks: {mismatches:?}"
        );
    }
}
