//! agent-info must match reality: valid against the vendored framework
//! schema, every listed command routable, and the domain extras (models,
//! costs, breaking changes) in sync with the actual CLI surface.

mod common;
use common::{agent_info, suno, vendored_schema};

#[test]
fn manifest_validates_against_vendored_schema() {
    let schema = vendored_schema("agent-info.schema.json");
    let info = agent_info();
    if let Err(e) = jsonschema::validate(&schema, &info) {
        panic!("agent-info violates the vendored framework schema: {e}");
    }
}

#[test]
fn name_and_version_match_the_binary() {
    let info = agent_info();
    assert_eq!(info["name"], "suno");
    assert_eq!(info["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn every_listed_command_is_routable() {
    // Command keys may be multi-word ("config show"); each must accept
    // --help with exit 0 or the manifest is advertising a phantom command.
    let info = agent_info();
    for cmd in info["commands"].as_object().unwrap().keys() {
        let mut args: Vec<&str> = cmd.split_whitespace().collect();
        args.push("--help");
        let out = suno().args(&args).output().unwrap();
        assert!(
            out.status.success(),
            "command listed in agent-info is not routable: {cmd}"
        );
    }
}

#[test]
fn advertised_options_exist_in_help() {
    // Assert the manifest's options against the real clap surface, not just
    // routability: every advertised long flag must appear in that command's
    // --help. Catches manifest drift (e.g. extend's new --force) and dead
    // flags the manifest never should have listed.
    let info = agent_info();
    for (cmd, spec) in info["commands"].as_object().unwrap() {
        let Some(options) = spec["options"].as_array() else {
            continue;
        };
        let mut args: Vec<&str> = cmd.split_whitespace().collect();
        args.push("--help");
        let out = suno().args(&args).output().unwrap();
        let help = String::from_utf8_lossy(&out.stdout);
        for opt in options {
            let name = opt["name"].as_str().unwrap();
            if name.starts_with("--") {
                assert!(
                    help.contains(name),
                    "manifest advertises {name} for `{cmd}` but --help does not show it"
                );
            }
        }
    }
}

#[test]
fn exit_codes_cover_0_to_4_and_5_is_gone() {
    let info = agent_info();
    let codes = info["exit_codes"].as_object().unwrap();
    for code in ["0", "1", "2", "3", "4"] {
        assert!(codes[code].is_string(), "must document exit code {code}");
    }
    // Code 5 (not found) was removed in 0.6.0 — not-found is now 3.
    assert!(
        !codes.contains_key("5"),
        "exit code 5 must not be documented"
    );
}

#[test]
fn models_map_includes_v4_5_all() {
    // The account-selectable "best free model" was missing from the enum
    // through 0.5.x; the manifest and --model must both know it.
    let info = agent_info();
    assert_eq!(info["models"]["v4.5-all"], "chirp-auk-turbo");

    let out = suno().args(["generate", "--help"]).output().unwrap();
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(help.contains("v4.5-all"), "--model must accept v4.5-all");
}

#[test]
fn models_map_matches_model_flag_values() {
    // Every model in the manifest must be selectable via --model, and the
    // dead --variation flag must stay dead.
    let info = agent_info();
    let out = suno().args(["generate", "--help"]).output().unwrap();
    let help = String::from_utf8_lossy(&out.stdout);
    for model in info["models"].as_object().unwrap().keys() {
        assert!(help.contains(model.as_str()), "--model missing {model}");
    }
    assert!(
        !help.contains("--variation"),
        "--variation was removed in 0.6.0"
    );
}

#[test]
fn generation_cost_documented_as_10() {
    // Measured live 2026-08-30 on a Premier plan: two consecutive default
    // v5.5 calls cost exactly 10 credits each (5/clip, 2 clips), read from
    // `suno credits` before and after each call — 8770 -> 8760 -> 8750.
    //
    // This contradicts a 2026-07-18 note claiming 70/call, which is what the
    // figure was changed to. Only one of the two can describe a given
    // account, so if an agent's budget math goes wrong here, re-measure
    // rather than trusting either number: the cost may well vary by plan.
    let info = agent_info();
    let cost = serde_json::to_string(&info["generation_cost"]).unwrap();
    assert!(
        cost.contains("10"),
        "generation_cost must state ~10 credits"
    );
}

#[test]
fn breaking_changes_document_the_exit_code_remap() {
    let info = agent_info();
    let note = info["breaking_changes"]["0.6.0"]
        .as_str()
        .expect("0.6.0 breaking-change note must exist");
    assert!(
        note.contains("Exit codes"),
        "must mention the exit-code remap"
    );
}

#[test]
fn config_metadata_present() {
    let info = agent_info();
    assert!(info["config"]["path"].is_string());
    assert_eq!(info["config"]["env_prefix"], "SUNO_");
}
