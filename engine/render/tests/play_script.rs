//! The sandbox's scripted checks, headless: `play` run on the scripts in `play-scripts/` queues
//! the commands a player's clicks would, and the Sims' stacks enter the interactions. They need
//! the disc, the home log and its save, named by HF_ISO, HF_HOME_LOG and HF_HOME_CARD (the test
//! says so and passes without them).

use std::path::PathBuf;
use std::process::Command;

fn inputs() -> Option<(String, String, String)> {
    let get = |k: &str| std::env::var(k).ok();
    Some((get("HF_ISO")?, get("HF_HOME_LOG")?, get("HF_HOME_CARD")?))
}

/// `play` on the script, headless: its lines of stdout, and whether it succeeded.
fn play(script: &str) -> Option<(Vec<String>, bool)> {
    let Some((iso, log, card)) = inputs() else {
        eprintln!("skipped: HF_ISO, HF_HOME_LOG and HF_HOME_CARD name the disc, the home log and its save");
        return None;
    };
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("play-scripts").join(script);
    let out = Command::new(env!("CARGO_BIN_EXE_play")).args([&log, &iso, "--card", &card, "--headless", "--script"]).arg(path).output().expect("play runs");
    let lines = String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect();
    Some((lines, out.status.success()))
}

#[test]
fn sim_12_has_a_snack_from_the_fridge() {
    let Some((lines, ok)) = play("fridge.play") else { return };
    // the command: target 62 (the fridge), action 8215, as the menu's entry gives it
    assert!(lines.iter().any(|l| l == "play: tick 10 sim 12 target 62 action 8215 (Have A Snack)"), "{lines:#?}");
    assert!(lines.iter().any(|l| l.starts_with("play: expect sim 12 tree 8215: ok")), "{lines:#?}");
    assert!(ok);
}

#[test]
fn sim_237_is_sent_to_a_point_of_the_floor() {
    let Some((lines, ok)) = play("gohere.play") else { return };
    assert!(lines.iter().any(|l| l.starts_with("play: tick 10 sim 237 target ") && l.ends_with(" action 4096 (go here)")), "{lines:#?}");
    assert!(lines.iter().any(|l| l.starts_with("play: expect sim 237 tree 4096: ok")), "{lines:#?}");
    assert!(ok);
}
